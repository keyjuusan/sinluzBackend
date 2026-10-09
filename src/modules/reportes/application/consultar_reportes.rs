use std::fmt;

use serde::Serialize;

use crate::modules::reportes::domain::{
    ErrorValidacionConsulta, ReporteConsulta, SolicitudConsultarReportes, construir_filtros,
};
use crate::modules::reportes::infra::PostgresRepositorioReportes;

#[derive(Debug, Serialize)]
pub struct ResultadoConsulta {
    pub reportes: Vec<ReporteConsulta>,
    pub total: i64,
    pub limit: u32,
    pub offset: u32,
}

pub async fn consultar_reportes(
    repositorio: &PostgresRepositorioReportes,
    solicitud: &SolicitudConsultarReportes,
) -> Result<ResultadoConsulta, ErrorConsulta> {
    let filtros = construir_filtros(solicitud).map_err(ErrorConsulta::Validacion)?;

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
