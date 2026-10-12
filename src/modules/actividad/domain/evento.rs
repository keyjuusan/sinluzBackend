use serde::Serialize;
use utoipa::ToSchema;

pub const EVENTO_REPORTE_CREADO: &str = "reporte_creado";

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct EventoActividad {
    /// Discriminador del tipo de evento (p. ej. `reporte_creado`).
    pub tipo: String,
    /// Identificador del reporte que originó el evento.
    pub id: String,
    /// Latitud del corte en grados decimales (WGS84).
    pub lat: f64,
    /// Longitud del corte en grados decimales (WGS84).
    pub lng: f64,
    /// Instante UTC (RFC 3339) de creación del reporte.
    pub creado: String,
    /// Duración estimada en horas enteras. `null` si no se conoce.
    pub horas_duracion: Option<u16>,
}

impl EventoActividad {
    pub fn reporte_creado(
        id: String,
        lat: f64,
        lng: f64,
        creado: String,
        horas_duracion: Option<u16>,
    ) -> Self {
        Self {
            tipo: EVENTO_REPORTE_CREADO.to_owned(),
            id,
            lat,
            lng,
            creado,
            horas_duracion,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evento_ejemplo() -> EventoActividad {
        EventoActividad::reporte_creado(
            "9b0e1d2c-3a4b-4c5d-8e9f-0a1b2c3d4e5f".to_owned(),
            -33.4489,
            -70.6693,
            "2026-10-06T14:30:00Z".to_owned(),
            Some(2),
        )
    }

    #[test]
    fn serializa_exactamente_las_claves_del_evento() {
        let valor =
            serde_json::to_value(evento_ejemplo()).expect("el evento debe ser serializable");
        let objeto = valor
            .as_object()
            .expect("el evento debe serializarse como objeto JSON");

        assert_eq!(objeto.len(), 6);
        for clave in ["tipo", "id", "lat", "lng", "creado", "horas_duracion"] {
            assert!(objeto.contains_key(clave), "falta la clave {clave}");
        }
        assert!(
            !objeto.contains_key("id_usuario"),
            "el evento jamás debe incluir id_usuario"
        );
        assert_eq!(
            objeto.get("tipo").and_then(|tipo| tipo.as_str()),
            Some(EVENTO_REPORTE_CREADO)
        );
        assert_eq!(
            objeto.get("id").and_then(|id| id.as_str()),
            Some("9b0e1d2c-3a4b-4c5d-8e9f-0a1b2c3d4e5f")
        );
        assert_eq!(
            objeto.get("lat").and_then(|lat| lat.as_f64()),
            Some(-33.4489)
        );
        assert_eq!(
            objeto.get("lng").and_then(|lng| lng.as_f64()),
            Some(-70.6693)
        );
        assert_eq!(
            objeto.get("creado").and_then(|creado| creado.as_str()),
            Some("2026-10-06T14:30:00Z")
        );
        assert_eq!(
            objeto
                .get("horas_duracion")
                .and_then(|horas| horas.as_u64()),
            Some(2)
        );
    }

    #[test]
    fn serializa_la_duracion_desconocida_como_null() {
        let evento = EventoActividad::reporte_creado(
            "abc".to_owned(),
            1.0,
            2.0,
            "2026-10-06T14:30:00Z".to_owned(),
            None,
        );
        let valor = serde_json::to_value(evento).expect("el evento debe ser serializable");
        let objeto = valor
            .as_object()
            .expect("el evento debe serializarse como objeto JSON");
        assert!(
            objeto
                .get("horas_duracion")
                .is_some_and(|horas| horas.is_null()),
            "horas_duracion debe serializarse como null"
        );
    }
}
