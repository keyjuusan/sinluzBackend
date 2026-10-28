use country_boundaries::{BOUNDARIES_ODBL_360X180, CountryBoundaries, LatLon, ReadError};

use super::reporte::ErrorValidacion;

pub const ID_PAIS_VENEZUELA: &str = "VE";

pub struct GeocercaVenezuela {
    limites: CountryBoundaries,
}

impl GeocercaVenezuela {
    /// Carga el dataset embebido una única vez (se invoca en el arranque).
    pub fn desde_datos_embebidos() -> Result<Self, ReadError> {
        let limites = CountryBoundaries::from_reader(BOUNDARIES_ODBL_360X180)?;
        Ok(Self { limites })
    }

    /// `Ok(())` si el punto está dentro de Venezuela; error tipado en caso contrario.
    pub fn validar(&self, lat: f64, lng: f64) -> Result<(), ErrorValidacion> {
        let posicion = LatLon::new(lat, lng).map_err(|_| ErrorValidacion::CoordenadaInvalida)?;
        if self.limites.is_in(posicion, ID_PAIS_VENEZUELA) {
            Ok(())
        } else {
            Err(ErrorValidacion::UbicacionFueraDeVenezuela)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geocerca() -> GeocercaVenezuela {
        GeocercaVenezuela::desde_datos_embebidos().expect("el dataset embebido debe cargar")
    }

    #[test]
    fn carga_el_dataset_y_resuelve_venezuela() {
        let geocerca = geocerca();
        assert!(geocerca.validar(10.4806, -66.9036).is_ok());
    }

    #[test]
    fn acepta_ciudades_venezolanas_incluida_la_isla_de_margarita() {
        let geocerca = geocerca();
        for (lat, lng) in [
            (10.4806, -66.9036),
            (10.6545, -71.6405),
            (10.9577, -63.8697),
        ] {
            assert!(
                geocerca.validar(lat, lng).is_ok(),
                "debe aceptar ({lat}, {lng})"
            );
        }
    }

    #[test]
    fn rechaza_ubicaciones_fuera_de_venezuela() {
        let geocerca = geocerca();
        for (lat, lng) in [(-33.4489, -70.6693), (4.7110, -74.0721), (0.0, 0.0)] {
            assert_eq!(
                geocerca.validar(lat, lng),
                Err(ErrorValidacion::UbicacionFueraDeVenezuela),
                "debe rechazar ({lat}, {lng})"
            );
        }
    }

    #[test]
    fn rechaza_coordenadas_no_finitas_sin_panico() {
        let geocerca = geocerca();
        for (lat, lng) in [
            (f64::NAN, -66.9036),
            (10.4806, f64::NAN),
            (f64::INFINITY, -66.9036),
            (10.4806, f64::INFINITY),
        ] {
            assert_eq!(
                geocerca.validar(lat, lng),
                Err(ErrorValidacion::CoordenadaInvalida)
            );
        }
    }
}
