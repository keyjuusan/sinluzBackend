use axum::{
    Json, Router,
    extract::{
        State,
        ws::{WebSocketUpgrade, rejection::WebSocketUpgradeRejection},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::json;

use crate::modules::actividad::{application::atender_conexion, infra::CanalActividad};

pub fn router(canal: CanalActividad) -> Router {
    Router::new()
        .route("/api/v1/actividad/ws", get(conectar))
        .with_state(canal)
}

/// Handshake del canal de actividad en tiempo real (WebSocket).
///
/// Tras el upgrade (`101 Switching Protocols`), el servidor emite eventos
/// `reporte_creado` (`EventoActividad`) a todas las conexiones activas cada vez
/// que se registra un reporte. El canal es de solo lectura: el cliente no envía
/// mensajes de negocio. El servidor hace ping cada 30 s y cierra a los 60 s sin
/// actividad.
#[utoipa::path(
    get,
    path = "/api/v1/actividad/ws",
    responses(
        (status = 101, description = "Cambio de protocolo a WebSocket (OpenAPI no modela el protocolo WS)"),
        (status = 429, description = "Límite de conexiones de actividad alcanzado", body = crate::docs::RespuestaError),
        (status = 500, description = "Error interno del servidor", body = crate::docs::RespuestaError),
    )
)]
pub async fn conectar(
    State(canal): State<CanalActividad>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Result<Response, ErrorHttpActividad> {
    let upgrade = upgrade.map_err(ErrorHttpActividad::desde_upgrade)?;
    let reserva = canal
        .reservar()
        .ok_or(ErrorHttpActividad::LimiteConexiones)?;
    Ok(upgrade.on_upgrade(move |socket| atender_conexion(socket, canal, reserva)))
}

#[derive(Debug)]
pub enum ErrorHttpActividad {
    UpgradeInvalido,
    LimiteConexiones,
    Interno,
}

impl ErrorHttpActividad {
    fn desde_upgrade(rechazo: WebSocketUpgradeRejection) -> Self {
        match rechazo {
            WebSocketUpgradeRejection::ConnectionNotUpgradable(_) => ErrorHttpActividad::Interno,
            _ => ErrorHttpActividad::UpgradeInvalido,
        }
    }
}

impl IntoResponse for ErrorHttpActividad {
    fn into_response(self) -> Response {
        let (status, mensaje) = match self {
            ErrorHttpActividad::UpgradeInvalido => (
                StatusCode::BAD_REQUEST,
                "cabecera de upgrade websocket ausente o inválida",
            ),
            ErrorHttpActividad::LimiteConexiones => (
                StatusCode::TOO_MANY_REQUESTS,
                "limite de conexiones de actividad alcanzado, reintenta más tarde",
            ),
            ErrorHttpActividad::Interno => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "error interno del servidor",
            ),
        };
        (status, Json(json!({ "error": mensaje }))).into_response()
    }
}

#[cfg(test)]
mod tests;
