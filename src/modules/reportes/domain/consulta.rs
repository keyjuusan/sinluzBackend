use std::fmt;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

pub const HORAS_VENTANA_DEFECTO: i64 = 24;
pub const LIMITE_DEFECTO: u32 = 1000;
pub const LIMITE_MAXIMO: u32 = 5000;
pub const MIN_LAT: f64 = -90.0;
pub const MAX_LAT: f64 = 90.0;
pub const MIN_LNG: f64 = -180.0;
pub const MAX_LNG: f64 = 180.0;
pub const HORA_MIN: u8 = 0;
pub const HORA_MAX: u8 = 23;

#[derive(Debug, Deserialize, IntoParams, ToSchema)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct SolicitudConsultarReportes {
    /// Día calendario (`YYYY-MM-DD`) en la zona horaria configurada (`HORARIO_ZONA`,
    /// por defecto `-04:00`, Caracas) que acota la consulta a la jornada completa.
    /// Alternativa a `desde`/`hasta`: son mutuamente excluyentes.
    pub dia: Option<String>,
    /// Inicio de la ventana temporal (RFC 3339). Por defecto, 24 horas antes de `hasta`.
    pub desde: Option<String>,
    /// Fin de la ventana temporal (RFC 3339). Por defecto, el instante actual.
    pub hasta: Option<String>,
    /// Latitud mínima del bbox (WGS84).
    #[param(value_type = Option<f64>)]
    pub min_lat: Option<String>,
    /// Latitud máxima del bbox (WGS84).
    #[param(value_type = Option<f64>)]
    pub max_lat: Option<String>,
    /// Longitud mínima del bbox (WGS84).
    #[param(value_type = Option<f64>)]
    pub min_lng: Option<String>,
    /// Longitud máxima del bbox (WGS84).
    #[param(value_type = Option<f64>)]
    pub max_lng: Option<String>,
    /// Número máximo de reportes a devolver (1–5000; por defecto 1000).
    #[param(value_type = Option<u32>)]
    pub limit: Option<String>,
    /// Número de reportes a saltar (mayor o igual a 0; por defecto 0).
    #[param(value_type = Option<u32>)]
    pub offset: Option<String>,
    /// Hora del día en la zona horaria configurada (`HORARIO_ZONA`, por defecto
    /// `-04:00`, Caracas) (0–23) que acota la consulta a la franja `[hora, hora+1)`.
    #[param(value_type = Option<u8>)]
    pub hora: Option<String>,
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
    pub hora: Option<u8>,
    pub horario_zona: FixedOffset,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReporteConsulta {
    /// Identificador del reporte.
    pub id: String,
    /// Latitud del corte en grados decimales (WGS84).
    pub lat: f64,
    /// Longitud del corte en grados decimales (WGS84).
    pub lng: f64,
    /// Instante UTC (RFC 3339) en que el servidor registró el reporte.
    pub creado: DateTime<Utc>,
    /// Estimación en horas enteras de la duración del corte. `null` si no se conoce.
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
    HoraInvalida,
    DiaInvalido,
    DiaIncompatible,
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
            ErrorValidacionConsulta::HoraInvalida => {
                write!(f, "hora inválida: debe estar entre 0 y 23")
            }
            ErrorValidacionConsulta::DiaInvalido => {
                write!(f, "dia inválido: debe ser una fecha en formato YYYY-MM-DD")
            }
            ErrorValidacionConsulta::DiaIncompatible => {
                write!(f, "dia no es compatible con desde/hasta")
            }
        }
    }
}

impl std::error::Error for ErrorValidacionConsulta {}

pub fn construir_filtros(
    solicitud: &SolicitudConsultarReportes,
    horario_zona: FixedOffset,
) -> Result<FiltrosConsultaReporte, ErrorValidacionConsulta> {
    let (desde, hasta) = resolver_ventana(solicitud, horario_zona)?;

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
    let hora = parse_hora(solicitud.hora.as_deref())?;

    Ok(FiltrosConsultaReporte {
        desde,
        hasta,
        bbox,
        limit,
        offset,
        hora,
        horario_zona,
    })
}

fn resolver_ventana(
    solicitud: &SolicitudConsultarReportes,
    horario_zona: FixedOffset,
) -> Result<(DateTime<Utc>, DateTime<Utc>), ErrorValidacionConsulta> {
    if let Some(texto) = solicitud.dia.as_deref() {
        if solicitud.desde.is_some() || solicitud.hasta.is_some() {
            return Err(ErrorValidacionConsulta::DiaIncompatible);
        }
        return parse_dia(texto, horario_zona);
    }

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
    Ok((desde, hasta))
}

pub fn parse_dia(
    texto: &str,
    horario_zona: FixedOffset,
) -> Result<(DateTime<Utc>, DateTime<Utc>), ErrorValidacionConsulta> {
    let fecha = NaiveDate::parse_from_str(texto, "%Y-%m-%d")
        .map_err(|_| ErrorValidacionConsulta::DiaInvalido)?;
    let inicio_local = fecha
        .and_hms_opt(0, 0, 0)
        .ok_or(ErrorValidacionConsulta::DiaInvalido)?
        .and_utc();
    let inicio = inicio_local - Duration::seconds(i64::from(horario_zona.local_minus_utc()));
    let fin = inicio + Duration::days(1);
    Ok((inicio, fin))
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

fn parse_hora(valor: Option<&str>) -> Result<Option<u8>, ErrorValidacionConsulta> {
    let Some(texto) = valor else {
        return Ok(None);
    };
    let hora = texto
        .parse::<u8>()
        .map_err(|_| ErrorValidacionConsulta::HoraInvalida)?;
    if !(HORA_MIN..=HORA_MAX).contains(&hora) {
        return Err(ErrorValidacionConsulta::HoraInvalida);
    }
    Ok(Some(hora))
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
