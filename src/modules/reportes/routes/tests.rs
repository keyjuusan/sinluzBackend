use std::{
    sync::{Arc, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use chrono::{DateTime, Duration, FixedOffset, NaiveDate, SecondsFormat, TimeZone, Utc};
use http_body_util::BodyExt;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

use crate::modules::actividad::{MAX_CONEXIONES_ACTIVIDAD, infra::CanalActividad};
use crate::modules::reportes::domain::GeocercaVenezuela;

use super::router;

fn geocerca_de_prueba() -> Arc<GeocercaVenezuela> {
    static GEOCERCA: OnceLock<Arc<GeocercaVenezuela>> = OnceLock::new();
    GEOCERCA
        .get_or_init(|| {
            Arc::new(
                GeocercaVenezuela::desde_datos_embebidos()
                    .expect("el dataset embebido de fronteras debe cargar"),
            )
        })
        .clone()
}

async fn pool_de_prueba() -> sqlx::PgPool {
    dotenvy::dotenv().ok();
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL no definida");
    let pool = PgPoolOptions::new()
        .connect(&url)
        .await
        .expect("no se pudo conectar a postgres");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("no se pudieron aplicar las migraciones");
    pool
}

async fn aplicacion(horario_zona: FixedOffset) -> (sqlx::PgPool, Router) {
    let pool = pool_de_prueba().await;
    let canal = CanalActividad::nuevo(MAX_CONEXIONES_ACTIVIDAD);
    (
        pool.clone(),
        router(pool, canal, horario_zona, geocerca_de_prueba()),
    )
}

fn zona(segundos: i32) -> FixedOffset {
    FixedOffset::east_opt(segundos).expect("offset de prueba válido")
}

fn caracas() -> FixedOffset {
    zona(-4 * 3600)
}

fn utc() -> FixedOffset {
    zona(0)
}

fn dispositivo_unico() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("el reloj debe estar antes de 1970")
        .as_nanos();
    format!("test-dev-{nanos}")
}

async fn post_reporte(app: &Router, body: &str) -> (StatusCode, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/reportes")
                .header("content-type", "application/json")
                .body(Body::from(body.to_owned()))
                .expect("request inválido"),
        )
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

fn coordenada_unica() -> f64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("el reloj debe estar antes de 1970")
        .as_nanos();
    (nanos % 100_000) as f64 / 100_000.0
}

async fn get_reportes(app: &Router, query: &str) -> (StatusCode, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/api/v1/reportes{query}"))
                .body(Body::empty())
                .expect("request inválido"),
        )
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

async fn insertar_reporte(
    pool: &sqlx::PgPool,
    lat: f64,
    lng: f64,
    creado: DateTime<Utc>,
) -> String {
    let id: String = sqlx::query_scalar(
        "INSERT INTO reportes (id_usuario, lat, lng, creado, horas_duracion)
         VALUES ($1, $2, $3, $4, NULL)
         RETURNING id::text",
    )
    .bind(dispositivo_unico())
    .bind(lat)
    .bind(lng)
    .bind(creado)
    .fetch_one(pool)
    .await
    .expect("fallo insertar reporte");
    id
}

fn ids_de(reportes: &serde_json::Value) -> Vec<String> {
    reportes["reportes"]
        .as_array()
        .expect("falta reportes")
        .iter()
        .map(|reporte| reporte["id"].as_str().expect("falta id").to_owned())
        .collect()
}

async fn limpiar_banda(pool: &sqlx::PgPool, banda: f64) {
    sqlx::query("DELETE FROM reportes WHERE lat >= $1 AND lat < $1 + 1")
        .bind(banda)
        .execute(pool)
        .await
        .expect("fallo limpieza de banda");
}

