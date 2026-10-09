use std::{
    io::{ErrorKind, Read, Write},
    net::TcpStream,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Router,
    body::Body,
    extract::{
        State,
        ws::{WebSocketUpgrade, rejection::WebSocketUpgradeRejection},
    },
    http::{Method, Request, StatusCode},
    response::Response,
    routing::get,
};
use chrono::DateTime;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tokio::{net::TcpListener, time::Instant};
use tokio_tungstenite::{
    MaybeTlsStream, connect_async,
    tungstenite::{Error as ErrorWs, Message, WebSocket as WebSocketSincrono, protocol::Role},
};
use tower::ServiceExt;

use crate::modules::actividad::{
    application::conexion::atender_conexion_con_keepalive, domain::EVENTO_REPORTE_CREADO,
    infra::CanalActividad,
};
use crate::modules::reportes::routes::router as router_reportes;

use super::{ErrorHttpActividad, router};

const RUTA: &str = "/api/v1/actividad/ws";
const ESPERA_EVENTO: Duration = Duration::from_secs(3);
const ESPERA_VACIA: Duration = Duration::from_millis(700);
const SINCRONIZACION: Duration = Duration::from_millis(300);

type ClienteWs = WebSocketSincrono<std::net::TcpStream>;

#[derive(Clone)]
struct EstadoPrueba {
    canal: CanalActividad,
    intervalo_ping: Duration,
    inactividad: Duration,
}

fn router_con_tiempos(
    canal: CanalActividad,
    intervalo_ping: Duration,
    inactividad: Duration,
) -> Router {
    Router::new()
        .route(RUTA, get(conectar_con_tiempos))
        .with_state(EstadoPrueba {
            canal,
            intervalo_ping,
            inactividad,
        })
}

async fn conectar_con_tiempos(
    State(estado): State<EstadoPrueba>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Result<Response, ErrorHttpActividad> {
    let upgrade = upgrade.map_err(ErrorHttpActividad::desde_upgrade)?;
    let reserva = estado
        .canal
        .reservar()
        .ok_or(ErrorHttpActividad::LimiteConexiones)?;
    Ok(upgrade.on_upgrade(move |socket| {
        atender_conexion_con_keepalive(
            socket,
            estado.canal,
            reserva,
            estado.intervalo_ping,
            estado.inactividad,
        )
    }))
}

async fn pool_de_prueba() -> sqlx::PgPool {
    dotenvy::dotenv().ok();
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL no definida");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect(&url)
        .await
        .expect("no se pudo conectar a postgres");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("no se pudieron aplicar las migraciones");
    pool
}

fn dispositivo_unico() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("el reloj debe estar antes de 1970")
        .as_nanos();
    format!("test-act-{nanos}")
}

async fn levantar(app: Router) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("no se pudo enlazar el puerto de prueba");
    let puerto = listener
        .local_addr()
        .expect("dirección local no disponible")
        .port();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("servidor de prueba");
    });
    puerto
}

async fn responder(app: &Router, peticion: Request<Body>) -> (StatusCode, String) {
    let response = app
        .clone()
        .oneshot(peticion)
        .await
        .expect("el router no respondió");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("no se pudo leer el body")
        .to_bytes();
    (
        status,
        String::from_utf8(bytes.to_vec()).expect("body no es utf8"),
    )
}

async fn post_reporte(app: &Router, body: &str) -> (StatusCode, String) {
    responder(
        app,
        Request::builder()
            .method(Method::POST)
            .uri("/api/v1/reportes")
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .expect("request inválido"),
    )
    .await
}

fn peticion_get_sin_upgrade() -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(RUTA)
        .body(Body::empty())
        .expect("request inválido")
}

fn peticion_get_con_upgrade() -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(RUTA)
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
        .header("Sec-WebSocket-Version", "13")
        .body(Body::empty())
        .expect("request inválido")
}

