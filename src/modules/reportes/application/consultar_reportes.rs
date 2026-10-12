use std::collections::HashMap;
use std::fmt;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use crate::modules::reportes::domain::{
    ErrorValidacionConsulta, IntervaloReportes, ReporteConsulta, SolicitudConsultarReportes,
    construir_filtros,
};
use crate::modules::reportes::infra::PostgresRepositorioReportes;

#[derive(Debug, Serialize)]
pub struct ResultadoConsulta {
    pub reportes: Vec<ReporteConsulta>,
    pub total: i64,
    pub limit: u32,
    pub offset: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intervalo_horas: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intervalos: Option<Vec<IntervaloReportes>>,
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

    let (intervalo_horas, intervalos) = match filtros.intervalo_horas {
        Some(valor) => {
            let parciales = repositorio
                .contar_reportes_por_intervalo(&filtros, valor)
                .await
                .map_err(ErrorConsulta::Persistencia)?;
            (
                Some(valor),
                Some(construir_serie_intervalos(
                    filtros.desde,
                    filtros.hasta,
                    valor,
                    parciales,
                )),
            )
        }
        None => (None, None),
    };

    Ok(ResultadoConsulta {
        reportes,
        total,
        limit: filtros.limit,
        offset: filtros.offset,
        intervalo_horas,
        intervalos,
    })
}

fn construir_serie_intervalos(
    desde: DateTime<Utc>,
    hasta: DateTime<Utc>,
    intervalo_horas: u32,
    parciales: Vec<IntervaloReportes>,
) -> Vec<IntervaloReportes> {
    let paso = Duration::hours(i64::from(intervalo_horas));
    let mut conteos: HashMap<DateTime<Utc>, i64> = parciales
        .into_iter()
        .map(|intervalo| (intervalo.inicio, intervalo.total))
        .collect();

    let mut serie = Vec::new();
    let mut inicio = desde;
    while inicio < hasta {
        let fin_natural = inicio + paso;
        let fin = fin_natural.min(hasta);
        let total = conteos.remove(&inicio).unwrap_or(0);
        serie.push(IntervaloReportes { inicio, fin, total });
        inicio = fin_natural;
    }
    serie
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
