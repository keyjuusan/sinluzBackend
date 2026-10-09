use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use tokio::sync::broadcast;

use crate::modules::actividad::domain::EventoActividad;

const CAPACIDAD_BUFFER_EVENTOS: usize = 256;

#[derive(Clone)]
pub struct CanalActividad {
    emisor: broadcast::Sender<EventoActividad>,
    conexiones_activas: Arc<AtomicUsize>,
    max_conexiones: usize,
}

pub struct ReservaConexion {
    conexiones_activas: Arc<AtomicUsize>,
}

impl Drop for ReservaConexion {
    fn drop(&mut self) {
        self.conexiones_activas.fetch_sub(1, Ordering::AcqRel);
    }
}

impl CanalActividad {
    pub fn nuevo(max_conexiones: usize) -> Self {
        let (emisor, _) = broadcast::channel(CAPACIDAD_BUFFER_EVENTOS);
        Self {
            emisor,
            conexiones_activas: Arc::new(AtomicUsize::new(0)),
            max_conexiones,
        }
    }

    pub fn reservar(&self) -> Option<ReservaConexion> {
        let mut actual = self.conexiones_activas.load(Ordering::Acquire);
        loop {
            if actual >= self.max_conexiones {
                return None;
            }
            match self.conexiones_activas.compare_exchange_weak(
                actual,
                actual + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Some(ReservaConexion {
                        conexiones_activas: Arc::clone(&self.conexiones_activas),
                    });
                }
                Err(viejo) => actual = viejo,
            }
        }
    }

    pub fn suscribir(&self) -> broadcast::Receiver<EventoActividad> {
        self.emisor.subscribe()
    }

    pub fn publicar(&self, evento: &EventoActividad) {
        let _ = self.emisor.send(evento.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evento_ejemplo() -> EventoActividad {
        EventoActividad::reporte_creado(
            "id-de-prueba".to_owned(),
            -33.4,
            -70.6,
            "2026-10-06T14:30:00Z".to_owned(),
            None,
        )
    }

    #[test]
    fn reservar_devuelve_none_al_exceder_el_limite() {
        let canal = CanalActividad::nuevo(2);

        let primera = canal
            .reservar()
            .expect("la primera reserva debe ser válida");
        let segunda = canal
            .reservar()
            .expect("la segunda reserva debe ser válida");

        assert!(
            canal.reservar().is_none(),
            "con el límite alcanzado no debe reservar"
        );
        drop(primera);
        drop(segunda);
    }

    #[test]
    fn soltar_la_reserva_libera_el_slot() {
        let canal = CanalActividad::nuevo(1);

        let reserva = canal
            .reservar()
            .expect("la reserva inicial debe ser válida");
        assert!(canal.reservar().is_none(), "el slot sigue ocupado");

        drop(reserva);

        assert!(
            canal.reservar().is_some(),
            "soltar la reserva debe devolver el slot"
        );
    }

    #[tokio::test]
    async fn los_suscriptores_reciben_lo_publicado() {
        let canal = CanalActividad::nuevo(10);
        let mut suscriptor = canal.suscribir();

        canal.publicar(&evento_ejemplo());

        let evento = suscriptor
            .recv()
            .await
            .expect("el suscriptor debe recibir el evento");
        assert_eq!(evento.id, "id-de-prueba");
        assert_eq!(evento.tipo, "reporte_creado");
    }

    #[tokio::test]
    async fn publicar_sin_suscriptores_no_es_un_error() {
        let canal = CanalActividad::nuevo(10);

        canal.publicar(&evento_ejemplo());
    }
}
