# Spec: WebSocket de Actividad (Reportes en Tiempo Real)

## 1. Visión General e Impacto
- **Objetivo:** Exponer un canal WebSocket (`GET /api/v1/actividad/ws`) que refleje a todos los clientes conectados la actividad de otros usuarios: cada vez que se registra un reporte de corte, el servidor emite el evento a los suscriptores para que el mapa del cliente se actualice en tiempo real sin refetch. Solo lectura: el cliente no envía mensajes de negocio.
- **Contexto:** Módulo nuevo `src/modules/actividad/` (el scaffold `actividad/domain/` e `actividad/infra/` ya existe vacío y sin registrar). Interactúa con el módulo existente `reportes`: el handler `crear_reporte` (spec `approved/01_registrar_reporte`) publica un evento en el canal después de un `201`; un `400`/`429` no publica. El feed `GET /api/v1/reportes` (spec `approved/02_consultar_reportes`) sigue siendo la fuente de verdad histórica: el WS es un complemento incremental, no lo reemplaza. No hay autenticación (pendiente en el roadmap) ni persistencia nueva (sin migración). **Se modifican:** `Cargo.toml`, `src/main.rs`, `src/modules/mod.rs`, `src/modules/reportes/routes.rs`. **Se crean:** `src/modules/actividad/` completo (`mod.rs`, `domain/`, `infra/`, `application/`, `routes.rs`).

### Dependencias autorizadas (esta spec ordena explícitamente tocar `Cargo.toml`)
`[dependencies]`, a modificar por esta spec y solo esto:
- `axum` con features `["ws"]` — habilita `axum::extract::ws` (`WebSocketUpgrade`, `WebSocket`, `Message`), imprescindible para el endpoint. Ya está declarado `axum = "0.8.9"`; se cambia a `axum = { version = "0.8.9", features = ["ws"] }`.

`[dev-dependencies]`, a añadir por esta spec y solo esta:
- `tokio-tungstenite` (sin features TLS) — cliente WebSocket para los tests e2e: conectar al servidor real levantado en un puerto efímero y verificar la recepción de eventos. Es el mismo transporte que usa internamente la feature `ws` de axum.

El broadcast usa `tokio::sync::broadcast` (ya disponible con la feature `full` de `tokio`). Queda prohibido añadir cualquier otra dependencia o modificar el resto de `Cargo.toml`.

## 2. Modelos de Datos / Contratos

### Constantes (`src/modules/actividad/mod.rs`)
```rust
pub const MAX_CONEXIONES_ACTIVIDAD: usize = 100;
pub const INTERVALO_PING_SEGUNDOS: u64 = 30;
pub const TIMEOUT_INACTIVIDAD_SEGUNDOS: u64 = 60;
```

### Evento de dominio (`src/modules/actividad/domain/evento.rs`)
```rust
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct EventoActividad {
    pub tipo: String,                 // constante EVENTO_REPORTE_CREADO = "reporte_creado"
    pub id: String,                   // uuid del reporte
    pub lat: f64,
    pub lng: f64,
    pub creado: String,               // RFC 3339
    pub horas_duracion: Option<u16>,  // null = no lo sabe
}
```
- `id_usuario` jamás se serializa. `tipo` es un discriminador evolutivo (`"reporte_creado"` hoy).
- Serialización siempre como **texto JSON** (`Message::Text`).

### Canal de actividad (`src/modules/actividad/infra/canal.rs`)
```rust
#[derive(Clone)]
pub struct CanalActividad { /* broadcast::Sender<EventoActividad> + Arc<AtomicUsize> + max_conexiones */ }

pub struct ReservaConexion { /* libera el slot en Drop */ }
```
- `CanalActividad::nuevo(max_conexiones: usize)` — en producción `main.rs` construye con `MAX_CONEXIONES_ACTIVIDAD`; los tests inyectan un límite pequeño.
- `reservar() -> Option<ReservaConexion>` — reserva atómica (CAS sobre `AtomicUsize`); devuelve `None` si ya hay `max_conexiones` activas. `ReservaConexion` decrementa en `Drop` ⇒ no hay slots huérfanos ante retornos tempranos o pánico de tarea.
- `suscribir() -> broadcast::Receiver<EventoActividad>`.
- `publicar(&EventoActividad)` — **best-effort**: `send()` con error (cero receptores) no se propaga ni falla el `POST`.

