use std::time::Duration;

use axum::{
    body::Bytes,
    extract::ws::{Message, WebSocket},
};
use tokio::{
    sync::broadcast::error::RecvError,
    time::{Instant, interval_at, sleep_until},
};

use crate::modules::actividad::{
    INTERVALO_PING_SEGUNDOS, TIMEOUT_INACTIVIDAD_SEGUNDOS,
    infra::{CanalActividad, ReservaConexion},
};

pub async fn atender_conexion(socket: WebSocket, canal: CanalActividad, reserva: ReservaConexion) {
    atender_conexion_con_keepalive(
        socket,
        canal,
        reserva,
        Duration::from_secs(INTERVALO_PING_SEGUNDOS),
        Duration::from_secs(TIMEOUT_INACTIVIDAD_SEGUNDOS),
    )
    .await;
}

pub async fn atender_conexion_con_keepalive(
    mut socket: WebSocket,
    canal: CanalActividad,
    _reserva: ReservaConexion,
    intervalo_ping: Duration,
    inactividad: Duration,
) {
    let mut suscripcion = canal.suscribir();
    let ahora = Instant::now();
    let mut proximo_ping = interval_at(ahora + intervalo_ping, intervalo_ping);
    let mut ultimo_mensaje_cliente = ahora;

    loop {
        tokio::select! {
            evento = suscripcion.recv() => match evento {
                Ok(evento) => {
                    let Ok(texto) = serde_json::to_string(&evento) else {
                        continue;
                    };
                    if socket.send(Message::Text(texto.into())).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            },
            mensaje = socket.recv() => match mensaje {
                Some(Ok(Message::Ping(payload))) => {
                    ultimo_mensaje_cliente = Instant::now();
                    if socket.send(Message::Pong(payload)).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => ultimo_mensaje_cliente = Instant::now(),
                Some(Err(_)) => break,
            },
            _ = proximo_ping.tick() => {
                if socket.send(Message::Ping(Bytes::new())).await.is_err() {
                    break;
                }
            }
            _ = sleep_until(ultimo_mensaje_cliente + inactividad) => break,
        }
    }
}
