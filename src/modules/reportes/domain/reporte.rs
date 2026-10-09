use std::fmt;

use chrono::{DateTime, Utc};

pub const MINIMO_LARGO_ID_USUARIO: usize = 8;
pub const MAXIMO_LARGO_ID_USUARIO: usize = 64;
pub const MAXIMO_HORAS_DURACION: u16 = 168;

#[allow(dead_code)]
pub struct Reporte {
    pub id: String,
    pub id_usuario: String,
    pub lat: f64,
    pub lng: f64,
    pub creado: DateTime<Utc>,
    pub horas_duracion: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorValidacion {
    IdUsuarioInvalido,
    LatitudFueraDeRango,
    LongitudFueraDeRango,
    DuracionFueraDeRango,
}

impl fmt::Display for ErrorValidacion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorValidacion::IdUsuarioInvalido => {
                write!(
                    f,
                    "id_usuario inválido: debe tener entre {MINIMO_LARGO_ID_USUARIO} y {MAXIMO_LARGO_ID_USUARIO} caracteres alfanuméricos, '_' o '-'"
                )
            }
            ErrorValidacion::LatitudFueraDeRango => write!(f, "lat fuera de rango [-90, 90]"),
            ErrorValidacion::LongitudFueraDeRango => write!(f, "lng fuera de rango [-180, 180]"),
            ErrorValidacion::DuracionFueraDeRango => {
                write!(
                    f,
                    "horas_duracion inválida: debe estar entre 0 y {MAXIMO_HORAS_DURACION}"
                )
            }
        }
    }
}

impl std::error::Error for ErrorValidacion {}

pub fn validar_datos(
    id_usuario: &str,
    lat: f64,
    lng: f64,
    horas_duracion: Option<u16>,
) -> Result<(), ErrorValidacion> {
    validar_id_usuario(id_usuario)?;
    validar_latitud(lat)?;
    validar_longitud(lng)?;
    validar_duracion(horas_duracion)
}

pub fn validar_id_usuario(id_usuario: &str) -> Result<(), ErrorValidacion> {
    let valido = (MINIMO_LARGO_ID_USUARIO..=MAXIMO_LARGO_ID_USUARIO).contains(&id_usuario.len())
        && id_usuario
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if valido {
        Ok(())
    } else {
        Err(ErrorValidacion::IdUsuarioInvalido)
    }
}

pub fn validar_latitud(lat: f64) -> Result<(), ErrorValidacion> {
    if (-90.0..=90.0).contains(&lat) {
        Ok(())
    } else {
        Err(ErrorValidacion::LatitudFueraDeRango)
    }
}

pub fn validar_longitud(lng: f64) -> Result<(), ErrorValidacion> {
    if (-180.0..=180.0).contains(&lng) {
        Ok(())
    } else {
        Err(ErrorValidacion::LongitudFueraDeRango)
    }
}

pub fn validar_duracion(horas_duracion: Option<u16>) -> Result<(), ErrorValidacion> {
    if horas_duracion.is_some_and(|horas| horas > MAXIMO_HORAS_DURACION) {
        Err(ErrorValidacion::DuracionFueraDeRango)
    } else {
        Ok(())
    }
}
