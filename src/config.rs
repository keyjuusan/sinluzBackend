use std::env;
use std::fmt;

use axum::http::HeaderValue;

const ORIGEN_POR_DEFECTO: &str = "http://localhost:5173";

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub cors_allowed_origin: HeaderValue,
    pub docs_habilitado: bool,
}

impl Config {
    pub fn from_env() -> Result<Self, ErrorConfig> {
        let database_url =
            env::var("DATABASE_URL").map_err(|_| ErrorConfig::VariableAusente("DATABASE_URL"))?;
        let cors_allowed_origin = parsear_origen(env::var("CORS_ALLOWED_ORIGIN").ok().as_deref())?;
        let docs_habilitado = parsear_docs_habilitado(env::var("DOCS_HABILITADO").ok().as_deref())?;
        Ok(Self {
            database_url,
            cors_allowed_origin,
            docs_habilitado,
        })
    }
}

fn parsear_docs_habilitado(valor: Option<&str>) -> Result<bool, ErrorConfig> {
    let candidato = match valor {
        Some(valor) if !valor.trim().is_empty() => valor.trim(),
        _ => return Ok(false),
    };
    match candidato.to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(ErrorConfig::DocsInvalido(candidato.to_owned())),
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
    DocsInvalido(String),
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
            ErrorConfig::DocsInvalido(valor) => {
                write!(f, "DOCS_HABILITADO no es un booleano válido: {valor}")
            }
        }
    }
}

impl std::error::Error for ErrorConfig {}

#[cfg(test)]
mod tests {
    use super::{ErrorConfig, ORIGEN_POR_DEFECTO, parsear_docs_habilitado, parsear_origen};

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

    #[test]
    fn docs_ausente_o_vacio_equivale_a_false() {
        assert!(!parsear_docs_habilitado(None).expect("ausente es válido"));
        assert!(!parsear_docs_habilitado(Some("")).expect("vacío es válido"));
        assert!(!parsear_docs_habilitado(Some("   ")).expect("solo espacios es válido"));
    }

    #[test]
    fn docs_acepta_booleanos_ignorando_mayusculas() {
        assert!(parsear_docs_habilitado(Some("true")).expect("true es válido"));
        assert!(parsear_docs_habilitado(Some("TRUE")).expect("TRUE es válido"));
        assert!(parsear_docs_habilitado(Some("True")).expect("True es válido"));
        assert!(!parsear_docs_habilitado(Some("false")).expect("false es válido"));
        assert!(!parsear_docs_habilitado(Some("FALSE")).expect("FALSE es válido"));
    }

    #[test]
    fn docs_invalido_devuelve_error_de_configuracion() {
        let error = parsear_docs_habilitado(Some("sí")).expect_err("debe fallar");
        assert!(matches!(error, ErrorConfig::DocsInvalido(_)));
    }
}
