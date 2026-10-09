# Spec: Consultar Reportes (Feed para Heatmap)

## 1. Visión General e Impacto
- **Objetivo:** Exponer el endpoint `GET /api/v1/reportes` para consultar los reportes de corte registrados. Este feed de lectura alimenta el heatmap del cliente (roadmap: "Feed de datos para el heatmap").
- **Contexto:** Extiende el módulo `reportes` existente (spec `approved/01_registrar_reporte`). Se **modifican**: `src/modules/reportes/routes.rs` (añade `get` al router y DTOs de consulta), `src/modules/reportes/application/consultar_reportes.rs` (nuevo caso de uso), `src/modules/reportes/infra/postgres_repositorio.rs` (métodos de listado y conteo) y `src/modules/reportes/domain/` (filtros y errores de consulta). Se **crean**: los submódulos de `application/consultar_reportes` y `domain/consulta` dentro de `reportes`. No hay migración nueva (reutiliza la tabla `reportes`) ni autenticación en esta spec.

### Dependencias autorizadas (esta spec ordena explícitamente tocar `Cargo.toml`)
Ninguna. Queda prohibido añadir librerías o modificar `Cargo.toml` sin orden explícita de una spec.

## 2. Modelos de Datos / Contratos

### Entidades de dominio (`src/modules/reportes/domain/consulta.rs`)
```rust
pub struct Bbox {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lng: f64,
    pub max_lng: f64,
}

pub struct FiltrosConsultaReporte {
    pub desde: chrono::DateTime<Utc>,
    pub hasta: chrono::DateTime<Utc>,
    pub bbox: Option<Bbox>,
    pub limit: u32,
    pub offset: u32,
}
```
- `query.rs` expone funciones puras de validación y el enum de errores `ErrorConsulta` (implementa `std::error::Error` y `Display`), mensajes en español.

### Contratos HTTP
- **Request** `GET /api/v1/reportes` (query params versionados, `deny_unknown_fields`):
  | Parámetro | Tipo | Default | Regla |
  |---|---|---|---|
  | `desde` | RFC 3339 | `now() - 24h` | obligatorio parseable |
  | `hasta` | RFC 3339 | `now()` | obligatorio parseable; `desde < hasta` |
  | `min_lat`, `max_lat`, `min_lng`, `max_lng` | f64 | ausente | todos juntos o ninguno; `min_* ≤ max_*`; rangos: lat [-90,90], lng [-180,180] |
  | `limit` | u32 | `1000` | `1..=5000` |
  | `offset` | u32 | `0` | ≥ 0 |
- **Response éxito** `200 OK`:
```json
{
  "reportes": [
    { "id": "9b0e...", "lat": -33.4489, "lng": -70.6693, "creado": "2026-10-06T14:30:00Z", "horas_duracion": 2 }
  ],
  "total": 123,
  "limit": 1000,
  "offset": 0
}
```
- Los reportes se ordenan por `creado DESC, id DESC` (orden determinista para paginar). `id_usuario` **no** se expone en la respuesta.

## 3. Reglas de Negocio (Criterios de Aceptación)
- [ ] **Regla 1 (Entradas):** todos los query params se validan en el dominio → `400`. Fechas no RFC 3339, `desde >= hasta`, bbox parcial o con rangos invertidos/fuera de rango, `limit` fuera de `1..=5000`, `offset` negativo y params desconocidos son inválidos.
- [ ] **Regla 2 (Ventana temporal):** la consulta aplica SIEMPRE `desde`–`hasta` (con sus defaults) sobre `creado`. No existe consulta sin ventana temporal.
- [ ] **Regla 3 (Bbox):** si el cliente envía el bbox (los 4 params), la consulta filtra por `lat` y `lng` dentro del rectángulo, con criterios de borde inclusivos (`>=`/`<=`). El bbox se implementa con bindings opcionales sobre una única query estática (`$1::float8 IS NULL OR ...`), sin concatenar SQL dinámico por entrada de usuario.
- [ ] **Regla 4 (Paginación):** `limit` y `offset` acotan el resultado; `limit` máximo 5000. `total` es el conteo (COUNT) con los MISMOS filtros (ventana + bbox), sin aplicar `limit`/`offset`.
- [ ] **Regla 5 (Salidas):** éxito ⇒ `200` con `{reportes, total, limit, offset}`. Errores convertidos a respuestas HTTP en un único punto del módulo: validación ⇒ `400`, fallo de persistencia/infra ⇒ `500`. Los handlers no construyen respuestas de error a mano.

