# Spec: Horario Configurable por Variable de Entorno

## 1. Visión General e Impacto
- **Objetivo:** Habilitar la variable de entorno **`HORARIO_ZONA`** para configurar la zona horaria con la que el feed interpreta el query param `dia` (jornada calendario) y el query param `hora` (franja del día). Por defecto queda **Caracas (`-04:00`, UTC-4)**. Así "el día" y "la hora" del heatmap se miden en el horario local del servicio en lugar de en UTC, sin cambiar la API ni el contrato de respuesta.
- **Contexto:** Extiende la configuración existente (`src/config.rs`: `DATABASE_URL`, `CORS_ALLOWED_ORIGIN`, `DOCS_HABILITADO`) y los filtros aprobados `dia` (spec `06`) y `hora` (spec `05`) del módulo `reportes`. Es **configuración puramente de servidor**: no crea endpoints, no añade query params, no modifica el body de `GET /api/v1/reportes` ni los códigos de estado. Al ser un **offset fijo** (Caracas no tiene horario de verano desde 2016), no se necesitan reglas DST ni librerías de zonas.
  - Se **modifican**: `src/config.rs` (campo `horario_zona`, parser y tests), `src/main.rs` (pasa la zona al router), `src/modules/reportes/routes.rs` (`EstadoReportes`, `router`, handler), `src/modules/reportes/application/consultar_reportes.rs` (firma), `src/modules/reportes/domain/consulta.rs` (firmas de `construir_filtros`/`resolver_ventana`/`parse_dia`, campo en `FiltrosConsultaReporte`, doc comments de `dia`/`hora`), `src/modules/reportes/infra/postgres_repositorio.rs` (binding del offset en `listar_reportes`/`contar_reportes`), `src/modules/reportes/routes/tests.rs`, `.env.example` y documentación (`specs/GLOSSARY.md`, `specs/ARCHITECT.md`, referencias de `specs/approved/05_filtro_por_hora.md` y `specs/approved/06_filtro_por_dia.md`).
  - **No** se crea ningún módulo nuevo ni migración (la zona es configuración volátil, no estado) y **no** hay autenticación.
  - Sin cambios en: `FiltrosConsultaReporte.desde`/`hasta` (siguen siendo instantes absolutos), el campo `creado` (sigue UTC RFC 3339 en respuestas y eventos WS), el cooldown, `bbox` ni la paginación.

### Dependencias autorizadas (esta spec ordena explícitamente tocar `Cargo.toml`)
Ninguna. Queda prohibido añadir librerías o modificar `Cargo.toml`. El offset se resuelve con `chrono::FixedOffset`, ya presente en el árbol de dependencias (feature `chrono` activa en SQLx).

## 2. Modelos de Datos / Contratos

### Configuración (`src/config.rs`)
```rust
pub const HORARIO_ZONA_DEFECTO: &str = "-04:00"; // Caracas, UTC-4
```
- `Config` gana el campo `pub horario_zona: chrono::FixedOffset`.
- Nuevo helper puro `fn parsear_zona(texto: &str) -> Option<FixedOffset>`:
  - Acepta **únicamente** el formato `±HH:MM` (signo `+`/`-`, exactamente 2 dígitos de hora, `:`, exactamente 2 dígitos de minuto), p. ej. `-04:00`, `+05:30`, `+00:00`, `-14:00`.
  - Rangos: hora `00..=14`, minuto `00..=59` y offset total en `[-14:00, +14:00]`.
  - Se `trim()` el valor antes de validar. Cualquier otro formato (`banana`, `UTC`, `America/Caracas`, `-04`, `-0400`, `-4:0`, `+14:30`, `+15:00`, `-15:00`, signo ausente) ⇒ `None`.
  - Construcción con `FixedOffset::east_opt(segundos)` (nunca `unwrap`/`expect`); al haber validado el rango, `None` es inalcanzable y se resuelve sin pánico.