#[tokio::test]
async fn registra_reporte_valido() {
    let (pool, app) = aplicacion(caracas()).await;
    let dispositivo = dispositivo_unico();

    let (status, body) = post_reporte(
        &app,
        &json!({
            "id_usuario": dispositivo,
            "lat": 10.4806,
            "lng": -66.9036,
            "horas_duracion": 2
        })
        .to_string(),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert!(respuesta["id"].as_str().is_some_and(|id| !id.is_empty()));
    let creado = respuesta["creado"].as_str().expect("falta creado");
    assert!(
        chrono::DateTime::parse_from_rfc3339(creado).is_ok(),
        "creado no es RFC 3339: {creado}"
    );

    let filas: (i64,) = sqlx::query_as("SELECT count(*) FROM reportes WHERE id_usuario = $1")
        .bind(&dispositivo)
        .fetch_one(&pool)
        .await
        .expect("fallo la consulta de verificación");
    assert_eq!(filas.0, 1, "la fila quedó persistida una sola vez");

    let horas: (Option<i16>,) =
        sqlx::query_as("SELECT horas_duracion FROM reportes WHERE id_usuario = $1")
            .bind(&dispositivo)
            .fetch_one(&pool)
            .await
            .expect("fallo la consulta de horas");
    assert_eq!(horas.0, Some(2));
}

#[tokio::test]
async fn acepta_duracion_nula_y_omitida() {
    let (pool, app) = aplicacion(caracas()).await;
    let con_null = dispositivo_unico();
    let omitida = dispositivo_unico();

    let (status, _) = post_reporte(
        &app,
        &json!({
            "id_usuario": con_null,
            "lat": 10.4806,
            "lng": -66.9036,
            "horas_duracion": null
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = post_reporte(
        &app,
        &format!(r#"{{"id_usuario":"{omitida}","lat":10.4810,"lng":-66.9040}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    for dispositivo in [con_null, omitida] {
        let es_nulo: (bool,) =
            sqlx::query_as("SELECT horas_duracion IS NULL FROM reportes WHERE id_usuario = $1")
                .bind(&dispositivo)
                .fetch_one(&pool)
                .await
                .expect("fallo la consulta de verificación");
        assert!(es_nulo.0, "se debió persistir NULL para {dispositivo}");
    }
}

#[tokio::test]
async fn rechaza_coordenadas_fuera_de_rango() {
    let (_, app) = aplicacion(caracas()).await;

    let (status, body) = post_reporte(
        &app,
        &json!({
            "id_usuario": dispositivo_unico(),
            "lat": 90.1,
            "lng": 12.0
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("lat fuera de rango"), "body: {body}");

    let (status, body) = post_reporte(
        &app,
        &json!({
            "id_usuario": dispositivo_unico(),
            "lat": 12.0,
            "lng": -180.1
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("lng fuera de rango"), "body: {body}");
}

#[tokio::test]
async fn rechaza_device_invalido() {
    let (_, app) = aplicacion(caracas()).await;

    let (status, body) = post_reporte(
        &app,
        &json!({
            "id_usuario": "abc1234",
            "lat": 12.0,
            "lng": 45.0
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("id_usuario"), "body: {body}");

    let (status, body) = post_reporte(
        &app,
        &json!({
            "id_usuario": "device-invalido!",
            "lat": 12.0,
            "lng": 45.0
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("id_usuario"), "body: {body}");
}

#[tokio::test]
async fn rechaza_duracion_fuera_de_rango() {
    let (_, app) = aplicacion(caracas()).await;

    let (status, body) = post_reporte(
        &app,
        &json!({
            "id_usuario": dispositivo_unico(),
            "lat": 10.4806,
            "lng": -66.9036,
            "horas_duracion": 169
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("horas_duracion"), "body: {body}");

    let (status, _) = post_reporte(
        &app,
        &json!({
            "id_usuario": dispositivo_unico(),
            "lat": 10.6545,
            "lng": -71.6405,
            "horas_duracion": 168
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn rechaza_campo_desconocido() {
    let (_, app) = aplicacion(caracas()).await;
    let (status, _) = post_reporte(
        &app,
        &json!({
            "id_usuario": dispositivo_unico(),
            "lat": 12.0,
            "lng": 45.0,
            "foo": 1
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn rechaza_json_malformado_o_vacio() {
    let (_, app) = aplicacion(caracas()).await;

    let (status, body) = post_reporte(&app, "esto-no-es-json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.contains("cuerpo de solicitud inválido"),
        "body: {body}"
    );

    let (status, _) = post_reporte(&app, "").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn aplica_cooldown_misma_ubicacion() {
    let (_, app) = aplicacion(caracas()).await;
    let dispositivo = dispositivo_unico();
    let cuerpo = json!({
        "id_usuario": dispositivo,
        "lat": 10.4806,
        "lng": -66.9036
    })
    .to_string();

    let (status, _) = post_reporte(&app, &cuerpo).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = post_reporte(&app, &cuerpo).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(body.contains("cooldown"), "body: {body}");
}

#[tokio::test]
async fn permite_otra_ubicacion_mismo_dispositivo() {
    let (_, app) = aplicacion(caracas()).await;
    let dispositivo = dispositivo_unico();

    for (lat, lng) in [(10.4806, -66.9036), (10.6545, -71.6405)] {
        let (status, _) = post_reporte(
            &app,
            &json!({ "id_usuario": dispositivo, "lat": lat, "lng": lng }).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
}

#[tokio::test]
async fn permite_otro_dispositivo_misma_ubicacion() {
    let (_, app) = aplicacion(caracas()).await;

    for _ in 0..2 {
        let (status, _) = post_reporte(
            &app,
            &json!({
                "id_usuario": dispositivo_unico(),
                "lat": 10.4806,
                "lng": -66.9036
            })
            .to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
}

#[tokio::test]
async fn acepta_maracaibo_y_porlamar() {
    let (_, app) = aplicacion(caracas()).await;

    for (lat, lng) in [(10.6545, -71.6405), (10.9577, -63.8697)] {
        let (status, body) = post_reporte(
            &app,
            &json!({ "id_usuario": dispositivo_unico(), "lat": lat, "lng": lng }).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "({lat}, {lng}): {body}");
    }
}

#[tokio::test]
async fn rechaza_ubicacion_fuera_de_venezuela_sin_persistir() {
    let (pool, app) = aplicacion(caracas()).await;

    for (lat, lng) in [(-33.4489, -70.6693), (4.7110, -74.0721)] {
        let dispositivo = dispositivo_unico();
        let (status, body) = post_reporte(
            &app,
            &json!({ "id_usuario": dispositivo, "lat": lat, "lng": lng }).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "({lat}, {lng}): {body}");
        assert!(body.contains("fuera de Venezuela"), "body: {body}");

        let filas: (i64,) = sqlx::query_as("SELECT count(*) FROM reportes WHERE id_usuario = $1")
            .bind(&dispositivo)
            .fetch_one(&pool)
            .await
            .expect("fallo la consulta de verificación");
        assert_eq!(filas.0, 0, "({lat}, {lng}) no debe persistirse");
    }
}

#[tokio::test]
async fn el_rango_global_precede_a_la_geocerca() {
    let (_, app) = aplicacion(caracas()).await;

    let (status, body) = post_reporte(
        &app,
        &json!({
            "id_usuario": dispositivo_unico(),
            "lat": 90.0,
            "lng": 0.0
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("fuera de Venezuela"), "body: {body}");

    let (status, body) = post_reporte(
        &app,
        &json!({
            "id_usuario": dispositivo_unico(),
            "lat": 90.1,
            "lng": 0.0
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("lat fuera de rango"), "body: {body}");
}

#[tokio::test]
async fn id_usuario_invalido_precede_a_la_geocerca() {
    let (_, app) = aplicacion(caracas()).await;

    let (status, body) = post_reporte(
        &app,
        &json!({
            "id_usuario": "abc1234",
            "lat": -33.4489,
            "lng": -70.6693
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("id_usuario"), "body: {body}");
    assert!(!body.contains("Venezuela"), "body: {body}");
}

#[tokio::test]
async fn horas_duracion_precede_a_la_geocerca() {
    let (_, app) = aplicacion(caracas()).await;

    let (status, body) = post_reporte(
        &app,
        &json!({
            "id_usuario": dispositivo_unico(),
            "lat": -33.4489,
            "lng": -70.6693,
            "horas_duracion": 169
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("horas_duracion"), "body: {body}");
    assert!(!body.contains("Venezuela"), "body: {body}");
}

#[tokio::test]
async fn el_feed_no_aplica_la_geocerca() {
    let (pool, app) = aplicacion(caracas()).await;
    limpiar_banda(&pool, -34.0).await;
    let corte = coordenada_unica() * 0.001;
    let (base_lat, base_lng) = (-33.4489 - corte, -70.6693 - corte);
    let id = insertar_reporte(&pool, base_lat, base_lng, Utc::now()).await;

    let query = format!(
        "?min_lat={}&max_lat={}&min_lng={}&max_lng={}",
        base_lat - 0.0005,
        base_lat + 0.0005,
        base_lng - 0.0005,
        base_lng + 0.0005
    );
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(ids_de(&respuesta), vec![id], "body: {body}");
}

#[tokio::test]
async fn lista_reportes_ordenados_por_creado_con_total() {
    let (pool, app) = aplicacion(caracas()).await;
    limpiar_banda(&pool, 10.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (10.0 + corte, 10.0 + corte);
    let ahora = Utc::now();
    let id_antiguo = insertar_reporte(&pool, base_lat, base_lng, ahora - Duration::hours(2)).await;
    let id_reciente = insertar_reporte(
        &pool,
        base_lat,
        base_lng + 0.001,
        ahora - Duration::hours(1),
    )
    .await;

    let query = format!(
        "?min_lat={}&max_lat={}&min_lng={}&max_lng={}",
        base_lat - 0.01,
        base_lat + 0.01,
        base_lng - 0.01,
        base_lng + 0.02
    );
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK);
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    let ids = ids_de(&respuesta);
    assert_eq!(ids.len(), 2, "body: {body}");
    assert_eq!(respuesta["total"].as_i64(), Some(2));
    assert_eq!(respuesta["limit"].as_u64(), Some(1000));
    assert_eq!(respuesta["offset"].as_u64(), Some(0));
    assert_eq!(ids[0], id_reciente);
    assert_eq!(ids[1], id_antiguo);
}

#[tokio::test]
async fn filtra_por_ventana_temporal() {
    let (pool, app) = aplicacion(caracas()).await;
    limpiar_banda(&pool, 20.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (20.0 + corte, 20.0 + corte);
    let ahora = Utc::now();
    let id_lejano = insertar_reporte(&pool, base_lat, base_lng, ahora - Duration::hours(4)).await;
    let id_reciente = insertar_reporte(
        &pool,
        base_lat + 0.001,
        base_lng + 0.001,
        ahora - Duration::hours(1),
    )
    .await;

    let query = format!(
        "?desde={}&hasta={}&min_lat={}&max_lat={}&min_lng={}&max_lng={}",
        (ahora - Duration::hours(2)).to_rfc3339_opts(SecondsFormat::Secs, true),
        ahora.to_rfc3339_opts(SecondsFormat::Secs, true),
        base_lat - 0.01,
        base_lat + 0.02,
        base_lng - 0.01,
        base_lng + 0.02
    );
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK);
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    let ids = ids_de(&respuesta);
    assert_eq!(ids, vec![id_reciente], "body: {body}");
    assert_eq!(respuesta["total"].as_i64(), Some(1));
    assert!(!ids.contains(&id_lejano));
}

#[tokio::test]
async fn filtra_por_bbox_geografico() {
    let (pool, app) = aplicacion(caracas()).await;
    limpiar_banda(&pool, 30.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (30.0 + corte, 30.0 + corte);
    let ahora = Utc::now();
    let id_dentro = insertar_reporte(&pool, base_lat, base_lng, ahora).await;
    let id_fuera = insertar_reporte(
        &pool,
        base_lat + 1.0,
        base_lng + 1.0,
        ahora - Duration::minutes(1),
    )
    .await;

    let query = format!(
        "?min_lat={}&max_lat={}&min_lng={}&max_lng={}",
        base_lat - 0.01,
        base_lat + 0.01,
        base_lng - 0.01,
        base_lng + 0.01
    );
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK);
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    let ids = ids_de(&respuesta);
    assert_eq!(ids, vec![id_dentro], "body: {body}");
    assert!(!ids.contains(&id_fuera));
}

#[tokio::test]
async fn pagina_con_limit_y_offset() {
    let (pool, app) = aplicacion(caracas()).await;
    limpiar_banda(&pool, 40.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (40.0 + corte, 40.0 + corte);
    let ahora = Utc::now();
    let mut ids = Vec::new();
    for horas in 1..=3 {
        ids.push(insertar_reporte(&pool, base_lat, base_lng, ahora - Duration::hours(horas)).await);
    }
    let bbox = format!(
        "min_lat={}&max_lat={}&min_lng={}&max_lng={}",
        base_lat - 0.01,
        base_lat + 0.01,
        base_lng - 0.01,
        base_lng + 0.01
    );

    let (status, body) = get_reportes(&app, &format!("?{bbox}&limit=2&offset=0")).await;
    assert_eq!(status, StatusCode::OK);
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(3));
    let primera = ids_de(&respuesta);
    assert_eq!(primera.len(), 2, "body: {body}");

    let (status, body) = get_reportes(&app, &format!("?{bbox}&limit=2&offset=2")).await;
    assert_eq!(status, StatusCode::OK);
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(3));
    let segunda = ids_de(&respuesta);
    assert_eq!(segunda, vec![ids[2].clone()], "body: {body}");
    assert!(primera.iter().all(|id| !segunda.contains(id)));
}

#[tokio::test]
async fn devuelve_lista_vacia_sin_reportes() {
    let (_, app) = aplicacion(caracas()).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (-40.0 + corte, -40.0 + corte);

    let query = format!(
        "?desde=2000-01-01T00:00:00Z&hasta=2000-01-02T00:00:00Z&min_lat={}&max_lat={}&min_lng={}&max_lng={}",
        base_lat, base_lat, base_lng, base_lng
    );
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK);
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["reportes"].as_array().map(Vec::len), Some(0));
    assert_eq!(respuesta["total"].as_i64(), Some(0));
}

#[tokio::test]
async fn excluye_reportes_fuera_de_la_ventana_por_defecto() {
    let (pool, app) = aplicacion(caracas()).await;
    limpiar_banda(&pool, 50.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (50.0 + corte, 50.0 + corte);
    let id_antiguo =
        insertar_reporte(&pool, base_lat, base_lng, Utc::now() - Duration::hours(25)).await;

    let query = format!(
        "?min_lat={}&max_lat={}&min_lng={}&max_lng={}",
        base_lat - 0.01,
        base_lat + 0.01,
        base_lng - 0.01,
        base_lng + 0.01
    );
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK);
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    let ids = ids_de(&respuesta);
    assert!(ids.is_empty(), "body: {body}");
    assert!(!ids.contains(&id_antiguo));
}

#[tokio::test]
async fn rechaza_fechas_invalidas() {
    let (_, app) = aplicacion(caracas()).await;

    let (status, body) = get_reportes(&app, "?desde=no-es-fecha").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("desde"), "body: {body}");

    let (status, body) = get_reportes(&app, "?hasta=no-es-fecha").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("hasta"), "body: {body}");
}

#[tokio::test]
async fn rechaza_ventana_invertida() {
    let (_, app) = aplicacion(caracas()).await;
    let (status, body) = get_reportes(
        &app,
        "?desde=2026-10-06T00:00:00Z&hasta=2026-10-05T00:00:00Z",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("anterior a hasta"), "body: {body}");
}

#[tokio::test]
async fn rechaza_bbox_incompleto() {
    let (_, app) = aplicacion(caracas()).await;
    let (status, body) = get_reportes(&app, "?min_lat=10").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("bbox"), "body: {body}");
}

#[tokio::test]
async fn rechaza_bbox_invertido() {
    let (_, app) = aplicacion(caracas()).await;
    let (status, body) = get_reportes(&app, "?min_lat=10&max_lat=9&min_lng=-75&max_lng=-76").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("bbox"), "body: {body}");
}

#[tokio::test]
async fn rechaza_bbox_fuera_de_rango() {
    let (_, app) = aplicacion(caracas()).await;

    let (status, body) = get_reportes(&app, "?min_lat=91&max_lat=92&min_lng=-70&max_lng=-69").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("lat del bbox"), "body: {body}");

    let (status, body) =
        get_reportes(&app, "?min_lat=10&max_lat=11&min_lng=-181&max_lng=-179").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("lng del bbox"), "body: {body}");
}

#[tokio::test]
async fn rechaza_limit_fuera_de_rango() {
    let (_, app) = aplicacion(caracas()).await;

    let (status, body) = get_reportes(&app, "?limit=0").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("limit"), "body: {body}");

    let (status, body) = get_reportes(&app, "?limit=5001").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("limit"), "body: {body}");

    let (status, _) = get_reportes(&app, "?limit=5000").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn rechaza_offset_negativo() {
    let (_, app) = aplicacion(caracas()).await;
    let (status, body) = get_reportes(&app, "?offset=-1").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("offset"), "body: {body}");
}

#[tokio::test]
async fn rechaza_parametro_desconocido() {
    let (_, app) = aplicacion(caracas()).await;
    let (status, _) = get_reportes(&app, "?foo=1").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

fn instante_query(instante: DateTime<Utc>) -> String {
    instante.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn bbox_query(base_lat: f64, base_lng: f64) -> String {
    format!(
        "min_lat={}&max_lat={}&min_lng={}&max_lng={}",
        base_lat - 0.01,
        base_lat + 0.01,
        base_lng - 0.01,
        base_lng + 0.01
    )
}

fn medianoche(dia: &str) -> DateTime<Utc> {
    NaiveDate::parse_from_str(dia, "%Y-%m-%d")
        .expect("día inválido")
        .and_hms_opt(0, 0, 0)
        .expect("medianoche válida")
        .and_utc()
}

#[tokio::test]
async fn hora_filtra_la_franja_entre_esa_hora_y_la_siguiente() {
    let (pool, app) = aplicacion(utc()).await;
    limpiar_banda(&pool, 60.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (60.0 + corte, 60.0 + corte);
    let base = medianoche("2026-03-15") + Duration::hours(13);

    let id_antes = insertar_reporte(
        &pool,
        base_lat,
        base_lng,
        base + Duration::hours(1) - Duration::seconds(1),
    )
    .await;
    let id_inicio = insertar_reporte(&pool, base_lat, base_lng, base + Duration::hours(1)).await;
    let id_ultimo = insertar_reporte(
        &pool,
        base_lat,
        base_lng,
        base + Duration::hours(2) - Duration::seconds(1),
    )
    .await;
    let id_fin = insertar_reporte(&pool, base_lat, base_lng, base + Duration::hours(2)).await;

    let query = format!(
        "?desde={}&hasta={}&{}&hora=14",
        instante_query(base),
        instante_query(base + Duration::hours(3)),
        bbox_query(base_lat, base_lng)
    );
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(2), "body: {body}");
    let ids = ids_de(&respuesta);
    assert_eq!(ids, vec![id_ultimo, id_inicio], "body: {body}");
    assert!(
        !ids.contains(&id_antes) && !ids.contains(&id_fin),
        "13:59:59 y 15:00:00 deben quedar fuera: {body}"
    );
    assert!(respuesta.get("intervalos").is_none(), "body: {body}");
    assert!(respuesta.get("intervalo_horas").is_none(), "body: {body}");
}

#[tokio::test]
async fn hora_se_repite_en_cada_dia_de_la_ventana() {
    let (pool, app) = aplicacion(utc()).await;
    limpiar_banda(&pool, 70.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (70.0 + corte, 70.0 + corte);

    let id_dia1 = insertar_reporte(
        &pool,
        base_lat,
        base_lng,
        medianoche("2026-05-01") + Duration::hours(14) + Duration::minutes(30),
    )
    .await;
    let id_dia2 = insertar_reporte(
        &pool,
        base_lat,
        base_lng,
        medianoche("2026-05-02") + Duration::hours(14) + Duration::minutes(30),
    )
    .await;
    let id_hora_fuera = insertar_reporte(
        &pool,
        base_lat,
        base_lng,
        medianoche("2026-05-02") + Duration::hours(15) + Duration::minutes(30),
    )
    .await;
    let id_anterior = insertar_reporte(
        &pool,
        base_lat,
        base_lng,
        medianoche("2026-05-02") + Duration::hours(13) + Duration::minutes(30),
    )
    .await;
    let id_fuera_ventana = insertar_reporte(
        &pool,
        base_lat,
        base_lng,
        medianoche("2026-05-03") + Duration::hours(14) + Duration::minutes(30),
    )
    .await;

    let query = format!(
        "?desde={}&hasta={}&{}&hora=14",
        instante_query(medianoche("2026-05-01") + Duration::hours(10)),
        instante_query(medianoche("2026-05-03") + Duration::hours(14)),
        bbox_query(base_lat, base_lng)
    );
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(2), "body: {body}");
    let ids = ids_de(&respuesta);
    assert_eq!(ids, vec![id_dia2, id_dia1], "body: {body}");
    assert!(
        !ids.contains(&id_hora_fuera) && !ids.contains(&id_anterior),
        "solo debe entrar la franja 14:00–15:00: {body}"
    );
    assert!(!ids.contains(&id_fuera_ventana), "body: {body}");
}

#[tokio::test]
async fn hora_se_recorta_con_la_ventana_multidia_parcial() {
    let (pool, app) = aplicacion(utc()).await;
    limpiar_banda(&pool, 80.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (80.0 + corte, 80.0 + corte);
    let desde = medianoche("2026-06-10") + Duration::hours(14) + Duration::minutes(30);
    let hasta = medianoche("2026-06-11") + Duration::hours(14) + Duration::minutes(30);

    let id_dia1 = insertar_reporte(&pool, base_lat, base_lng, desde + Duration::minutes(15)).await;
    let id_dia2 = insertar_reporte(&pool, base_lat, base_lng, hasta - Duration::minutes(15)).await;
    let id_antes_ventana =
        insertar_reporte(&pool, base_lat, base_lng, desde - Duration::minutes(15)).await;
    let id_despues_ventana =
        insertar_reporte(&pool, base_lat, base_lng, hasta + Duration::minutes(15)).await;

    let query = format!(
        "?desde={}&hasta={}&{}&hora=14",
        instante_query(desde),
        instante_query(hasta),
        bbox_query(base_lat, base_lng)
    );
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(2), "body: {body}");
    let ids = ids_de(&respuesta);
    assert_eq!(ids, vec![id_dia2, id_dia1], "body: {body}");
    assert!(
        !ids.contains(&id_antes_ventana) && !ids.contains(&id_despues_ventana),
        "la franja debe recortarse a la ventana: {body}"
    );
}

#[tokio::test]
async fn hora_se_combina_con_bbox() {
    let (pool, app) = aplicacion(utc()).await;
    limpiar_banda(&pool, -55.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (-55.0 + corte, -55.0 + corte);
    let desde = medianoche("2026-08-01");
    let hasta = desde + Duration::days(1);

    let id_dentro = insertar_reporte(
        &pool,
        base_lat,
        base_lng,
        desde + Duration::hours(12) + Duration::minutes(30),
    )
    .await;
    let id_fuera_bbox = insertar_reporte(
        &pool,
        base_lat + 1.0,
        base_lng + 1.0,
        desde + Duration::hours(12) + Duration::minutes(30),
    )
    .await;
    let id_hora_fuera = insertar_reporte(
        &pool,
        base_lat,
        base_lng,
        desde + Duration::hours(11) + Duration::minutes(30),
    )
    .await;

    let query = format!(
        "?desde={}&hasta={}&{}&hora=12",
        instante_query(desde),
        instante_query(hasta),
        bbox_query(base_lat, base_lng)
    );
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(1), "body: {body}");
    let ids = ids_de(&respuesta);
    assert_eq!(ids, vec![id_dentro], "body: {body}");
    assert!(
        !ids.contains(&id_fuera_bbox) && !ids.contains(&id_hora_fuera),
        "body: {body}"
    );
}

#[tokio::test]
async fn hora_pagina_con_limit_y_offset() {
    let (pool, app) = aplicacion(utc()).await;
    limpiar_banda(&pool, -60.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (-60.0 + corte, -60.0 + corte);
    let desde = medianoche("2026-09-01") + Duration::hours(13);
    let hasta = desde + Duration::hours(3);

    let mut ids_franja = Vec::new();
    for minutos in [10, 20, 30] {
        ids_franja.push(
            insertar_reporte(
                &pool,
                base_lat,
                base_lng,
                desde + Duration::minutes(70 + minutos),
            )
            .await,
        );
    }
    insertar_reporte(&pool, base_lat, base_lng, desde + Duration::minutes(10)).await;
    insertar_reporte(&pool, base_lat, base_lng, desde + Duration::minutes(130)).await;
    ids_franja.reverse();

    let base_query = format!(
        "?desde={}&hasta={}&{}&hora=14",
        instante_query(desde),
        instante_query(hasta),
        bbox_query(base_lat, base_lng)
    );

    let (status, body) = get_reportes(&app, &format!("{base_query}&limit=2&offset=0")).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(3), "body: {body}");
    let primera = ids_de(&respuesta);
    assert_eq!(primera, ids_franja[..2].to_vec(), "body: {body}");

    let (status, body) = get_reportes(&app, &format!("{base_query}&limit=2&offset=2")).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(3), "body: {body}");
    let segunda = ids_de(&respuesta);
    assert_eq!(segunda, vec![ids_franja[2].clone()], "body: {body}");
    assert!(primera.iter().all(|id| !segunda.contains(id)));
}

#[tokio::test]
async fn franja_sin_reportes_devuelve_lista_vacia() {
    let (pool, app) = aplicacion(utc()).await;
    limpiar_banda(&pool, -50.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (-50.0 + corte, -50.0 + corte);

    insertar_reporte(
        &pool,
        base_lat,
        base_lng,
        medianoche("2026-09-15") + Duration::hours(10) + Duration::minutes(30),
    )
    .await;

    let query = format!(
        "?desde=2026-09-15T14:00:00Z&hasta=2026-09-15T15:00:00Z&{}&hora=14",
        bbox_query(base_lat, base_lng)
    );
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["reportes"].as_array().map(Vec::len), Some(0));
    assert_eq!(respuesta["total"].as_i64(), Some(0), "body: {body}");
    assert!(respuesta.get("intervalos").is_none(), "body: {body}");
}

#[tokio::test]
async fn sin_hora_la_respuesta_solo_tiene_las_claves_de_la_spec_02() {
    let (_, app) = aplicacion(caracas()).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (-45.0 + corte, -45.0 + corte);

    let query = format!("?{}", bbox_query(base_lat, base_lng));
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    let claves: Vec<&str> = respuesta
        .as_object()
        .expect("body no es objeto")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        claves,
        ["limit", "offset", "reportes", "total"],
        "body: {body}"
    );
}

#[tokio::test]
async fn hora_cero_es_valido_y_cubre_la_medianoche() {
    let (pool, app) = aplicacion(utc()).await;
    limpiar_banda(&pool, -65.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (-65.0 + corte, -65.0 + corte);
    let desde = medianoche("2026-04-09");
    let hasta = desde + Duration::days(2);

    let id_dentro = insertar_reporte(
        &pool,
        base_lat,
        base_lng,
        medianoche("2026-04-10") + Duration::minutes(30),
    )
    .await;
    let id_una_hora = insertar_reporte(
        &pool,
        base_lat,
        base_lng,
        medianoche("2026-04-10") + Duration::hours(1) + Duration::minutes(30),
    )
    .await;
    let id_antes = insertar_reporte(
        &pool,
        base_lat,
        base_lng,
        medianoche("2026-04-09") + Duration::hours(23) + Duration::minutes(30),
    )
    .await;

    let query = format!(
        "?desde={}&hasta={}&{}&hora=0",
        instante_query(desde),
        instante_query(hasta),
        bbox_query(base_lat, base_lng)
    );
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(1), "body: {body}");
    let ids = ids_de(&respuesta);
    assert_eq!(ids, vec![id_dentro], "body: {body}");
    assert!(
        !ids.contains(&id_una_hora) && !ids.contains(&id_antes),
        "body: {body}"
    );
}

#[tokio::test]
async fn hora_23_es_valido_y_llega_hasta_el_cambio_de_dia() {
    let (pool, app) = aplicacion(utc()).await;
    limpiar_banda(&pool, -70.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (-70.0 + corte, -70.0 + corte);
    let desde = medianoche("2026-04-09") + Duration::hours(20);
    let hasta = medianoche("2026-04-11") + Duration::hours(4);

    let id_dentro = insertar_reporte(
        &pool,
        base_lat,
        base_lng,
        medianoche("2026-04-09") + Duration::hours(23) + Duration::minutes(30),
    )
    .await;
    let id_cambio_de_dia = insertar_reporte(
        &pool,
        base_lat,
        base_lng,
        medianoche("2026-04-10") + Duration::minutes(30),
    )
    .await;
    let id_antes = insertar_reporte(
        &pool,
        base_lat,
        base_lng,
        medianoche("2026-04-10") + Duration::hours(22) + Duration::minutes(30),
    )
    .await;

    let query = format!(
        "?desde={}&hasta={}&{}&hora=23",
        instante_query(desde),
        instante_query(hasta),
        bbox_query(base_lat, base_lng)
    );
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(1), "body: {body}");
    let ids = ids_de(&respuesta);
    assert_eq!(ids, vec![id_dentro], "body: {body}");
    assert!(
        !ids.contains(&id_cambio_de_dia) && !ids.contains(&id_antes),
        "body: {body}"
    );
}

#[tokio::test]
async fn rechaza_hora_invalida() {
    let (_, app) = aplicacion(caracas()).await;
    for valor in ["24", "-1", "abc", "14.5", ""] {
        let (status, body) = get_reportes(&app, &format!("?hora={valor}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "valor {valor}: {body}");
        assert!(body.contains("hora"), "valor {valor}: {body}");
    }
}

#[tokio::test]
async fn rechaza_intervalo_horas_retirado() {
    let (_, app) = aplicacion(caracas()).await;
    let (status, body) = get_reportes(&app, "?intervalo_horas=1").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    assert!(body.contains("intervalo_horas"), "body: {body}");
}

#[tokio::test]
async fn rechaza_hora_con_parametro_desconocido() {
    let (_, app) = aplicacion(caracas()).await;
    let (status, _) = get_reportes(&app, "?hora=14&foo=1").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn filtra_por_dia_calendario() {
    let (pool, app) = aplicacion(utc()).await;
    limpiar_banda(&pool, 5.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (5.0 + corte, 5.0 + corte);
    let inicio = medianoche("2026-03-15");

    let id_temprano =
        insertar_reporte(&pool, base_lat, base_lng, inicio + Duration::hours(5)).await;
    let id_tardio = insertar_reporte(&pool, base_lat, base_lng, inicio + Duration::hours(23)).await;
    let id_antes = insertar_reporte(&pool, base_lat, base_lng, inicio - Duration::seconds(1)).await;
    let id_despues = insertar_reporte(&pool, base_lat, base_lng, inicio + Duration::days(1)).await;

    let query = format!("?dia=2026-03-15&{}", bbox_query(base_lat, base_lng));
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    let ids = ids_de(&respuesta);
    assert_eq!(respuesta["total"].as_i64(), Some(2), "body: {body}");
    assert_eq!(ids.len(), 2, "body: {body}");
    assert!(ids.contains(&id_temprano) && ids.contains(&id_tardio));
    assert!(!ids.contains(&id_antes) && !ids.contains(&id_despues));
}

#[tokio::test]
async fn dia_incluye_el_inicio_y_excluye_el_fin() {
    let (pool, app) = aplicacion(utc()).await;
    limpiar_banda(&pool, 15.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (15.0 + corte, 15.0 + corte);
    let inicio = medianoche("2026-04-10");

    let id_inicio = insertar_reporte(&pool, base_lat, base_lng, inicio).await;
    let id_fin = insertar_reporte(&pool, base_lat, base_lng, inicio + Duration::days(1)).await;

    let query = format!("?dia=2026-04-10&{}", bbox_query(base_lat, base_lng));
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    let ids = ids_de(&respuesta);
    assert_eq!(ids, vec![id_inicio], "body: {body}");
    assert!(!ids.contains(&id_fin));
    assert_eq!(respuesta["total"].as_i64(), Some(1));
}

#[tokio::test]
async fn dia_se_combina_con_bbox() {
    let (pool, app) = aplicacion(utc()).await;
    limpiar_banda(&pool, 25.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (25.0 + corte, 25.0 + corte);
    let inicio = medianoche("2026-05-20");

    let id_dentro = insertar_reporte(&pool, base_lat, base_lng, inicio + Duration::hours(12)).await;
    let id_fuera = insertar_reporte(
        &pool,
        base_lat + 1.0,
        base_lng + 1.0,
        inicio + Duration::hours(12),
    )
    .await;

    let query = format!("?dia=2026-05-20&{}", bbox_query(base_lat, base_lng));
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    let ids = ids_de(&respuesta);
    assert_eq!(ids, vec![id_dentro], "body: {body}");
    assert!(!ids.contains(&id_fuera));
    assert_eq!(respuesta["total"].as_i64(), Some(1));
}

#[tokio::test]
async fn dia_pagina_con_limit_y_offset() {
    let (pool, app) = aplicacion(utc()).await;
    limpiar_banda(&pool, 35.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (35.0 + corte, 35.0 + corte);
    let inicio = medianoche("2026-06-01");

    let mut ids = Vec::new();
    for horas in 1..=3 {
        ids.push(
            insertar_reporte(&pool, base_lat, base_lng, inicio + Duration::hours(horas)).await,
        );
    }

    let bbox = bbox_query(base_lat, base_lng);
    let (status, body) =
        get_reportes(&app, &format!("?dia=2026-06-01&{bbox}&limit=2&offset=0")).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(3));
    let primera = ids_de(&respuesta);
    assert_eq!(primera.len(), 2, "body: {body}");

    let (status, body) =
        get_reportes(&app, &format!("?dia=2026-06-01&{bbox}&limit=2&offset=2")).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(3));
    let segunda = ids_de(&respuesta);
    assert_eq!(segunda, vec![ids[0].clone()], "body: {body}");
    assert!(primera.iter().all(|id| !segunda.contains(id)));
}

#[tokio::test]
async fn dia_y_hora_se_combinan() {
    let (pool, app) = aplicacion(utc()).await;
    limpiar_banda(&pool, 55.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (55.0 + corte, 55.0 + corte);
    let inicio = medianoche("2026-07-08");

    let id_inicio_franja =
        insertar_reporte(&pool, base_lat, base_lng, inicio + Duration::hours(5)).await;
    let id_en_franja = insertar_reporte(
        &pool,
        base_lat,
        base_lng,
        inicio + Duration::hours(5) + Duration::minutes(30),
    )
    .await;
    let id_antes = insertar_reporte(
        &pool,
        base_lat,
        base_lng,
        inicio + Duration::hours(5) - Duration::minutes(1),
    )
    .await;
    let id_despues = insertar_reporte(&pool, base_lat, base_lng, inicio + Duration::hours(6)).await;

    let query = format!("?dia=2026-07-08&{}&hora=5", bbox_query(base_lat, base_lng));
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(2), "body: {body}");
    let ids = ids_de(&respuesta);
    assert_eq!(ids, vec![id_en_franja, id_inicio_franja], "body: {body}");
    assert!(
        !ids.contains(&id_antes) && !ids.contains(&id_despues),
        "body: {body}"
    );
}

#[tokio::test]
async fn dia_sin_reportes_devuelve_lista_vacia() {
    let (pool, app) = aplicacion(caracas()).await;
    limpiar_banda(&pool, 65.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (65.0 + corte, 65.0 + corte);

    let query = format!("?dia=2001-01-01&{}", bbox_query(base_lat, base_lng));
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["reportes"].as_array().map(Vec::len), Some(0));
    assert_eq!(respuesta["total"].as_i64(), Some(0));
}

#[tokio::test]
async fn rechaza_dia_malformado() {
    let (_, app) = aplicacion(caracas()).await;
    for valor in [
        "no-es-fecha",
        "2026-13-01",
        "2026-02-30",
        "2026-10-05T00:00:00Z",
        "",
    ] {
        let (status, body) = get_reportes(&app, &format!("?dia={valor}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "valor {valor}: {body}");
        assert!(body.contains("dia"), "valor {valor}: {body}");
    }
}

#[tokio::test]
async fn rechaza_dia_incompatible_con_ventana() {
    let (_, app) = aplicacion(caracas()).await;

    let (status, body) = get_reportes(&app, "?dia=2026-10-05&desde=2026-10-04T00:00:00Z").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    assert!(body.contains("dia no es compatible"), "body: {body}");

    let (status, body) = get_reportes(&app, "?dia=2026-10-05&hasta=2026-10-06T00:00:00Z").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    assert!(body.contains("dia no es compatible"), "body: {body}");
}

#[tokio::test]
async fn rechaza_dia_con_parametro_desconocido() {
    let (_, app) = aplicacion(caracas()).await;
    let (status, _) = get_reportes(&app, "?dia=2026-10-05&foo=1").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

fn instante(anio: i32, mes: u32, dia: u32, hora: u32, minuto: u32, segundo: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(anio, mes, dia, hora, minuto, segundo)
        .single()
        .expect("instante válido")
}

#[tokio::test]
async fn dia_usa_la_zona_configurada_caracas() {
    let (pool, app) = aplicacion(caracas()).await;
    limpiar_banda(&pool, 85.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (85.0 + corte, 85.0 + corte);

    let id_fuera_antes =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 5, 3, 59, 59)).await;
    let id_inicio =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 5, 4, 0, 0)).await;
    let id_ultimo =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 6, 3, 59, 59)).await;
    let id_fuera_despues =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 6, 4, 0, 0)).await;

    let query = format!("?dia=2026-10-05&{}", bbox_query(base_lat, base_lng));
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(2), "body: {body}");
    let ids = ids_de(&respuesta);
    assert!(
        ids.contains(&id_inicio) && ids.contains(&id_ultimo),
        "body: {body}"
    );
    assert!(
        !ids.contains(&id_fuera_antes) && !ids.contains(&id_fuera_despues),
        "body: {body}"
    );
}

#[tokio::test]
async fn hora_usa_la_zona_configurada_caracas() {
    let (pool, app) = aplicacion(caracas()).await;
    limpiar_banda(&pool, 45.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (45.0 + corte, 45.0 + corte);

    let id_fuera_antes =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 5, 17, 59, 59)).await;
    let id_inicio =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 5, 18, 0, 0)).await;
    let id_ultimo =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 5, 18, 59, 59)).await;
    let id_fuera_despues =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 5, 19, 0, 0)).await;

    let query = format!(
        "?desde=2026-10-05T00:00:00Z&hasta=2026-10-06T00:00:00Z&hora=14&{}",
        bbox_query(base_lat, base_lng)
    );
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(2), "body: {body}");
    let ids = ids_de(&respuesta);
    assert!(
        ids.contains(&id_inicio) && ids.contains(&id_ultimo),
        "body: {body}"
    );
    assert!(
        !ids.contains(&id_fuera_antes) && !ids.contains(&id_fuera_despues),
        "body: {body}"
    );
}

#[tokio::test]
async fn dia_y_hora_usan_la_zona_configurada_caracas() {
    let (pool, app) = aplicacion(caracas()).await;
    limpiar_banda(&pool, -75.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (-75.0 + corte, -75.0 + corte);

    let id_inicio =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 5, 18, 0, 0)).await;
    let id_media =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 5, 18, 30, 0)).await;
    let id_antes =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 5, 17, 59, 59)).await;
    let id_despues =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 5, 19, 0, 0)).await;

    let query = format!("?dia=2026-10-05&hora=14&{}", bbox_query(base_lat, base_lng));
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(2), "body: {body}");
    let ids = ids_de(&respuesta);
    assert!(
        ids.contains(&id_inicio) && ids.contains(&id_media),
        "body: {body}"
    );
    assert!(
        !ids.contains(&id_antes) && !ids.contains(&id_despues),
        "body: {body}"
    );
}

#[tokio::test]
async fn hora_con_offset_de_minutos_desplaza_la_franja() {
    let (pool, app) = aplicacion(zona(5 * 3600 + 30 * 60)).await;
    limpiar_banda(&pool, -80.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (-80.0 + corte, -80.0 + corte);

    let id_fuera_antes =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 5, 8, 29, 59)).await;
    let id_inicio =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 5, 8, 30, 0)).await;
    let id_ultimo =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 5, 9, 29, 59)).await;
    let id_fuera_despues =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 5, 9, 30, 0)).await;

    let query = format!(
        "?desde=2026-10-05T00:00:00Z&hasta=2026-10-06T00:00:00Z&hora=14&{}",
        bbox_query(base_lat, base_lng)
    );
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(2), "body: {body}");
    let ids = ids_de(&respuesta);
    assert!(
        ids.contains(&id_inicio) && ids.contains(&id_ultimo),
        "body: {body}"
    );
    assert!(
        !ids.contains(&id_fuera_antes) && !ids.contains(&id_fuera_despues),
        "body: {body}"
    );
}

#[tokio::test]
async fn dia_con_offset_de_minutos_cruza_la_medianoche_utc() {
    let (pool, app) = aplicacion(zona(5 * 3600 + 30 * 60)).await;
    limpiar_banda(&pool, -85.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (-85.0 + corte, -85.0 + corte);

    let id_fuera_antes =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 4, 18, 29, 59)).await;
    let id_inicio =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 4, 18, 30, 0)).await;
    let id_ultimo =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 5, 18, 29, 59)).await;
    let id_fuera_despues =
        insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 5, 18, 30, 0)).await;

    let query = format!("?dia=2026-10-05&{}", bbox_query(base_lat, base_lng));
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(2), "body: {body}");
    let ids = ids_de(&respuesta);
    assert!(
        ids.contains(&id_inicio) && ids.contains(&id_ultimo),
        "body: {body}"
    );
    assert!(
        !ids.contains(&id_fuera_antes) && !ids.contains(&id_fuera_despues),
        "body: {body}"
    );
}

