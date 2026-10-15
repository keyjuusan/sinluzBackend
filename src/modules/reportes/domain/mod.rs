pub mod consulta;
pub mod reporte;

pub use consulta::{
    ErrorValidacionConsulta, FiltrosConsultaReporte, ReporteConsulta, SolicitudConsultarReportes,
    construir_filtros,
};
pub use reporte::{ErrorValidacion, Reporte, validar_datos};
