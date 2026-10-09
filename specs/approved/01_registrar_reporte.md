# Spec: Registrar Reporte de Corte

## 1. Visión General e Impacto
- **Objetivo:** Exponer el endpoint `POST /api/v1/reportes` para que cualquier dispositivo registre de forma anónima un corte de suministro eléctrico (ubicación geográfica y duración aproximada). Este registro alimentará posteriormente el heatmap del cliente.
- **Contexto:** Modifica el arranque existente (`server/src/main.rs`, hoy solo con `GET /api/health`) añadiendo el módulo `reportes` bajo `src/modules/reportes`. Introduce la primera persistencia real del proyecto: PostgreSQL con SQLx (ver §3 y Sección de Dependencias). El feed de lectura queda fuera de esta spec (roadmap). No hay autenticación: el autor se identifica con un `device_id` opaco generado por el dispositivo.

### Dependencias autorizadas (esta spec ordena explícitamente tocar `Cargo.toml`)
`[dependencies]`, a añadir por esta spec y solo estas:
- `sqlx` con features `["postgres", "runtime-tokio-rustls", "migrate", "chrono"]` — conexión a Postgres, ejecución de migraciones y decodificación de `TIMESTAMPTZ` a `chrono::DateTime<Utc>`.
- `chrono` (features `["serde"]`) — tipado de fechas y formato RFC 3339.
- `dotenvy` — carga de variables desde `.env` en el arranque (convención de `ARCHITECT.md` §4).

`[dev-dependencies]`, a añadir por esta spec y solo estas:
- `tower` — útil para `tower::ServiceExt::oneshot` en los tests del Router.
- `http-body-util` — para leer el body de las respuestas en los tests.

Queda prohibido añadir cualquier otra dependencia o modificar el resto de `Cargo.toml`.

## 2. Modelos de Datos / Contratos

### Tabla `reportes` (migración `migrations/0001_crear_reportes.sql`)
```sql
CREATE TABLE reportes (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    id_usuario     TEXT NOT NULL,
    lat            DOUBLE PRECISION NOT NULL,
    lng            DOUBLE PRECISION NOT NULL,
    creado         TIMESTAMPTZ NOT NULL DEFAULT now(),
    horas_duracion SMALLINT NULL,
    CHECK (lat BETWEEN '-90' AND '90'),
    CHECK (lng BETWEEN '-180' AND '180'),
    CHECK (horas_duracion IS NULL OR horas_duracion BETWEEN 0 AND 168)
);

CREATE INDEX idx_reportes_id_usuario ON reportes (id_usuario);
CREATE INDEX idx_reportes_creado        ON reportes (creado);
```
> `gen_random_uuid()` es nativo de PostgreSQL ≥ 13. Se usará `postgres:17` en el contenedor de desarrollo.
> `id` y `creado` los genera el servidor/postgres, nunca el cliente. No se añade crate `uuid`.

### Entidad de dominio
```rust
// src/modules/reportes/domain/reporte.rs
pub struct Reporte {
    pub id: String,                    // uuid generado por la BD
    pub id_usuario: String,            // device_id opaco del dispositivo
    pub lat: f64,
    pub lng: f64,
    pub creado: chrono::DateTime<Utc>, // instante del registro
    pub horas_duracion: Option<u16>,   // NULL = el usuario no lo sabe
}
```

### Contratos HTTP
- **Request** `POST /api/v1/reportes` (Content-Type `application/json`, `deny_unknown_fields`):
```json
{ "id_usuario": "dispositivo-a1b2c3d4", "lat": -33.4489, "lng": -70.6693, "horas_duracion": 2 }
```
- **Response éxito** `201 Created`:
```json
{ "id": "9b0e...", "creado": "2026-10-06T14:30:00Z" }
```

## 3. Reglas de Negocio (Criterios de Aceptación)
- [ ] **Regla 1 (Identidad):** `id_usuario` es obligatorio, cadena de 8 a 64 caracteres `[A-Za-z0-9_-]`. No existe tabla de usuarios en esta spec.
- [ ] **Regla 2 (Geo):** `lat` ∈ [-90, 90] y `lng` ∈ [-180, 180] (validación tanto en el dominio como en `CHECK` de BD).
- [ ] **Regla 3 (Duración):** `horas_duracion` es opcional; si viene, debe ser un entero 0–168. `null` u omisión ⇒ `NULL` en BD.
- [ ] **Regla 4 (Anti-spam / cooldown):** si existe un reporte del mismo `id_usuario` con `|lat - $lat| < 0.001` y `|lng - $lng| < 0.001` (~100 m) registrado en los últimos 15 minutos, la solicitud se rechaza con `429`. Distinta ubicación o distinto `id_usuario` no dispara el cooldown.
- [ ] **Regla 5 (Proceso):** el servidor genera `id` (UUID, vía BD) y `creado` (instante actual, ISO 8601 / RFC 3339). El `creado` insertado es el mismo que se devuelve en la respuesta.
- [ ] **Regla 6 (Salidas):** éxito ⇒ `201` + `{id, creado}`. Los errores se convierten a respuestas HTTP en un único punto del módulo: validación ⇒ `400`, cooldown ⇒ `429`, fallo de persistencia/infra ⇒ `500`. Los handlers no construyen respuestas de error a mano.