### Composición de estado (`src/main.rs`)
```rust
let canal = CanalActividad::nuevo(MAX_CONEXIONES_ACTIVIDAD);
let app = Router::new()
    .route("/api/health", get(|| async { StatusCode::OK }))
    .merge(modules::reportes::routes::router(pool, canal.clone()))
    .merge(modules::actividad::routes::router(canal));
```
- `reportes::routes::router` pasa a recibir el canal y su estado pasa a contener repositorio + `CanalActividad` (struct de estado propia, `Clone`).

### Contrato del canal WebSocket
- **Handshake:** `GET /api/v1/actividad/ws` con headers `Upgrade: websocket` ⇒ `101 Switching Protocols`.
- **Mensaje de evento (servidor → cliente, texto JSON):**
```json
{ "tipo": "reporte_creado", "id": "9b0e...", "lat": -33.4489, "lng": -70.6693, "creado": "2026-10-06T14:30:00Z", "horas_duracion": 2 }
```
- **Sin mensaje inicial** de bienvenida ni de snapshot: al conectar no se envía nada hasta el primer evento.
- **Mensajes cliente → servidor:** ninguno de negocio. `Text`/`Binary` se descartan (no cierran); `Ping` ⇒ `Pong` con el mismo payload; `Close` ⇒ fin de conexión.

## 3. Reglas de Negocio (Criterios de Aceptación)
- [ ] **Regla 1 (Entradas / límite de conexiones):** antes del upgrade se reserva un slot de forma atómica. Con `MAX_CONEXIONES_ACTIVIDAD` (100) conexiones activas, una solicitud adicional ⇒ `429` con body JSON de error. El slot se libera siempre al terminar la conexión (Drop de `ReservaConexion`), incluidos cierres por timeout, red o error.
- [ ] **Regla 2 (Proceso / publicación):** el handler `crear_reporte` publica el evento en el canal **solo después** de un registro exitoso (`201`). Validación (`400`) y cooldown (`429`) no publican. La publicación es best-effort: sin suscriptores no es error y no altera la respuesta del `POST`.
- [ ] **Regla 3 (Broadcast):** todos los conectados reciben todos los eventos (sin filtro geográfico ni por usuario); el filtro de mapa lo hace el cliente. Si el buffer del `broadcast` se desborda (`Lagged`), la conexión continúa (el cliente reconciliará con `GET /api/v1/reportes`); si el canal se cierra (`Closed`), la conexión WS se cierra.
- [ ] **Regla 4 (Keepalive):** el servidor envía `Ping` cada 30 s; si la conexión no recibe ningún mensaje del cliente (incluido `Pong`) durante 60 s, el servidor la cierra. El cliente no envía mensajes propios más allá del `Pong` del protocolo.
- [ ] **Regla 5 (Salidas):** éxito de handshake ⇒ `101`. Errores convertidos a respuestas HTTP en un único punto del módulo (`routes.rs` de `actividad`): GET sin headers `Upgrade`/`Connection` ⇒ `400`; límite de conexiones ⇒ `429`; fallo de infraestructura ⇒ `500`. El handler no construye respuestas de error a mano. Todos los mensajes de error en español.
- [ ] **Regla 6 (Identidad):** el evento nunca incluye `id_usuario` ni ningún dato que identifique al emisor (coherencia con `02_consultar_reportes`).

## 4. Comportamiento de la Interfaz / API
- **Endpoint:** `GET /api/v1/actividad/ws` (WebSocket)
- **Casos de Éxito:**
  - `101 Switching Protocols` y conexión establecida; el cliente permanece en silencio (solo `Pong` automático).
  - Tras un `POST /api/v1/reportes` que devuelve `201`, **todos** los conectados reciben exactamente un `Message::Text` con el JSON de evento de la §2.
