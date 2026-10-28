use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{
        Query, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::FixedOffset;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use utoipa::ToSchema;

use crate::modules::actividad::domain::EventoActividad;
use crate::modules::actividad::infra::CanalActividad;
use crate::modules::reportes::application::{
    ErrorConsulta, ErrorRegistro, ResultadoConsulta, consultar_reportes, registrar_reporte,
};
use crate::modules::reportes::domain::{
    ErrorValidacion, ErrorValidacionConsulta, GeocercaVenezuela, SolicitudConsultarReportes,
};
use crate::modules::reportes::infra::PostgresRepositorioReportes;

#[derive(Clone)]
pub(crate) struct EstadoReportes {
    repositorio: PostgresRepositorioReportes,
    canal: CanalActividad,
    horario_zona: FixedOffset,
    geocerca: Arc<GeocercaVenezuela>,
}

pub fn router(
    pool: PgPool,
    canal: CanalActividad,
    horario_zona: FixedOffset,
    geocerca: Arc<GeocercaVenezuela>,
) -> Router {
    let repositorio = PostgresRepositorioReportes::new(pool);
    Router::new()
        .route("/api/v1/reportes", get(listar_reportes).post(crear_reporte))
        .with_state(EstadoReportes {
            repositorio,
            canal,
            horario_zona,
            geocerca,
        })
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SolicitudCrearReporte {
    /// Identificador opaco del dispositivo que reporta.
    pub id_usuario: String,
    /// Latitud del corte en grados decimales (WGS84).
    pub lat: f64,
    /// Longitud del corte en grados decimales (WGS84).
    pub lng: f64,
    /// Estimación en horas enteras (0–168) de cuánto suele durar el corte. `null` si no se conoce.
    pub horas_duracion: Option<u16>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RespuestaRegistroReporte {
    /// Identificador del reporte creado.
    pub id: String,
    /// Instante UTC (RFC 3339) en que el servidor registró el reporte.
    pub creado: String,
}

/// Registra un corte de suministro eléctrico notificado por un dispositivo.
///
/// Valida el cuerpo, exige que la ubicación esté dentro de Venezuela, aplica la
/// medida anti-spam (cooldown) por dispositivo y ubicación, persiste el reporte
/// y publica el evento de actividad.
#[utoipa::path(
    post,
    path = "/api/v1/reportes",
    request_body = SolicitudCrearReporte,
    responses(
        (status = 201, description = "Reporte registrado", body = RespuestaRegistroReporte),
        (status = 400, description = "Solicitud inválida o ubicación fuera de Venezuela", body = crate::docs::RespuestaError),
        (status = 429, description = "Cooldown activo para el mismo dispositivo y ubicación", body = crate::docs::RespuestaError),
        (status = 500, description = "Error interno del servidor", body = crate::docs::RespuestaError),
    )
)]
pub async fn crear_reporte(
    State(estado): State<EstadoReportes>,
    solicitud: Result<Json<SolicitudCrearReporte>, JsonRejection>,
) -> Result<(StatusCode, Json<RespuestaRegistroReporte>), ErrorHttp> {
    let Json(solicitud) = solicitud.map_err(ErrorHttp::SolicitudInvalida)?;

    let reporte = registrar_reporte(
        &estado.repositorio,
        estado.geocerca.as_ref(),
        &solicitud.id_usuario,
        solicitud.lat,
        solicitud.lng,
        solicitud.horas_duracion,
    )
    .await
    .map_err(ErrorHttp::desde_registro)?;

    let creado = reporte.creado.to_rfc3339();
    estado.canal.publicar(&EventoActividad::reporte_creado(
        reporte.id.clone(),
        reporte.lat,
        reporte.lng,
        creado.clone(),
        reporte.horas_duracion,
    ));

    Ok((
        StatusCode::CREATED,
        Json(RespuestaRegistroReporte {
            id: reporte.id,
            creado,
        }),
    ))
}

/// Consulta el feed de reportes con ventana temporal, bbox, hora del día y paginación.
///
/// Devuelve los reportes ordenados por antigüedad y el total de coincidencias.
#[utoipa::path(
    get,
    path = "/api/v1/reportes",
    params(SolicitudConsultarReportes),
    responses(
        (status = 200, description = "Consulta de reportes", body = ResultadoConsulta),
        (status = 400, description = "Consulta inválida", body = crate::docs::RespuestaError),
        (status = 500, description = "Error interno del servidor", body = crate::docs::RespuestaError),
    )
)]
pub async fn listar_reportes(
    State(estado): State<EstadoReportes>,
    query: Result<Query<SolicitudConsultarReportes>, QueryRejection>,
) -> Result<Json<ResultadoConsulta>, ErrorHttp> {
    let Query(solicitud) = query.map_err(ErrorHttp::ConsultaInvalida)?;

    let resultado = consultar_reportes(&estado.repositorio, &solicitud, estado.horario_zona)
        .await
        .map_err(ErrorHttp::desde_consulta)?;

    Ok(Json(resultado))
}

#[derive(Debug)]
pub enum ErrorHttp {
    SolicitudInvalida(JsonRejection),
    ConsultaInvalida(QueryRejection),
    Validacion(ErrorValidacion),
    Consulta(ErrorValidacionConsulta),
    CooldownActivo,
    Interno,
}

impl ErrorHttp {
    fn desde_registro(error: ErrorRegistro) -> Self {
        match error {
            ErrorRegistro::Validacion(validacion) => ErrorHttp::Validacion(validacion),
            ErrorRegistro::CooldownActivo => ErrorHttp::CooldownActivo,
            ErrorRegistro::Persistencia(_) => ErrorHttp::Interno,
        }
    }

    fn desde_consulta(error: ErrorConsulta) -> Self {
        match error {
            ErrorConsulta::Validacion(validacion) => ErrorHttp::Consulta(validacion),
            ErrorConsulta::Persistencia(_) => ErrorHttp::Interno,
        }
    }
}

impl IntoResponse for ErrorHttp {
    fn into_response(self) -> Response {
        let (status, mensaje) = match self {
            ErrorHttp::SolicitudInvalida(rechazo) => (
                StatusCode::BAD_REQUEST,
                format!("cuerpo de solicitud inválido: {rechazo}"),
            ),
            ErrorHttp::ConsultaInvalida(rechazo) => (
                StatusCode::BAD_REQUEST,
                format!("consulta inválida: {rechazo}"),
            ),
            ErrorHttp::Validacion(error) => (StatusCode::BAD_REQUEST, error.to_string()),
            ErrorHttp::Consulta(error) => (StatusCode::BAD_REQUEST, error.to_string()),
            ErrorHttp::CooldownActivo => (
                StatusCode::TOO_MANY_REQUESTS,
                "cooldown activo: espera 15 minutos antes de reportar la misma ubicación"
                    .to_owned(),
            ),
            ErrorHttp::Interno => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "error interno del servidor".to_owned(),
            ),
        };
        (status, Json(json!({ "error": mensaje }))).into_response()
    }
}

#[cfg(test)]
mod tests;
