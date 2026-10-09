use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use http_body_util::BodyExt;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

use crate::modules::actividad::{MAX_CONEXIONES_ACTIVIDAD, infra::CanalActividad};

use super::router;

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

async fn aplicacion() -> (sqlx::PgPool, Router) {
    let pool = pool_de_prueba().await;
    let canal = CanalActividad::nuevo(MAX_CONEXIONES_ACTIVIDAD);
    (pool.clone(), router(pool, canal))
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
    let (pool, app) = aplicacion().await;
    let dispositivo = dispositivo_unico();

    let (status, body) = post_reporte(
        &app,
        &json!({
            "id_usuario": dispositivo,
            "lat": -33.4489,
            "lng": -70.6693,
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
    let (pool, app) = aplicacion().await;
    let con_null = dispositivo_unico();
    let omitida = dispositivo_unico();

    let (status, _) = post_reporte(
        &app,
        &json!({
            "id_usuario": con_null,
            "lat": -33.45,
            "lng": -70.67,
            "horas_duracion": null
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = post_reporte(
        &app,
        &format!(r#"{{"id_usuario":"{omitida}","lat":-33.46,"lng":-70.68}}"#),
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
async fn acepta_coordenadas_en_limites() {
    let (_, app) = aplicacion().await;

    let (status, _) = post_reporte(
        &app,
        &json!({
            "id_usuario": dispositivo_unico(),
            "lat": 90.0,
            "lng": 180.0
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = post_reporte(
        &app,
        &json!({
            "id_usuario": dispositivo_unico(),
            "lat": -90.0,
            "lng": -180.0
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn rechaza_coordenadas_fuera_de_rango() {
    let (_, app) = aplicacion().await;

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
    let (_, app) = aplicacion().await;

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
    let (_, app) = aplicacion().await;

    let (status, body) = post_reporte(
        &app,
        &json!({
            "id_usuario": dispositivo_unico(),
            "lat": 12.0,
            "lng": 45.0,
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
            "lat": 12.0,
            "lng": 46.0,
            "horas_duracion": 168
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn rechaza_campo_desconocido() {
    let (_, app) = aplicacion().await;
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
    let (_, app) = aplicacion().await;

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
    let (_, app) = aplicacion().await;
    let dispositivo = dispositivo_unico();
    let cuerpo = json!({
        "id_usuario": dispositivo,
        "lat": -33.45,
        "lng": -70.67
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
    let (_, app) = aplicacion().await;
    let dispositivo = dispositivo_unico();

    for (lat, lng) in [(-33.45, -70.67), (-32.45, -69.67)] {
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
    let (_, app) = aplicacion().await;

    for _ in 0..2 {
        let (status, _) = post_reporte(
            &app,
            &json!({
                "id_usuario": dispositivo_unico(),
                "lat": -33.45,
                "lng": -70.67
            })
            .to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
}

#[tokio::test]
async fn lista_reportes_ordenados_por_creado_con_total() {
    let (pool, app) = aplicacion().await;
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
    let (pool, app) = aplicacion().await;
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
    let (pool, app) = aplicacion().await;
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
    let (pool, app) = aplicacion().await;
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
    let (_, app) = aplicacion().await;
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
    let (pool, app) = aplicacion().await;
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
    let (_, app) = aplicacion().await;

    let (status, body) = get_reportes(&app, "?desde=no-es-fecha").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("desde"), "body: {body}");

    let (status, body) = get_reportes(&app, "?hasta=no-es-fecha").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("hasta"), "body: {body}");
}

#[tokio::test]
async fn rechaza_ventana_invertida() {
    let (_, app) = aplicacion().await;
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
    let (_, app) = aplicacion().await;
    let (status, body) = get_reportes(&app, "?min_lat=10").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("bbox"), "body: {body}");
}

#[tokio::test]
async fn rechaza_bbox_invertido() {
    let (_, app) = aplicacion().await;
    let (status, body) = get_reportes(&app, "?min_lat=10&max_lat=9&min_lng=-75&max_lng=-76").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("bbox"), "body: {body}");
}

#[tokio::test]
async fn rechaza_bbox_fuera_de_rango() {
    let (_, app) = aplicacion().await;

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
    let (_, app) = aplicacion().await;

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
    let (_, app) = aplicacion().await;
    let (status, body) = get_reportes(&app, "?offset=-1").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("offset"), "body: {body}");
}

#[tokio::test]
async fn rechaza_parametro_desconocido() {
    let (_, app) = aplicacion().await;
    let (status, _) = get_reportes(&app, "?foo=1").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