- **Casos de Error:**
  - `400 Bad Request` — GET sin headers `Upgrade: websocket` / `Connection: Upgrade` (el extractor `WebSocketUpgrade` lo rechaza). Body: `{ "error": "cabecera de upgrade websocket ausente o inválida" }`.
  - `429 Too Many Requests` — límite de conexiones alcanzado. Body: `{ "error": "limite de conexiones de actividad alcanzado, reintenta más tarde" }`.
  - `500 Internal Server Error` — fallo de infraestructura. Body: `{ "error": "error interno del servidor" }`.
  - Durante la conexión: el servidor cierra a los 60 s sin actividad del cliente; una desconexión/reconexión del cliente es siempre permitida (el slot queda liberado).

## 5. Estrategia de Pruebas Obligatoria (Tests)
> **Precondición:** los tests e2e requieren Postgres corriendo (`docker compose up -d db`, `DATABASE_URL`) y siguen el patrón existente de `reportes/routes/tests.rs`. Para el WS se levanta el Router real con `axum::serve` sobre un `TcpListener` en `127.0.0.1:0` y se conecta `tokio_tungstenite::connect_async` como cliente; cada test usa un canal con `max_conexiones` inyectado cuando aplique. Los tests de unidad (dominio/canal) no requieren BD.

- **Pruebas de Camino Feliz (Happy Path):**
  - [ ] Handshake `GET /api/v1/actividad/ws` ⇒ `101` y conexión abierta.
  - [ ] `POST /api/v1/reportes` válido con un suscriptor conectado ⇒ el cliente recibe un `Message::Text` cuyo JSON tiene `tipo = "reporte_creado"`, los campos `id`, `lat`, `lng`, `creado` (RFC 3339) y `horas_duracion` coherentes con el `201` y **sin** clave `id_usuario`.
  - [ ] Dos suscriptores conectados reciben **el mismo** evento tras un único `POST` (broadcast global).
  - [ ] Tras cerrar una conexión, la cuenta de slots se libera y es posible conectar de nuevo (hasta el límite).
  - [ ] Unitaria: serialización del evento produce exactamente las claves esperadas (sin `id_usuario`).
  - [ ] Unitaria: `reservar()` devuelve `Some` hasta el límite y `None` al excederlo; soltar la `ReservaConexion` libera el slot.

- **Pruebas de Borde y Errores (Edge Cases):**
  - [ ] GET a la ruta **sin** headers `Upgrade`/`Connection` ⇒ `400` con body `{ "error": ... }` en español.
  - [ ] Con `max_conexiones = 2` inyectado: 2 conexiones abiertas ⇒ tercera solicitud ⇒ `429`; tras cerrar una ⇒ nueva conexión `101`.
  - [ ] `POST /api/v1/reportes` rechazado por cooldown (`429`) o por validación (`400`) ⇒ **no** se emite ningún evento al suscriptor.
  - [ ] El cliente envía `Message::Text("hola")` ⇒ la conexión **no** se cierra y no se retransmite a otros (canal de solo lectura).
  - [ ] Cliente que abre la conexión y no responde pings ⇒ el servidor la cierra hacia los ~60 s (test con `max`/timeouts reducidos en el helper si el arranque lo permite; en su defecto, unitaria sobre la lógica de expiración).
  - [ ] Desconexión abrupta del cliente (drop del socket) ⇒ el slot queda liberado sin fuga (verificable reservando de nuevo hasta el límite).

