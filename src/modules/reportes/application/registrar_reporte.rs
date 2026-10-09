use std::fmt;

use chrono::Utc;

use crate::modules::reportes::domain::{ErrorValidacion, Reporte, validar_datos};
use crate::modules::reportes::infra::PostgresRepositorioReportes;

pub async fn registrar_reporte(
    repositorio: &PostgresRepositorioReportes,
    id_usuario: &str,
    lat: f64,
    lng: f64,
    horas_duracion: Option<u16>,
) -> Result<Reporte, ErrorRegistro> {
    validar_datos(id_usuario, lat, lng, horas_duracion).map_err(ErrorRegistro::Validacion)?;

    if repositorio
        .existe_reporte_reciente(id_usuario, lat, lng)
        .await
        .map_err(ErrorRegistro::Persistencia)?
    {
        return Err(ErrorRegistro::CooldownActivo);
    }

    let creado = Utc::now();
    repositorio
        .insertar_reporte(id_usuario, lat, lng, creado, horas_duracion)
        .await
        .map_err(ErrorRegistro::Persistencia)
}

#[derive(Debug)]
pub enum ErrorRegistro {
    Validacion(ErrorValidacion),
    CooldownActivo,
    Persistencia(sqlx::Error),
}

impl fmt::Display for ErrorRegistro {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorRegistro::Validacion(error) => write!(f, "{error}"),
            ErrorRegistro::CooldownActivo => {
                write!(
                    f,
                    "cooldown activo: existe un reporte reciente del mismo dispositivo en la misma ubicación"
                )
            }
            ErrorRegistro::Persistencia(error) => write!(f, "error de persistencia: {error}"),
        }
    }
}

impl std::error::Error for ErrorRegistro {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ErrorRegistro::Validacion(error) => Some(error),
            ErrorRegistro::Persistencia(error) => Some(error),
            ErrorRegistro::CooldownActivo => None,
        }
    }
}
