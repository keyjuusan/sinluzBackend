use std::env;
use std::fmt;

use axum::http::HeaderValue;
use chrono::{FixedOffset, Offset, Utc};

const ORIGEN_POR_DEFECTO: &str = "http://localhost:5173";
pub const HORARIO_ZONA_DEFECTO: &str = "-04:00";

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub cors_allowed_origin: HeaderValue,
    pub docs_habilitado: bool,
    pub horario_zona: FixedOffset,
}

impl Config {
    pub fn from_env() -> Result<Self, ErrorConfig> {
        let database_url =
            env::var("DATABASE_URL").map_err(|_| ErrorConfig::VariableAusente("DATABASE_URL"))?;
        let cors_allowed_origin = parsear_origen(env::var("CORS_ALLOWED_ORIGIN").ok().as_deref())?;
        let docs_habilitado = parsear_docs_habilitado(env::var("DOCS_HABILITADO").ok().as_deref())?;
        let horario_zona = resolver_zona(env::var("HORARIO_ZONA").ok().as_deref());
        Ok(Self {
            database_url,
            cors_allowed_origin,
            docs_habilitado,
            horario_zona,
        })
    }
}

fn resolver_zona(valor: Option<&str>) -> FixedOffset {
    match valor.map(str::trim) {
        Some(texto) if !texto.is_empty() => match parsear_zona(texto) {
            Some(zona) => zona,
            None => {
                eprintln!(
                    "HORARIO_ZONA inválida: \"{texto}\". Se usa la zona por defecto ({HORARIO_ZONA_DEFECTO})"
                );
                zona_por_defecto()
            }
        },
        _ => zona_por_defecto(),
    }
}

fn parsear_zona(texto: &str) -> Option<FixedOffset> {
    let bytes = texto.trim().as_bytes();
    if bytes.len() != 6 || bytes[3] != b':' {
        return None;
    }
    let signo = match bytes[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let hora = digito(bytes[1])? * 10 + digito(bytes[2])?;
    let minuto = digito(bytes[4])? * 10 + digito(bytes[5])?;
    if hora > 14 || minuto > 59 {
        return None;
    }
    let segundos = signo * (hora * 3600 + minuto * 60);
    if !(-14 * 3600..=14 * 3600).contains(&segundos) {
        return None;
    }
    FixedOffset::east_opt(segundos)
}

fn digito(byte: u8) -> Option<i32> {
    match byte {
        b'0'..=b'9' => Some(i32::from(byte - b'0')),
        _ => None,
    }
}

fn zona_por_defecto() -> FixedOffset {
    parsear_zona(HORARIO_ZONA_DEFECTO).unwrap_or_else(|| Utc.fix())
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
    use chrono::FixedOffset;

    use super::{
        ErrorConfig, HORARIO_ZONA_DEFECTO, ORIGEN_POR_DEFECTO, parsear_docs_habilitado,
        parsear_origen, parsear_zona, resolver_zona,
    };

    fn offset(segundos: i32) -> FixedOffset {
        FixedOffset::east_opt(segundos).expect("offset de prueba válido")
    }

    #[test]
    fn zona_ausente_o_vacia_usa_caracas() {
        let caracas = offset(-4 * 3600);
        assert_eq!(resolver_zona(None), caracas);
        assert_eq!(resolver_zona(Some("")), caracas);
        assert_eq!(resolver_zona(Some("   ")), caracas);
    }

    #[test]
    fn zona_valida_se_parsea_a_su_offset() {
        let casos = [
            ("-04:00", -4 * 3600),
            ("+00:00", 0),
            ("+05:30", 5 * 3600 + 30 * 60),
            ("-14:00", -14 * 3600),
            ("+14:00", 14 * 3600),
        ];
        for (texto, segundos) in casos {
            assert_eq!(
                resolver_zona(Some(texto)),
                offset(segundos),
                "texto: {texto}"
            );
            assert!(parsear_zona(texto).is_some(), "texto: {texto}");
        }
    }

    #[test]
    fn zona_con_espacios_alrededor_se_parsea() {
        assert_eq!(resolver_zona(Some("  -04:00  ")), offset(-4 * 3600));
    }

    #[test]
    fn zona_invalida_cae_al_valor_por_defecto() {
        let caracas = offset(-4 * 3600);
        for texto in [
            "banana",
            "UTC",
            "America/Caracas",
            "-04",
            "-0400",
            "-4:0",
            "+25:00",
            "+14:30",
            "-15:00",
            "+04:60",
            "2026-10-05",
        ] {
            assert!(
                parsear_zona(texto).is_none(),
                "debería ser inválido: {texto}"
            );
            assert_eq!(resolver_zona(Some(texto)), caracas, "texto: {texto}");
        }
    }

    #[test]
    fn horario_zona_defecto_es_caracas() {
        assert_eq!(HORARIO_ZONA_DEFECTO, "-04:00");
    }

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