- `Config::from_env` llama a `parsear_zona(env::var("HORARIO_ZONA").ok().as_deref())`:
  - Valor **ausente** o **vacío/solo espacios** ⇒ `HORARIO_ZONA_DEFECTO` (`-04:00`), silenciosamente (mismo criterio que `CORS_ALLOWED_ORIGIN`/`DOCS_HABILITADO`).
  - Valor **presente e inválido** ⇒ `eprintln!` de aviso en el arranque (`HORARIO_ZONA inválida: "<valor>". Se usa la zona por defecto (-04:00)`) y se aplica `HORARIO_ZONA_DEFECTO`. **El servidor siempre arranca**: esta variable nunca aborta (a diferencia de `DATABASE_URL`).
  - **No** se añaden variantes a `ErrorConfig` (el parser es total vía fallback).

### Dominio (`src/modules/reportes/domain/consulta.rs`)
- `construir_filtros(solicitud: &SolicitudConsultarReportes, horario_zona: FixedOffset) -> Result<FiltrosConsultaReporte, ErrorValidacionConsulta>` (nuevo parámetro).
- `FiltrosConsultaReporte` gana `pub horario_zona: FixedOffset` (para que el repositorio aplique el offset al filtro `hora`).
- `parse_dia(texto: &str, horario_zona: FixedOffset) -> Result<(DateTime<Utc>, DateTime<Utc>), ErrorValidacionConsulta>`: la jornada `[dia 00:00, (dia+1) 00:00)` se construye en la zona y se convierte a UTC restando `horario_zona.local_minus_utc()` segundos. Con Caracas, `dia=2026-10-05` ⇒ ventana UTC `[2026-10-05T04:00:00Z, 2026-10-06T04:00:00Z)`.
- `resolver_ventana` recibe y propaga `horario_zona`. `ErrorValidacionConsulta` **no cambia** (ninguna variante nueva: la zona no produce errores HTTP).
- Doc comments de `dia` y `hora` (alimentan `IntoParams`/OpenAPI): sustituir "en UTC" por "en la zona horaria configurada (`HORARIO_ZONA`, por defecto `-04:00`, Caracas)".

### Aplicación y composición
- `consultar_reportes(repositorio, solicitud, horario_zona)` propaga la zona a `construir_filtros`.
- `EstadoReportes` gana `horario_zona: FixedOffset`; `router(pool: PgPool, canal: CanalActividad, horario_zona: FixedOffset)`; `main.rs` pasa `config.horario_zona`.

### Persistencia (`src/modules/reportes/infra/postgres_repositorio.rs`)
- Ambas queries estáticas ganan un binding entero **`$8` = offset en segundos** (`filtros.horario_zona.local_minus_utc()`, nunca `NULL`), condición de hora pasada a hora **local** y renumeración de la paginación:
  - `listar_reportes`: `AND ($7::int IS NULL OR EXTRACT(HOUR FROM ((creado + ($8::int * interval '1 second')) AT TIME ZONE 'UTC'))::int = $7)` … `LIMIT $9 OFFSET $10`.
  - `contar_reportes`: misma condición con `$7`/`$8` (sin LIMIT/OFFSET).
  - Lectura: desplazar `creado` por el offset fijo y leer la hora UTC resultante = hora local. Ej.: offset `-14400`, `creado = 18:30Z` ⇒ `14:30` ⇒ hora local `14`.
  - Sin SQL dinámico por entrada de usuario: solo un binding numérico fijo más, mismo patrón que bbox/hora. Si sqlx/Postgres requiriera otro casting equivalente (p. ej. `make_interval(secs => $8::double precision)` con `f64`), se ajusta manteniendo exactamente la misma semántica.