async fn get_upgrade_en_servidor(puerto: u16) -> String {
    tokio::task::spawn_blocking(move || {
        let mut stream = TcpStream::connect(("127.0.0.1", puerto))
            .expect("no se pudo conectar al servidor");
        let peticion = format!(
            "GET {RUTA} HTTP/1.1\r\nHost: 127.0.0.1:{puerto}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream
            .write_all(peticion.as_bytes())
            .expect("no se pudo escribir la petición");
        stream
            .set_read_timeout(Some(Duration::from_millis(700)))
            .expect("no se pudo fijar el timeout de lectura");
        let mut respuesta = Vec::new();
        if let Err(error) = stream.read_to_end(&mut respuesta) {
            assert!(
                matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut),
                "lectura inesperada: {error}"
            );
        }
        String::from_utf8_lossy(&respuesta).into_owned()
    })
    .await
    .expect("petición websocket bloqueante")
}

async fn conectar_ws(puerto: u16) -> ClienteWs {
    let url = format!("ws://127.0.0.1:{puerto}{RUTA}");
    let (stream, _) = connect_async(url.as_str())
        .await
        .unwrap_or_else(|error| panic!("se esperaba un 101 y ocurrió: {error}"));
    let tcp = match stream.into_inner() {
        MaybeTlsStream::Plain(tcp) => tcp,
        _ => panic!("los tests solo admiten conexión sin TLS"),
    };
    let tcp = tcp.into_std().expect("socket no convertible a std");
    tcp.set_nonblocking(false)
        .expect("el socket debe ser bloqueante");
    WebSocketSincrono::from_raw_socket(tcp, Role::Client, None)
}

async fn ws_leer(
    mut cliente: ClienteWs,
    espera: Duration,
) -> (ClienteWs, Result<Message, ErrorWs>) {
    tokio::task::spawn_blocking(move || {
        let _ = cliente.get_ref().set_read_timeout(Some(espera));
        let mensaje = cliente.read();
        (cliente, mensaje)
    })
    .await
    .expect("lectura websocket bloqueante")
}

async fn ws_enviar_texto(mut cliente: ClienteWs, texto: &str) -> ClienteWs {
    let texto = texto.to_owned();
    tokio::task::spawn_blocking(move || {
        cliente
            .send(Message::Text(texto.into()))
            .expect("no se pudo enviar el mensaje");
        cliente
    })
    .await
    .expect("envío websocket bloqueante")
}

async fn ws_cerrar(mut cliente: ClienteWs) {
    tokio::task::spawn_blocking(move || {
        cliente.close(None).expect("no se pudo enviar el cierre");
    })
    .await
    .expect("cierre websocket bloqueante");
}

