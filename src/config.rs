use std::env;
use std::fmt;

use axum::http::HeaderValue;

const ORIGEN_POR_DEFECTO: &str = "http://localhost:5173";

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub cors_allowed_origin: HeaderValue,
}

impl Config {
    pub fn from_env() -> Result<Self, ErrorConfig> {
        let database_url =
            env::var("DATABASE_URL").map_err(|_| ErrorConfig::VariableAusente("DATABASE_URL"))?;
        let cors_allowed_origin = parsear_origen(env::var("CORS_ALLOWED_ORIGIN").ok().as_deref())?;
        Ok(Self {
            database_url,
            cors_allowed_origin,
        })
    }
}

fn parsear_origen(valor: Option<&str>) -> Result<HeaderValue, ErrorConfig> {
    let candidato = match valor {
        Some(valor) if !valor.trim().is_empty() => valor,
        _ => ORIGEN_POR_DEFECTO,
    };
    HeaderValue::from_str(candidato).map_err(|_| ErrorConfig::OrigenInvalido(candidato.to_owned()))
}

#[derive(Debug)]
pub enum ErrorConfig {
    VariableAusente(&'static str),
    OrigenInvalido(String),
}

impl fmt::Display for ErrorConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorConfig::VariableAusente(nombre) => {
                write!(f, "falta la variable de entorno {nombre}")
            }
            ErrorConfig::OrigenInvalido(valor) => {
                write!(f, "CORS_ALLOWED_ORIGIN no es un origen válido: {valor}")
            }
        }
    }
}

impl std::error::Error for ErrorConfig {}

#[cfg(test)]
mod tests {
    use super::{ErrorConfig, ORIGEN_POR_DEFECTO, parsear_origen};

    #[test]
    fn origen_ausente_usa_el_valor_por_defecto() {
        let origen = parsear_origen(None).expect("el origen por defecto es válido");
        assert_eq!(origen, ORIGEN_POR_DEFECTO);
    }

    #[test]
    fn origen_vacio_usa_el_valor_por_defecto() {
        let origen = parsear_origen(Some("")).expect("el origen por defecto es válido");
        assert_eq!(origen, ORIGEN_POR_DEFECTO);
    }

    #[test]
    fn origen_con_solo_espacios_usa_el_valor_por_defecto() {
        let origen = parsear_origen(Some("   ")).expect("el origen por defecto es válido");
        assert_eq!(origen, ORIGEN_POR_DEFECTO);
    }

    #[test]
    fn origen_valido_se_parsea() {
        let origen = parsear_origen(Some("https://sinluz.example")).expect("origen válido");
        assert_eq!(origen, "https://sinluz.example");
    }

    #[test]
    fn origen_invalido_devuelve_error_de_configuracion() {
        let error = parsear_origen(Some("http://sinluz.example\n")).expect_err("debe fallar");
        assert!(matches!(error, ErrorConfig::OrigenInvalido(_)));
    }
}