## 4. Comportamiento de la Interfaz / API
- **Endpoint:** `POST /api/v1/reportes`
- **Casos de Éxito:** `201 Created` con body `{ "id": "<uuid>", "creado": "<RFC3339>" }`. La fila queda persistida en la tabla `reportes`.
- **Casos de Error:**
  - `400 Bad Request` — body no parseable o campo desconocido; `id_usuario` inválido; `lat`/`lng` fuera de rango; `horas_duracion` fuera de 0–168.
  - `429 Too Many Requests` — cooldown activo (Regla 4).
  - `500 Internal Server Error` — error de infraestructura (conexión/consulta a la BD).
  - Toda respuesta de error usa `application/json`. El body de error de validación identifica el campo y el motivo en español, p. ej. `{ "error": "lat fuera de rango [-90, 90]" }`.

## 5. Estrategia de Pruebas Obligatoria
> **Precondición:** los tests requieren Postgres corriendo. Se levanta con el `docker-compose.yml` de esta spec (`docker compose up -d db`). Los tests usan la misma base (configurable con `DATABASE_URL`) y truncan `reportes` en el setup de cada test para no depender del estado.

- **Pruebas de Camino Feliz (Happy Path):**
  - [ ] POST válido completo devuelve `201` y body con `id` + `creado` RFC 3339; fila persistida verificada por consulta a la BD.
  - [ ] POST con `horas_duracion: null` (y omitido) devuelve `201` y persiste `NULL`.
- **Pruebas de Borde y Errores (Edge Cases):**
  - [ ] `lat = -90`, `lat = 90`, `lng = -180`, `lng = 180` ⇒ `201` (límites inclusivos).
  - [ ] `lat = 90.1` o `lng = -180.1` ⇒ `400` con mensaje de campo.
  - [ ] `id_usuario` de 7 caracteres o con caracteres inválidos ⇒ `400`.
  - [ ] `horas_duracion = 169` ⇒ `400`; `horas_duracion = 168` ⇒ `201`.
  - [ ] Body con campo desconocido (`deny_unknown_fields`) ⇒ `400`.
  - [ ] Body malformado (JSON inválido) o vacío ⇒ `400`.
  - [ ] Segundo POST del mismo `id_usuario` en ±~100 m a los <15 min ⇒ `429`.
  - [ ] Mismo `id_usuario`, nueva ubicación lejana ⇒ `201`.
  - [ ] Otro `id_usuario`, misma ubicación a los pocos segundos ⇒ `201`.

## 6. Pasos Sugeridos para la Implementación (Orden de Ejecución)
1. Añadir a `Cargo.toml` las dependencias autorizadas (§1): `sqlx`, `chrono`, `dotenvy`; dev-deps `tower` y `http-body-util`.
2. Crear `docker-compose.yml` (servicio `db` con imagen `postgres:17`, user `sinluz`, password `sinluz`, db `sinluz`, puerto `5432`), `.env.example` con `DATABASE_URL=postgres://sinluz:sinluz@127.0.0.1:5432/sinluz`.
3. Crear `specs/GLOSSARY.md` con la terminología acordada: `reporte`, `id_usuario` (device_id opaco), `lat`/`lng`, `creado`, `horas_duracion`, `cooldown` — dejando claro el significado de cada término para futuras specs.
4. Crear `src/config` para cargar `.env` (dotenvy) y `DATABASE_URL`.
5. Crear la migración `migrations/0001_crear_reportes.sql` (tabla + índices de §2).
6. Implementar `src/modules/reportes/domain/` (entidad `Reporte`, enum de errores de validación que implementa `std::error::Error` y `Display`, funciones puras de validación). Sin `unwrap`/`expect`/`panic!` en lógica de negocio.
7. Implementar `src/modules/reportes/infra/` (repo con SQLx: `insertar` y consulta de cooldown de la Regla 4). Sin macros `query!`; usar `sqlx::query` con bindings.
8. Implementar `src/modules/reportes/application/` (caso de uso `registrar_reporte` que valida en dominio, comprueba cooldown e inserta; `creado` = `Utc::now()` compartido con la respuesta).
9. Implementar `src/modules/reportes/routes/` (handler `POST /api/v1/reportes`, estado con `PgPool`, DTOs de request/response `Result<_, _>`, y un único punto de conversión de errores ⇒ `400/429/500`).
10. Componer el Router en `main.rs`: migrar en el arranque (`sqlx::migrate!`), crear el pool y montar el submódulo `reportes` junto a `/api/health`.
11. Escribir los tests de la §5 (setup con TRUNCATE; `tower::ServiceExt::oneshot` + `http_body_util::BodyExt`).
12. Ejecutar la Fase de Validación completa y autocorregir hasta el 100%:
    - `cargo fmt --all -- --check`
    - `cargo clippy --all-targets -- -D warnings`
    - `cargo check --all-targets`
    - `cargo test`