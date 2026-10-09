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
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;

use crate::modules::actividad::domain::EventoActividad;
use crate::modules::actividad::infra::CanalActividad;
use crate::modules::reportes::application::{
    ErrorConsulta, ErrorRegistro, ResultadoConsulta, consultar_reportes, registrar_reporte,
};
use crate::modules::reportes::domain::{
    ErrorValidacion, ErrorValidacionConsulta, SolicitudConsultarReportes,
};
use crate::modules::reportes::infra::PostgresRepositorioReportes;

#[derive(Clone)]
pub(crate) struct EstadoReportes {
    repositorio: PostgresRepositorioReportes,
    canal: CanalActividad,
}

pub fn router(pool: PgPool, canal: CanalActividad) -> Router {
    let repositorio = PostgresRepositorioReportes::new(pool);
    Router::new()
        .route("/api/v1/reportes", get(listar_reportes).post(crear_reporte))
        .with_state(EstadoReportes { repositorio, canal })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolicitudCrearReporte {
    pub id_usuario: String,
    pub lat: f64,
    pub lng: f64,
    pub horas_duracion: Option<u16>,
}

#[derive(Debug, Serialize)]
pub struct RespuestaRegistroReporte {
    pub id: String,
    pub creado: String,
}

pub async fn crear_reporte(
    State(estado): State<EstadoReportes>,
    solicitud: Result<Json<SolicitudCrearReporte>, JsonRejection>,
) -> Result<(StatusCode, Json<RespuestaRegistroReporte>), ErrorHttp> {
    let Json(solicitud) = solicitud.map_err(ErrorHttp::SolicitudInvalida)?;

    let reporte = registrar_reporte(
        &estado.repositorio,
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

pub async fn listar_reportes(
    State(estado): State<EstadoReportes>,
    query: Result<Query<SolicitudConsultarReportes>, QueryRejection>,
) -> Result<Json<ResultadoConsulta>, ErrorHttp> {
    let Query(solicitud) = query.map_err(ErrorHttp::ConsultaInvalida)?;

    let resultado = consultar_reportes(&estado.repositorio, &solicitud)
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
