pub mod application;
pub mod domain;
pub mod infra;
pub mod routes;

pub const MAX_CONEXIONES_ACTIVIDAD: usize = 100;
pub const INTERVALO_PING_SEGUNDOS: u64 = 30;
pub const TIMEOUT_INACTIVIDAD_SEGUNDOS: u64 = 60;