#[tokio::test]
async fn zona_configurada_pagina_sobre_la_franja_local() {
    let (pool, app) = aplicacion(caracas()).await;
    limpiar_banda(&pool, 75.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (75.0 + corte, 75.0 + corte);

    let mut ids_franja = Vec::new();
    for minuto in [10, 20, 30] {
        ids_franja.push(
            insertar_reporte(
                &pool,
                base_lat,
                base_lng,
                instante(2026, 10, 5, 18, minuto, 0),
            )
            .await,
        );
    }
    insertar_reporte(&pool, base_lat, base_lng, instante(2026, 10, 5, 17, 50, 0)).await;
    insertar_reporte(
        &pool,
        base_lat + 1.0,
        base_lng + 1.0,
        instante(2026, 10, 5, 18, 15, 0),
    )
    .await;
    ids_franja.reverse();

    let base_query = format!("?dia=2026-10-05&hora=14&{}", bbox_query(base_lat, base_lng));

    let (status, body) = get_reportes(&app, &format!("{base_query}&limit=2&offset=0")).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(3), "body: {body}");
    let primera = ids_de(&respuesta);
    assert_eq!(primera, ids_franja[..2].to_vec(), "body: {body}");

    let (status, body) = get_reportes(&app, &format!("{base_query}&limit=2&offset=2")).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(3), "body: {body}");
    let segunda = ids_de(&respuesta);
    assert_eq!(segunda, vec![ids_franja[2].clone()], "body: {body}");
    assert!(primera.iter().all(|id| !segunda.contains(id)));
}

#[tokio::test]
async fn sin_dia_ni_hora_la_zona_no_afecta_la_consulta() {
    let (pool, app) = aplicacion(caracas()).await;
    limpiar_banda(&pool, -35.0).await;
    let corte = coordenada_unica();
    let (base_lat, base_lng) = (-35.0 + corte, -35.0 + corte);
    let ahora = Utc::now();

    let id_dentro = insertar_reporte(&pool, base_lat, base_lng, ahora - Duration::hours(1)).await;
    let id_fuera = insertar_reporte(&pool, base_lat, base_lng, ahora - Duration::hours(25)).await;

    let query = format!("?{}", bbox_query(base_lat, base_lng));
    let (status, body) = get_reportes(&app, &query).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let respuesta: serde_json::Value = serde_json::from_str(&body).expect("respuesta no es json");
    assert_eq!(respuesta["total"].as_i64(), Some(1), "body: {body}");
    let ids = ids_de(&respuesta);
    assert_eq!(ids, vec![id_dentro], "body: {body}");
    assert!(!ids.contains(&id_fuera));
    assert!(respuesta.get("hora").is_none(), "body: {body}");
}