## 6. Pasos Sugeridos para la Implementación (Orden de Ejecución)
1. Modificar `Cargo.toml` (§1): activar features `["ws"]` en `axum` y añadir dev-dep `tokio-tungstenite` (sin TLS).
2. Crear `src/modules/actividad/mod.rs` con las constantes de la §2 y `pub mod domain; pub mod infra; pub mod application; pub mod routes;`. Registrar `pub mod actividad;` en `src/modules/mod.rs`.
3. Crear `src/modules/actividad/domain/evento.rs`: `EventoActividad` (tipado, `Serialize`, `snake_case`) y la constante `EVENTO_REPORTE_CREADO`; reexportar en `domain/mod.rs`.
4. Crear `src/modules/actividad/infra/canal.rs`: `CanalActividad` (broadcast + contador atómico), `ReservaConexion` con `Drop`, `nuevo`/`reservar`/`suscribir`/`publicar`; reexportar en `infra/mod.rs`. Sin `unwrap`/`expect`/`panic!`.
5. Crear `src/modules/actividad/application/conexion.rs`: caso de uso `atender_conexion(socket: WebSocket, canal: CanalActividad, reserva: ReservaConexion)` con `tokio::select!`: (a) `recv` del broadcast ⇒ `Message::Text(json)` (manejar `Lagged` ⇒ continuar, `Closed` ⇒ salir), (b) mensajes entrantes del cliente (`Ping` ⇒ `Pong`, `Close` ⇒ salir, `Text`/`Binary` ⇒ descartar), (c) tick de ping cada 30 s, (d) expiración de inactividad a los 60 s ⇒ cerrar. Reexportar en `application/mod.rs`.
6. Crear `src/modules/actividad/routes.rs`: handler `conectar(State(canal), WebSocketUpgrade)` que reserva slot, mapea errores ⇒ `400`/`429`/`500` en un único punto (`ErrorHttpActividad` con `IntoResponse`), y ejecuta `on_upgrade` moviendo `reserva` a la tarea. Exponer `pub fn router(canal: CanalActividad) -> Router`. Tests en `routes/tests.rs` (`#[cfg(test)] mod tests;`).
7. Modificar `src/modules/reportes/routes.rs`: estado propio `EstadoReportes { repositorio, canal }`, constructor `router(pool, canal)`, y en `crear_reporte` publicar el evento (`canal.publicar(...)`) **solo** en la rama `Ok` antes del `201`.
8. Modificar `src/main.rs`: crear `CanalActividad::nuevo(MAX_CONEXIONES_ACTIVIDAD)` y componer ambos routers (§2).
9. Escribir los tests de la §5 (e2e con servidor en puerto efímero + `tokio_tungstenite`; unitarias de dominio/canal sin BD).
10. Ejecutar la Fase de Validación completa y autocorregir hasta el 100%:
    - `cargo fmt --all -- --check`
    - `cargo clippy --all-targets -- -D warnings`
    - `cargo check --all-targets`
    - `cargo test`

## Nota para el Frontend (uso del WebSocket desde el cliente)

- **Conexión:** abrir `ws://<host>:1234/api/v1/actividad/ws` (misma IP/puerto que la API REST; en producción, detrás de TLS, `wss://`). Éxito = handshake `101`; un GET sin headers `Upgrade` responde `400` con `{"error": ...}`.
- **Mensajes:** leer cada mensaje de texto como JSON y actuar solo si `tipo == "reporte_creado"`; ignorar tipos desconocidos (el campo `tipo` existe para evolucionar sin romper clientes). El payload trae `id`, `lat`, `lng`, `creado` y `horas_duracion`; **no incluye `id_usuario`** ni quién reportó (anonimato por diseño).
- **Mapa/estado:** al recibir el evento, insertar el punto en el heatmap (o actualizarlo por `id`) sin volver a llamar al feed. El WS **no** envía historial ni snapshot: al cargar la app, el estado inicial se obtiene con `GET /api/v1/reportes` y el WS solo lo mantiene al día (complemento, no reemplazo).
- **Pérdida de eventos:** el servidor puede descartar eventos si el cliente va lento (`Lagged`) — no hay garantía de orden ni de completitud. Al reconectar o detectar un hueco (p. ej. desconexión prolongada), reconciliar con `GET /api/v1/reportes`.
- **Reconexión con backoff:** ante `429` (límite de 100 conexiones alcanzado, body `{"error": "limite de conexiones..."}`), cierre por inactividad (60 s) o caída de red, reconectar con backoff exponencial + jitter y techo razonable (p. ej. 1 s → 2 s → 4 s … máx. 60 s).
- **Keepalive:** el servidor envía `Ping` cada 30 s; el `WebSocket` del navegador responde `Pong` automáticamente — no hace falta lógica propia para eso. **No enviar mensajes** de aplicación: el canal es de solo lectura y los `Text`/`Binary` entrantes se descartan en el servidor.
- **Una sola conexión por app:** compartirla entre componentes (contexto de la SPA) y cerrarla al desmontar; en la PWA, cerrar al pasar a `offline` y reabrir al recuperar la red, aplicando el mismo backoff.

### Ejemplo de código (TypeScript / React)

Tipos del contrato (espejo del evento de la §2; `snake_case` en claves JSON):

