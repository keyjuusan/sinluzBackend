mod config;
mod cors;
mod docs;
mod modules;

use axum::{Router, http::StatusCode, routing::get};
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;

use crate::config::Config;
use crate::cors::capa;
use crate::modules::actividad::{MAX_CONEXIONES_ACTIVIDAD, infra::CanalActividad};

/// Comprobación de salud del servicio.
#[utoipa::path(
    get,
    path = "/api/health",
    responses((status = 200, description = "Servicio saludable"))
)]
pub(crate) async fn health() -> StatusCode {
    StatusCode::OK
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("Error de configuración: {error}");
            std::process::exit(1);
        }
    };

    let pool = match PgPoolOptions::new()
        .max_connections(5)
        .connect(&config.database_url)
        .await
    {
        Ok(pool) => pool,
        Err(error) => {
            eprintln!("No se pudo conectar a la base de datos: {error}");
            std::process::exit(1);
        }
    };

    if let Err(error) = sqlx::migrate!("./migrations").run(&pool).await {
        eprintln!("No se pudieron aplicar las migraciones: {error}");
        std::process::exit(1);
    }

    let port = 1234;
    let host = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&host).await.unwrap();

    let canal = CanalActividad::nuevo(MAX_CONEXIONES_ACTIVIDAD);

    let app = Router::new()
        .route("/api/health", get(health))
        .merge(modules::reportes::routes::router(
            pool,
            canal.clone(),
            config.horario_zona,
        ))
        .merge(modules::actividad::routes::router(canal))
        .merge(docs::router_opcional(config.docs_habilitado))
        .layer(capa(config.cors_allowed_origin.clone()));

    println!("Servidor iniciado en: http://{host}");
    axum::serve(listener, app).await.unwrap();
}