### Contratos HTTP (sin cambios de forma)
- **Endpoint:** `GET /api/v1/reportes` — mismos params (`dia`, `desde`, `hasta`, bbox, `limit`, `offset`, `hora`), mismo body `{reportes, total, limit, offset}` y mismos códigos.
- **Semántica de los params afectados por `HORARIO_ZONA`:**
  | Parámetro | Antes (UTC) | Ahora |
  |---|---|---|
  | `dia` | jornada `[00:00Z, +1d 00:00Z)` | jornada local `[00:00 zona, +1d 00:00 zona)` convertida a UTC |
  | `hora` | franja `[hora, hora+1)` UTC | franja `[hora, hora+1)` **local** de cada día |
- **Sin cambios:** `creado` (UTC RFC 3339), `desde`/`hasta` (instantes absolutos RFC 3339, la zona del emisor ya va en el propio offset), cooldown y resto del body.

## 3. Reglas de Negocio (Criterios de Aceptación)
- [ ] **Regla 1 (Entradas):** `HORARIO_ZONA` ausente o vacía ⇒ Caracas (`-04:00`). Presente ⇒ debe cumplir `±HH:MM` con hora `00..=14`, minuto `00..=59` y offset total dentro de `[-14:00, +14:00]`; si no cumple ⇒ aviso en stderr y valor por defecto `-04:00`. La variable **nunca** impide el arranque del servidor.
- [ ] **Regla 2 (Proceso — `dia`):** `dia=YYYY-MM-DD` acota a la jornada completa **en la zona configurada**: ventana semiabierta `[dia 00:00 zona, (dia+1) 00:00 zona)` traducida a UTC sobre `creado`. Con `-04:00`, `dia=2026-10-05` ⇒ `[2026-10-05T04:00Z, 2026-10-06T04:00Z)`: un reporte a las `03:59:59Z` del día 6 **no** entra y uno a las `04:00:00Z` **sí**.
- [ ] **Regla 3 (Proceso — `hora`):** `hora=N` acota a la franja semiabierta `[N:00, N+1:00)` **local** de cada día de la ventana. Con `-04:00`, `hora=14` ⇒ franja UTC `[18:00, 19:00)`: entra un reporte a las `18:00:00Z` y no uno a las `19:00:00Z`. Con offsets de minutos (`+05:30`), `hora=14` ⇒ UTC `[08:30, 09:30)` (franja desplazada correctamente aunque no caiga en hora exacta UTC).
- [ ] **Regla 4 (Alcance):** la zona **solo** gobierna `dia` y `hora`. El campo `creado` en respuestas y eventos de actividad sigue siendo UTC RFC 3339; `desde`/`hasta` siguen siendo instantes absolutos; el cooldown no cambia. Con `HORARIO_ZONA=+00:00` el comportamiento es idéntico al actual (regresión UTC).
- [ ] **Regla 5 (Idempotencia/límites):** el offset es fijo (sin DST) y solo depende de la config de arranque; la misma petición con la misma configuración produce siempre los mismos `reportes` y `total`. No hay límites nuevos, ni params nuevos, ni cooldown adicional.
- [ ] **Regla 6 (Salidas):** sin códigos nuevos: éxito `200` con el body de la spec `02`; validaciones del feed siguen `400`; infraestructura `500`. La zona **no** aparece en el body ni genera errores HTTP. Errores convertidos en el único punto del módulo (spec `05`), handlers intactos.

## 4. Comportamiento de la Interfaz / API (Si aplica)
- **Endpoint:** `GET /api/v1/reportes` (ruta, verbo, params y shape de respuesta **sin cambios**; solo cambia la interpretación temporal de `dia` y `hora`). Config: variable de entorno `HORARIO_ZONA`.
- **Casos de Éxito (con `HORARIO_ZONA` por defecto `-04:00`):**
  - `GET /api/v1/reportes?dia=2026-10-05` ⇒ `200` con los reportes de la jornada local del 5 de octubre (UTC `04:00Z` del 5 a `04:00Z` del 6).
  - `GET /api/v1/reportes?hora=14` ⇒ `200` solo con reportes de las `18:00–19:00Z` de cada día de la ventana, `total` coherente.
  - `GET /api/v1/reportes?dia=2026-10-05&hora=14` ⇒ `200` con los reportes de las `18:00–19:00Z` del día 5.
  - `GET /api/v1/reportes?desde=2026-10-05T12:00:00Z&hasta=2026-10-06T12:00:00Z&hora=14` ⇒ `200` con la franja local intersectada con la ventana.
  - Con `HORARIO_ZONA=+05:30`: `?dia=2026-10-05` ⇒ ventana UTC `[2026-10-04T18:30Z, 2026-10-05T18:30Z)`; `?hora=14` ⇒ `[08:30, 09:30)Z` de cada día.
  - Con `HORARIO_ZONA=+00:00` cualquier consulta coincide exactamente con el comportamiento de las specs `05`/`06` (regresión).
  - Día sin reportes ⇒ `200` con `reportes: []` y `total: 0`.
