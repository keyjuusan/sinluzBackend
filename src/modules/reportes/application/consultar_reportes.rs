use std::fmt;

use chrono::FixedOffset;
use serde::Serialize;
use utoipa::ToSchema;

use crate::modules::reportes::domain::{
    ErrorValidacionConsulta, ReporteConsulta, SolicitudConsultarReportes, construir_filtros,
};
use crate::modules::reportes::infra::PostgresRepositorioReportes;

#[derive(Debug, Serialize, ToSchema)]
pub struct ResultadoConsulta {
    /// Reportes que coinciden con la consulta, ordenados por antigüedad.
    pub reportes: Vec<ReporteConsulta>,
    /// Total de reportes que coinciden con los filtros (ignora `limit`/`offset`).
    pub total: i64,
    /// Número máximo de reportes solicitados.
    pub limit: u32,
    /// Número de reportes saltados.
    pub offset: u32,
}

pub async fn consultar_reportes(
    repositorio: &PostgresRepositorioReportes,
    solicitud: &SolicitudConsultarReportes,
    horario_zona: FixedOffset,
) -> Result<ResultadoConsulta, ErrorConsulta> {
    let filtros = construir_filtros(solicitud, horario_zona).map_err(ErrorConsulta::Validacion)?;

    let total = repositorio
        .contar_reportes(&filtros)
        .await
        .map_err(ErrorConsulta::Persistencia)?;
    let reportes = repositorio
        .listar_reportes(&filtros)
        .await
        .map_err(ErrorConsulta::Persistencia)?;

    Ok(ResultadoConsulta {
        reportes,
        total,
        limit: filtros.limit,
        offset: filtros.offset,
    })
}

#[derive(Debug)]
pub enum ErrorConsulta {
    Validacion(ErrorValidacionConsulta),
    Persistencia(sqlx::Error),
}

impl fmt::Display for ErrorConsulta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorConsulta::Validacion(error) => write!(f, "{error}"),
            ErrorConsulta::Persistencia(error) => write!(f, "error de persistencia: {error}"),
        }
    }
}

impl std::error::Error for ErrorConsulta {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ErrorConsulta::Validacion(error) => Some(error),
            ErrorConsulta::Persistencia(error) => Some(error),
        }
    }
}
