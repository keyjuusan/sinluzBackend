# SinLuz — Glosario de Negocio

Terminología acordada para nombrar variables, funciones, rutas y tablas de base de datos.

- **reporte:** registro de un corte de suministro eléctrico notificado por un dispositivo. Es la unidad de entrada del sistema.
- **id_usuario:** identificador opaco del dispositivo que reporta (device_id). Se genera en el cliente y no identifica a una persona; no implica autenticación.
- **lat / lng:** coordenadas geográficas del corte en grados decimales (WGS84).
- **creado:** instante (UTC, RFC 3339) en el que el servidor registra el reporte.
- **horas_duracion:** estimación aproximada, en horas enteras (0–168), de cuánto suelen durar los cortes en la zona según la experiencia del usuario. `NULL` = no lo sabe.
- **cooldown:** medida anti-spam. Un mismo `id_usuario` no puede reportar la misma ubicación (±~100 m) más de una vez cada 15 minutos.
- **feed / consulta de reportes:** lectura de reportes registrados que alimenta el heatmap del cliente. Expuesta por `GET /api/v1/reportes`.
- **ventana temporal:** rango `desde`–`hasta` (RFC 3339) que acota la consulta del feed por el campo `creado`.
- **día calendario (`dia`):** query param opt-in del feed (`YYYY-MM-DD`) que acota la consulta a la jornada completa en la zona horaria configurada (`HORARIO_ZONA`, por defecto `-04:00`, Caracas), ventana semiabierta `[dia 00:00:00, (dia+1) 00:00:00)` local traducida a UTC. Alternativa a `desde`/`hasta`: son mutuamente excluyentes.
- **bbox:** rectángulo geográfico de filtrado definido por `min_lat`, `max_lat`, `min_lng`, `max_lng` (WGS84), opcional en la consulta del feed.
- **actividad:** reflejo en tiempo real de la actividad de otros usuarios (hoy: reportes recién registrados) hacia los clientes conectados. Expuesta por el canal WebSocket `GET /api/v1/actividad/ws`.
- **evento de actividad:** mensaje JSON emitido por el servidor por cada actividad relevante. Clave `tipo` (discriminador, p. ej. `reporte_creado`) + datos del hecho; nunca incluye `id_usuario`.
- **canal de actividad:** canal `broadcast` interno (tokio) que distribuye los eventos de actividad a todas las conexiones WS activas, sin filtro geográfico (global). El filtro de mapa lo hace el cliente.
- **conexión de actividad:** sesión WebSocket de un cliente suscrito al canal. Límite 100 concurrentes; el servidor hace ping cada 30 s y cierra a los 60 s sin actividad. Canal de solo lectura: el cliente no envía mensajes de negocio.
- **hora del día (`hora`):** query param opt-in del feed (entero `0–23`) que acota la consulta a la franja semiabierta `[hora:00, hora+1:00)` **local** (zona `HORARIO_ZONA`) de cada día incluido en la ventana temporal; los reportes fuera de esa franja quedan excluidos.
- **horario (zona horaria):** offset fijo con el que el feed interpreta `dia` y `hora`. Se configura con la variable de entorno `HORARIO_ZONA` (formato `±HH:MM`, p. ej. `-04:00`); por defecto `-04:00` (Caracas, UTC-4). No altera `creado` (siempre UTC RFC 3339) ni los instantes `desde`/`hasta`.
- **origen permitido:** origen HTTP exacto (esquema + host + puerto) autorizado a consumir la API desde el navegador vía CORS. Se define con la variable de entorno `CORS_ALLOWED_ORIGIN`; por defecto `http://localhost:5173` si no se define.
- **documentación de la API:** vista interactiva (Scalar) y documento OpenAPI 3.1 generados desde el código del backend con `utoipa`, que describen las operaciones de la API. Se sirven desde el propio servidor en `GET /api/docs` (UI) y `GET /api/docs/openapi.json` (documento).
- **docs habilitado:** variable de entorno `DOCS_HABILITADO` (booleana) que monta o no la documentación de la API. Ausente o vacía equivale a `false`; cuando está desactivada, `/api/docs` y `/api/docs/openapi.json` responden `404`.