## 4. Comportamiento de la Interfaz / API
- **Endpoint:** `GET /api/v1/reportes`
- **Casos de Éxito:** `200 OK`. Body con lista (posiblemente vacía), `total`, `limit` y `offset` devueltos. Ejemplo: `GET /api/v1/reportes?desde=2026-10-05T00:00:00Z&limit=50`.
- **Casos de Error:**
  - `400 Bad Request` — query param desconocido; `desde`/`hasta` no parseables o `desde >= hasta`; bbox incompleto o inválido; `limit`/`offset` fuera de rango.
  - `500 Internal Server Error` — error de infraestructura (conexión/consulta a la BD).
  - Toda respuesta de error usa `application/json` con body `{ "error": "<motivo en español>" }`, p. ej. `{ "error": "desde debe ser anterior a hasta" }`.

## 5. Estrategia de Pruebas Obligatoria
> **Precondición:** los tests requieren Postgres corriendo (`docker compose up -d db`, `DATABASE_URL`). Siguen el patrón existente en `routes/tests.rs`: `tower::ServiceExt::oneshot`, `http-body-util::BodyExt` y limpieza por filas únicas (`id_usuario`/timestamps únicos) sin dependencia del estado previo.

- **Pruebas de Camino Feliz (Happy Path):**
  - [ ] GET sin params devuelve `200`, lista ordenada `creado DESC, id DESC` y `total` ≥ número de reportes insertados en las últimas 24h.
  - [ ] GET con `desde`/`hasta` recorta la ventana y `total` refleja el conteo correcto.
  - [ ] GET con bbox devuelve solo los reportes dentro del rectángulo (límites inclusivos).
  - [ ] GET con `limit`/`offset` pagina correctamente: segunda página (offset=limit) no repite elementos y `total` es constante.
  - [ ] GET sin reportes en la ventana devuelve `200` con `reportes: []` y `total: 0`.
- **Pruebas de Borde y Errores (Edge Cases):**
  - [ ] `desde`/`hasta` no RFC 3339 ⇒ `400` con mensaje de campo.
  - [ ] `desde >= hasta` ⇒ `400`.
  - [ ] Bbox con solo 1 o 2 de los 4 params ⇒ `400`.
  - [ ] Bbox con `min_lat > max_lat` o `min_lng > max_lng` ⇒ `400`.
  - [ ] `limit = 0` y `limit = 5001` ⇒ `400`; `limit = 5000` ⇒ `200`.
  - [ ] `offset` negativo (`offset=-1`) ⇒ `400`.
  - [ ] Query param desconocido (`foo=1`) ⇒ `400`.
  - [ ] Reporte con `creado` fuera de la ventana por defecto (24h) no aparece ⇒ `200` sin él.

## 6. Pasos Sugeridos para la Implementación (Orden de Ejecución)
1. Crear `src/modules/reportes/domain/consulta.rs` con `Bbox`, `FiltrosConsultaReporte`, `ErrorConsulta` (impl `std::error::Error` y `Display`) y validaciones puras; reexportar en `domain/mod.rs`.
2. Ampliar `src/modules/reportes/infra/postgres_repositorio.rs` con `listar_reportes(filtros)` (query estática con bindings opcionales de bbox; `ORDER BY creado DESC, id DESC LIMIT ? OFFSET ?`) y `contar_reportes(filtros)` (COUNT con la misma ventana+bbox).
3. Crear `src/modules/reportes/application/consultar_reportes.rs` (caso de uso `consultar_reportes` que valida filtros en dominio, lista y cuenta; devuelve los reportes más `total`). Sin `unwrap`/`expect`/`panic!`.
4. Modificar `src/modules/reportes/routes.rs`:
   - Añadir `get(listar_reportes)` junto al `post` existente en `/api/v1/reportes`.
   - DTO de query con `deny_unknown_fields`, DTO de ítem de reporte (sin `id_usuario`) y DTO de respuesta `{reportes, total, limit, offset}`.
   - Ampliar `ErrorHttp` con el caso `Consulta(ErrorConsulta)` y su mapeo → `400`; mantener el único punto de conversión de errores.
5. Escribir los tests de la §5 en `routes/tests.rs` (helpers `get_reportes` y `insertar_fila` con `creado` e `id_usuario` únicos).
6. Ejecutar la Fase de Validación completa y autocorregir hasta el 100%:
   - `cargo fmt --all -- --check`
   - `cargo clippy --all-targets -- -D warnings`
   - `cargo check --all-targets`
   - `cargo test`