```ts
// src/actividad/tipos.ts
export interface EventoReporteCreado {
  tipo: "reporte_creado";
  id: string;
  lat: number;
  lng: number;
  creado: string; // RFC 3339
  horas_duracion: number | null;
}

export interface EventoActividad {
  [clave: string]: unknown; // para validar `tipo` antes de confiar en el payload
}
```

Hook con conexión única, backoff exponencial + jitter, `offline` de la PWA y tipado estricto de mensajes:

```ts
// src/actividad/conexion.ts
const URL_WS = "ws://127.0.0.1:1234/api/v1/actividad/ws";
const BACKOFF_BASE_MS = 1000;
const BACKOFF_MAX_MS = 60_000;

type ManejarEvento = (evento: EventoReporteCreado) => void;

function esReporteCreado(datos: unknown): datos is EventoReporteCreado {
  if (typeof datos !== "object" || datos === null) return false;
  const e = datos as Record<string, unknown>;
  return (
    e.tipo === "reporte_creado" &&
    typeof e.id === "string" &&
    typeof e.lat === "number" &&
    typeof e.lng === "number" &&
    typeof e.creado === "string" &&
    (e.horas_duracion === null || typeof e.horas_duracion === "number")
  );
}

export function conectarActividad(onReporte: ManejarEvento): () => void {
  let ws: WebSocket | null = null;
  let reintento = 0;
  let cerrado = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const backoff = () => {
    const base = Math.min(BACKOFF_BASE_MS * 2 ** reintento, BACKOFF_MAX_MS);
    const conJitter = base / 2 + Math.random() * (base / 2); // evita reconexiones sincronizadas
    reintento += 1;
    timer = setTimeout(abrir, conJitter);
  };

  const abrir = () => {
    if (cerrado || navigator.onLine === false) {
      if (!cerrado) timer = setTimeout(abrir, BACKOFF_BASE_MS); // reintentar al volver la red
      return;
    }
    ws = new WebSocket(URL_WS);

    ws.onopen = () => {
      reintento = 0; // backoff exitoso: volvemos a 1 s
    };

    ws.onmessage = (mensaje) => {
      if (typeof mensaje.data !== "string") return; // solo texto
      let datos: unknown;
      try {
        datos = JSON.parse(mensaje.data);
      } catch {
        return; // payload corrupto: ignorar
      }
      if (esReporteCreado(datos)) onReporte(datos);
      // tipos desconocidos: se ignoran (evolutivo)
    };

    ws.onclose = () => {
      ws = null;
      if (!cerrado) backoff(); // 429, timeout 60 s o caída de red
    };
  };

  const alVolverEnLinea = () => {
    if (navigator.onLine && !cerrado) {
      if (timer) clearTimeout(timer);
      reintento = 0;
      abrir();
    }
  };
  window.addEventListener("online", alVolverEnLinea);

  abrir();

  return () => {
    cerrado = true;
    window.removeEventListener("online", alVolverEnLinea);
    if (timer) clearTimeout(timer);
    ws?.close(); // el servidor libera el slot; una sola conexión por app
  };
}
```

Uso en un componente (el estado inicial viene del feed; el WS solo lo incrementa; ante un hueco se reconcilia con `GET /api/v1/reportes`):

```tsx
// src/componentes/MapaActividad.tsx
function MapaActividad() {
  const [reportes, setReportes] = useState<Reporte[]>([]);

  useEffect(() => {
    // 1. Snapshot inicial (el WS NO envía historial)
    void fetch("/api/v1/reportes?limit=5000")
      .then((r) => r.json())
      .then((d: { reportes: Reporte[] }) => setReportes(d.reportes));

    // 2. Incremento en tiempo real (reconciliar tras reconexiones largas)
    return conectarActividad((evento) => {
      setReportes((prev) =>
        prev.some((r) => r.id === evento.id) ? prev : [...prev, evento],
      );
    });
  }, []);

  return <Heatmap reportes={reportes} />;
}
```

Notas del ejemplo: no se envía ningún mensaje (`ws.send` nunca se usa); `Pong` lo responde el `WebSocket` del navegador ante el `Ping` del servidor; la función de retorno del hook cierra la conexión y limpia temporizadores/listeners.