- **Casos de Error:** idénticos a los ya definidos (sin variaciones nuevas): `400` por `dia` malformado, `dia` con `desde`/`hasta`, `hora` fuera de `0..=23`, `desde >= hasta`, bbox inválido, `limit`/`offset` fuera de rango, params desconocidos; `500` por infraestructura. Body uniforme `{ "error": "<motivo en español>" }`.
- **Configuración:** `HORARIO_ZONA` inválida no produce respuesta HTTP: solo aviso en stderr del arranque y uso del default.

## 5. Estrategia de Pruebas Obligatoria (Tests)
> **Precondición:** las pruebas HTTP requieren Postgres corriendo (`docker compose up -d db`, `DATABASE_URL`) y siguen el patrón de `routes/tests.rs` (`tower::ServiceExt::oneshot`, `http_body_util::BodyExt`, inserción directa con `creado`/`id_usuario` únicos y limpieza por banda de coordenadas). El helper de app pasa a recibir la zona (`router(pool, canal, horario_zona)`). No se añaden `dev-dependencies`.

- **Pruebas de Camino Feliz:**
  - [ ] `config.rs` unit (sin BD): `HORARIO_ZONA` ausente ⇒ `-04:00`; vacía (`""`, `"   "`) ⇒ `-04:00`; valores válidos `-04:00`, `+00:00`, `+05:30`, `-14:00`, `+14:00` se parsean a sus offsets esperados.
  - [ ] `dia` con zona Caracas: con reportes en `2026-10-05T03:59:59Z` (fuera), `04:00:00Z` (dentro), `2026-10-06T03:59:59Z` (dentro) y `2026-10-06T04:00:00Z` (fuera) ⇒ solo los dos del medio, `total` correcto (borde semiabierta local).
  - [ ] `hora=14` con zona Caracas: reportes en `17:59:59Z` (fuera), `18:00:00Z` (dentro), `18:59:59Z` (dentro), `19:00:00Z` (fuera) ⇒ `total: 2`.
  - [ ] `dia` + `hora` con zona Caracas ⇒ una única franja `18:00–19:00Z` de la jornada local.
  - [ ] App construida con `+05:30`: `hora=14` ⇒ franja `[08:30, 09:30)Z` (`08:29:59Z` fuera, `08:30:00Z` dentro, `09:30:00Z` fuera) y `dia=2026-10-05` ⇒ ventana `[2026-10-04T18:30Z, 2026-10-05T18:30Z)` (cruce de medianoche UTC).
  - [ ] App construida con `+00:00`: `dia` y `hora` reproducen exactamente los valores UTC de las specs `05`/`06` (regresión).
  - [ ] Zona distinta de cero + bbox y `limit`/`offset`: `total` coherente y paginación sin repetir elementos.
  - [ ] Sin `dia` ni `hora`, la respuesta es idéntica a la spec `02` con cualquier zona (el offset no afecta al resto de la consulta).
