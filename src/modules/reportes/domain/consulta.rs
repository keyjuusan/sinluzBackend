use std::fmt;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

pub const HORAS_VENTANA_DEFECTO: i64 = 24;
pub const LIMITE_DEFECTO: u32 = 1000;
pub const LIMITE_MAXIMO: u32 = 5000;
pub const MIN_LAT: f64 = -90.0;
pub const MAX_LAT: f64 = 90.0;
pub const MIN_LNG: f64 = -180.0;
pub const MAX_LNG: f64 = 180.0;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolicitudConsultarReportes {
    pub desde: Option<String>,
    pub hasta: Option<String>,
    pub min_lat: Option<String>,
    pub max_lat: Option<String>,
    pub min_lng: Option<String>,
    pub max_lng: Option<String>,
    pub limit: Option<String>,
    pub offset: Option<String>,
}

pub struct Bbox {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lng: f64,
    pub max_lng: f64,
}

pub struct FiltrosConsultaReporte {
    pub desde: DateTime<Utc>,
    pub hasta: DateTime<Utc>,
    pub bbox: Option<Bbox>,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Serialize)]
pub struct ReporteConsulta {
    pub id: String,
    pub lat: f64,
    pub lng: f64,
    pub creado: DateTime<Utc>,
    pub horas_duracion: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorValidacionConsulta {
    DesdeInvalida,
    HastaInvalida,
    VentanaInvalida,
    BboxIncompleto,
    BboxCoordenadaInvalida,
    BboxRangosInvertidos,
    BboxLatitudFueraDeRango,
    BboxLongitudFueraDeRango,
    LimitInvalido,
    OffsetInvalido,
}

impl fmt::Display for ErrorValidacionConsulta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorValidacionConsulta::DesdeInvalida => {
                write!(f, "desde inválido: debe ser una fecha en formato RFC 3339")
            }
            ErrorValidacionConsulta::HastaInvalida => {
                write!(f, "hasta inválido: debe ser una fecha en formato RFC 3339")
            }
            ErrorValidacionConsulta::VentanaInvalida => {
                write!(f, "desde debe ser anterior a hasta")
            }
            ErrorValidacionConsulta::BboxIncompleto => {
                write!(
                    f,
                    "bbox incompleto: deben enviarse min_lat, max_lat, min_lng y max_lng juntos"
                )
            }
            ErrorValidacionConsulta::BboxCoordenadaInvalida => {
                write!(f, "coordenadas del bbox inválidas: deben ser números")
            }
            ErrorValidacionConsulta::BboxRangosInvertidos => {
                write!(
                    f,
                    "bbox inválido: min_lat debe ser menor o igual a max_lat y min_lng menor o igual a max_lng"
                )
            }
            ErrorValidacionConsulta::BboxLatitudFueraDeRango => {
                write!(f, "lat del bbox fuera de rango [-90, 90]")
            }
            ErrorValidacionConsulta::BboxLongitudFueraDeRango => {
                write!(f, "lng del bbox fuera de rango [-180, 180]")
            }
            ErrorValidacionConsulta::LimitInvalido => {
                write!(f, "limit inválido: debe estar entre 1 y {LIMITE_MAXIMO}")
            }
            ErrorValidacionConsulta::OffsetInvalido => {
                write!(f, "offset inválido: debe ser un entero mayor o igual a 0")
            }
        }
    }
}

impl std::error::Error for ErrorValidacionConsulta {}

pub fn construir_filtros(
    solicitud: &SolicitudConsultarReportes,
) -> Result<FiltrosConsultaReporte, ErrorValidacionConsulta> {
    let ahora = Utc::now();
    let hasta = match solicitud.hasta.as_deref() {
        Some(texto) => parse_fecha(texto).ok_or(ErrorValidacionConsulta::HastaInvalida)?,
        None => ahora,
    };
    let desde = match solicitud.desde.as_deref() {
        Some(texto) => parse_fecha(texto).ok_or(ErrorValidacionConsulta::DesdeInvalida)?,
        None => hasta - Duration::hours(HORAS_VENTANA_DEFECTO),
    };
    if desde >= hasta {
        return Err(ErrorValidacionConsulta::VentanaInvalida);
    }

    let bbox = construir_bbox(solicitud)?;
    let limit = parse_u32(
        solicitud.limit.as_deref(),
        LIMITE_DEFECTO,
        |valor| (1..=LIMITE_MAXIMO).contains(&valor),
        ErrorValidacionConsulta::LimitInvalido,
    )?;
    let offset = parse_u32(
        solicitud.offset.as_deref(),
        0,
        |_| true,
        ErrorValidacionConsulta::OffsetInvalido,
    )?;

    Ok(FiltrosConsultaReporte {
        desde,
        hasta,
        bbox,
        limit,
        offset,
    })
}

fn parse_fecha(texto: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(texto)
        .map(|fecha| fecha.with_timezone(&Utc))
        .ok()
}

fn parse_u32(
    valor: Option<&str>,
    defecto: u32,
    valido: impl Fn(u32) -> bool,
    error: ErrorValidacionConsulta,
) -> Result<u32, ErrorValidacionConsulta> {
    match valor {
        None => Ok(defecto),
        Some(texto) => {
            let numero = texto.parse::<u32>().map_err(|_| error)?;
            if valido(numero) {
                Ok(numero)
            } else {
                Err(error)
            }
        }
    }
}

fn construir_bbox(
    solicitud: &SolicitudConsultarReportes,
) -> Result<Option<Bbox>, ErrorValidacionConsulta> {
    match (
        solicitud.min_lat.as_deref(),
        solicitud.max_lat.as_deref(),
        solicitud.min_lng.as_deref(),
        solicitud.max_lng.as_deref(),
    ) {
        (None, None, None, None) => Ok(None),
        (Some(min_lat), Some(max_lat), Some(min_lng), Some(max_lng)) => {
            let min_lat = parse_coordenada(min_lat)?;
            let max_lat = parse_coordenada(max_lat)?;
            let min_lng = parse_coordenada(min_lng)?;
            let max_lng = parse_coordenada(max_lng)?;
            if !(MIN_LAT..=MAX_LAT).contains(&min_lat) || !(MIN_LAT..=MAX_LAT).contains(&max_lat) {
                return Err(ErrorValidacionConsulta::BboxLatitudFueraDeRango);
            }
            if !(MIN_LNG..=MAX_LNG).contains(&min_lng) || !(MIN_LNG..=MAX_LNG).contains(&max_lng) {
                return Err(ErrorValidacionConsulta::BboxLongitudFueraDeRango);
            }
            if min_lat > max_lat || min_lng > max_lng {
                return Err(ErrorValidacionConsulta::BboxRangosInvertidos);
            }
            Ok(Some(Bbox {
                min_lat,
                max_lat,
                min_lng,
                max_lng,
            }))
        }
        _ => Err(ErrorValidacionConsulta::BboxIncompleto),
    }
}

fn parse_coordenada(texto: &str) -> Result<f64, ErrorValidacionConsulta> {
    texto
        .parse::<f64>()
        .map_err(|_| ErrorValidacionConsulta::BboxCoordenadaInvalida)
}