fn es_espera_vacia(error: &ErrorWs) -> bool {
    matches!(
        error,
        ErrorWs::Io(io) if matches!(io.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
    )
}

async fn esperar_evento(cliente: ClienteWs) -> (ClienteWs, Value) {
    let mut cliente = cliente;
    loop {
        let (conexiones, resultado) = ws_leer(cliente, ESPERA_EVENTO).await;
        cliente = conexiones;
        match resultado {
            Ok(Message::Text(texto)) => {
                let evento = serde_json::from_str(&texto).expect("el evento debe ser JSON");
                return (cliente, evento);
            }
            Ok(_) => continue,
            Err(error) => panic!("no se recibió el evento: {error}"),
        }
    }
}

async fn exigir_silencio(cliente: ClienteWs, espera: Duration) -> ClienteWs {
    let (cliente, resultado) = ws_leer(cliente, espera).await;
    match resultado {
        Ok(mensaje) => panic!("no debía llegar ningún mensaje, llegó {mensaje:?}"),
        Err(error) if es_espera_vacia(&error) => cliente,
        Err(error) => panic!("la conexión falló antes de tiempo: {error}"),
    }
}

async fn exigir_cierre(cliente: ClienteWs, presupuesto: Duration) {
    let mut cliente = cliente;
    let limite = Instant::now() + presupuesto;
    loop {
        let restante = limite.saturating_duration_since(Instant::now());
        assert!(!restante.is_zero(), "la conexión debía haberse cerrado");
        let (conexiones, resultado) =
            ws_leer(cliente, restante.min(Duration::from_millis(400))).await;
        cliente = conexiones;
        match resultado {
            Ok(Message::Close(_)) => return,
            Ok(_) => continue,
            Err(error) if es_espera_vacia(&error) => continue,
            Err(_) => return,
        }
    }
}

fn app_reportes_y_actividad(pool: sqlx::PgPool, canal: CanalActividad) -> Router {
    Router::new()
        .merge(router_reportes(pool, canal.clone()))
        .merge(router(canal))
}

#[tokio::test]
async fn handshake_exitoso_responde_101() {
    let canal = CanalActividad::nuevo(10);
    let puerto = levantar(router(canal)).await;

    let cliente = conectar_ws(puerto).await;

    drop(cliente);
}

#[tokio::test]
async fn get_sin_headers_de_upgrade_responde_400() {
    let canal = CanalActividad::nuevo(10);
    let app = router(canal);

    let (status, body) = responder(&app, peticion_get_sin_upgrade()).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    let error: Value = serde_json::from_str(&body).expect("la respuesta debe ser JSON");
    assert_eq!(
        error.get("error").and_then(Value::as_str),
        Some("cabecera de upgrade websocket ausente o inválida")
    );
}

#[tokio::test]
async fn get_con_headers_pero_sin_estado_de_upgrade_responde_500() {
    let canal = CanalActividad::nuevo(10);
    let app = router(canal);

    let (status, body) = responder(&app, peticion_get_con_upgrade()).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let error: Value = serde_json::from_str(&body).expect("la respuesta debe ser JSON");
    assert_eq!(
        error.get("error").and_then(Value::as_str),
        Some("error interno del servidor")
    );
}

#[tokio::test]
async fn el_limite_de_conexiones_responde_429_y_libera_al_cerrar() {
    let canal = CanalActividad::nuevo(2);
    let puerto = levantar(router(canal)).await;

    let primera = conectar_ws(puerto).await;
    let segunda = conectar_ws(puerto).await;

    let respuesta = get_upgrade_en_servidor(puerto).await;
    assert!(
        respuesta.starts_with("HTTP/1.1 429"),
        "respuesta: {respuesta}"
    );
    assert!(
        respuesta.contains("limite de conexiones de actividad alcanzado"),
        "respuesta: {respuesta}"
    );

    ws_cerrar(primera).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let tercera = conectar_ws(puerto).await;

    drop(segunda);
    drop(tercera);
}

#[tokio::test]
async fn un_post_valido_emite_el_evento_al_suscriptor() {
    let pool = pool_de_prueba().await;
    let canal = CanalActividad::nuevo(10);
    let app = app_reportes_y_actividad(pool, canal);
    let app_post = app.clone();
    let puerto = levantar(app).await;

    let cliente = exigir_silencio(conectar_ws(puerto).await, SINCRONIZACION).await;

    let cuerpo = json!({
        "id_usuario": dispositivo_unico(),
        "lat": -33.4489,
        "lng": -70.6693,
        "horas_duracion": 2
    })
    .to_string();
    let (status, cuerpo_post) = post_reporte(&app_post, &cuerpo).await;
    assert_eq!(status, StatusCode::CREATED);

    let (cliente, evento) = esperar_evento(cliente).await;
    let respuesta: Value =
        serde_json::from_str(&cuerpo_post).expect("la respuesta del POST debe ser JSON");

    assert_eq!(
        evento.get("tipo").and_then(Value::as_str),
        Some(EVENTO_REPORTE_CREADO)
    );
    assert_eq!(evento.get("id"), respuesta.get("id"));
    assert_eq!(evento.get("lat").and_then(Value::as_f64), Some(-33.4489));
    assert_eq!(evento.get("lng").and_then(Value::as_f64), Some(-70.6693));
    assert_eq!(
        evento.get("horas_duracion").and_then(Value::as_u64),
        Some(2)
    );
    assert!(
        evento
            .get("creado")
            .and_then(Value::as_str)
            .is_some_and(|creado| DateTime::parse_from_rfc3339(creado).is_ok()),
        "creado debe ser RFC 3339: {evento}"
    );
    assert!(
        evento.get("id_usuario").is_none(),
        "el evento jamás debe identificar al emisor: {evento}"
    );

    drop(cliente);
}

#[tokio::test]
async fn dos_suscriptores_reciben_el_mismo_evento() {
    let pool = pool_de_prueba().await;
    let canal = CanalActividad::nuevo(10);
    let app = app_reportes_y_actividad(pool, canal);
    let app_post = app.clone();
    let puerto = levantar(app).await;

    let primera = exigir_silencio(conectar_ws(puerto).await, SINCRONIZACION).await;
    let segunda = exigir_silencio(conectar_ws(puerto).await, SINCRONIZACION).await;

    let cuerpo = json!({
        "id_usuario": dispositivo_unico(),
        "lat": -33.11,
        "lng": -70.22
    })
    .to_string();
    let (status, _) = post_reporte(&app_post, &cuerpo).await;
    assert_eq!(status, StatusCode::CREATED);

    let (primera, evento_primero) = esperar_evento(primera).await;
    let (segunda, evento_segundo) = esperar_evento(segunda).await;

    assert_eq!(evento_primero, evento_segundo);

    drop(primera);
    drop(segunda);
}

#[tokio::test]
async fn el_post_rechazado_no_emite_evento() {
    let pool = pool_de_prueba().await;
    let canal = CanalActividad::nuevo(10);
    let app = app_reportes_y_actividad(pool, canal);
    let app_post = app.clone();
    let puerto = levantar(app).await;

    let cliente = exigir_silencio(conectar_ws(puerto).await, SINCRONIZACION).await;

    let cuerpo = json!({
        "id_usuario": dispositivo_unico(),
        "lat": -33.55,
        "lng": -70.77
    })
    .to_string();
    let (status, _) = post_reporte(&app_post, &cuerpo).await;
    assert_eq!(status, StatusCode::CREATED);

    let (cliente, evento) = esperar_evento(cliente).await;
    assert_eq!(
        evento.get("tipo").and_then(Value::as_str),
        Some(EVENTO_REPORTE_CREADO)
    );

    let (status, _) = post_reporte(&app_post, &cuerpo).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    let cliente = exigir_silencio(cliente, ESPERA_VACIA).await;

    let invalido = json!({
        "id_usuario": dispositivo_unico(),
        "lat": 91.0,
        "lng": 0.0
    })
    .to_string();
    let (status, _) = post_reporte(&app_post, &invalido).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    drop(exigir_silencio(cliente, ESPERA_VACIA).await);
}

#[tokio::test]
async fn el_mensaje_de_texto_del_cliente_no_cierra_ni_se_retransmite() {
    let pool = pool_de_prueba().await;
    let canal = CanalActividad::nuevo(10);
    let app = app_reportes_y_actividad(pool, canal);
    let app_post = app.clone();
    let puerto = levantar(app).await;

    let emisor = exigir_silencio(conectar_ws(puerto).await, SINCRONIZACION).await;
    let observador = exigir_silencio(conectar_ws(puerto).await, SINCRONIZACION).await;

    let emisor = ws_enviar_texto(emisor, "hola").await;
    let observador = exigir_silencio(observador, ESPERA_VACIA).await;

    let cuerpo = json!({
        "id_usuario": dispositivo_unico(),
        "lat": -33.66,
        "lng": -70.88
    })
    .to_string();
    let (status, _) = post_reporte(&app_post, &cuerpo).await;
    assert_eq!(status, StatusCode::CREATED);

    let (emisor, evento) = esperar_evento(emisor).await;
    assert_eq!(
        evento.get("tipo").and_then(Value::as_str),
        Some(EVENTO_REPORTE_CREADO)
    );

    drop(emisor);
    drop(observador);
}

#[tokio::test]
async fn tras_cerrar_la_conexion_el_slot_se_libera() {
    let canal = CanalActividad::nuevo(1);
    let puerto = levantar(router(canal)).await;

    let primera = conectar_ws(puerto).await;
    ws_cerrar(primera).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let segunda = conectar_ws(puerto).await;

    drop(segunda);
}

#[tokio::test]
async fn la_desconexion_abrupta_libera_el_slot() {
    let canal = CanalActividad::nuevo(1);
    let puerto = levantar(router(canal)).await;

    let primera = conectar_ws(puerto).await;
    drop(primera);
    tokio::time::sleep(Duration::from_millis(200)).await;

    let segunda = conectar_ws(puerto).await;

    drop(segunda);
}

#[tokio::test]
async fn el_servidor_cierra_la_conexion_por_inactividad() {
    let canal = CanalActividad::nuevo(1);
    let app = router_con_tiempos(
        canal,
        Duration::from_millis(100),
        Duration::from_millis(400),
    );
    let puerto = levantar(app).await;

    let cliente = conectar_ws(puerto).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;

    let segundo = conectar_ws(puerto).await;
    drop(segundo);

    exigir_cierre(cliente, Duration::from_secs(2)).await;
}