- **Pruebas de Borde y Errores:**
  - [ ] `config.rs` unit: `HORARIO_ZONA` inválida (`banana`, `UTC`, `America/Caracas`, `-04`, `-0400`, `-4:0`, `+25:00`, `+14:30`, `-15:00`, `+04:60`, `2026-10-05`) ⇒ fallback a `-04:00` sin error ni pánico.
  - [ ] `dia=no-es-fecha&...` con zona configurada ⇒ `400` (validación intacta).
  - [ ] `dia=2026-10-05&desde=...` ⇒ `400` (incompatibilidad intacta).
  - [ ] `hora=24`, `hora=abc`, `hora=` ⇒ `400` (validación intacta).
  - [ ] Param desconocido (`dia=...&foo=1`) ⇒ `400` (`deny_unknown_fields` intacto).

## 6. Pasos Sugeridos para la Implementación (Orden de Ejecución)
1. `src/config.rs`: añadir `HORARIO_ZONA_DEFECTO: &str = "-04:00"`, el campo `horario_zona: FixedOffset` en `Config`, el helper `parsear_zona` (total, con `FixedOffset::east_opt`, sin `unwrap`/`expect`) y su uso en `from_env` con fallback + `eprintln!` de aviso solo si el valor está presente y es inválido. Sin variantes nuevas en `ErrorConfig`. Tests unitarios de la §5 en el `mod tests` existente.
2. `src/modules/reportes/domain/consulta.rs`: añadir el parámetro `horario_zona: FixedOffset` a `construir_filtros`, `resolver_ventana` y `parse_dia` (la ventana del día se resta el offset para convertirse a UTC con `Duration::seconds(horario_zona.local_minus_utc() as i64)`); añadir `horario_zona` a `FiltrosConsultaReporte`; actualizar los doc comments de `dia`/`hora`.
3. `src/modules/reportes/application/consultar_reportes.rs`: añadir `horario_zona: FixedOffset` a `consultar_reportes` y propagarlo a `construir_filtros`.
4. `src/modules/reportes/routes.rs`: añadir `horario_zona` a `EstadoReportes`, al parámetro de `router(...)` y al handler `listar_reportes` (paso a `consultar_reportes`). No tocar `crear_reporte`.
5. `src/modules/reportes/infra/postgres_repositorio.rs`: en `listar_reportes` y `contar_reportes` añadir el binding `$8` (offset en segundos, `i32`, desde `filtros.horario_zona.local_minus_utc()`), cambiar la condición de hora a hora local con `EXTRACT(HOUR FROM ((creado + ($8::int * interval '1 second')) AT TIME ZONE 'UTC'))::int = $7` y renumerar `LIMIT`/`OFFSET` a `$9`/`$10` en `listar_reportes`. Mantener el patrón de bindings estáticos (sin SQL dinámico).
6. `src/main.rs`: pasar `config.horario_zona` a `modules::reportes::routes::router(...)`.
7. `src/modules/reportes/routes/tests.rs`: adaptar el helper de construcción de la app a la firma nueva y añadir los escenarios de la §5 (insertar `creado` en UTC y calcular las ventanas locales esperadas con `FixedOffset`).
8. `.env.example`: añadir `HORARIO_ZONA=-04:00`.
9. Consistencia documental: `specs/GLOSSARY.md` (entrada nueva **horario**; reescribir **día calendario (`dia`)** y **hora del día (`hora`)** para que digan que se interpretan en la zona `HORARIO_ZONA`, por defecto `-04:00` Caracas), `specs/ARCHITECT.md` (línea del filtro `dia` del roadmap: "jornada UTC" ⇒ "jornada en `HORARIO_ZONA`, por defecto Caracas UTC-04:00"; plantilla `.env.example`) y las specs `approved/05_filtro_por_hora.md` y `approved/06_filtro_por_dia.md` (referencias al carácter UTC de `dia`/`hora` ⇒ zona configurable).
10. Ejecutar la Fase de Validación completa y autocorregir hasta el 100%:
    - `cargo fmt --all -- --check`
    - `cargo clippy --all-targets -- -D warnings`
    - `cargo check --all-targets`
    - `cargo test`
