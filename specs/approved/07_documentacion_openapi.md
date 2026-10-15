# Spec: Documentación Interactiva de la API (OpenAPI + Scalar)

## 1. Visión General e Impacto
- **Objetivo:** Exponer la documentación de la API REST del backend como un documento OpenAPI 3.1 generado desde el propio código (`utoipa`) y servirlo con la UI interactiva de Scalar. Se publica el documento en `/api/docs/openapi.json` y la UI en `/api/docs`. El montaje queda condicionado por la variable de entorno `DOCS_HABILITADO` para no exponer el contrato de la API en producción.
- **Contexto:** Módulo nuevo `src/docs.rs` (agregado del documento OpenAPI, router de la UI y su handler JSON). Se **modifican** `src/main.rs` (extraer el handler `health` a una función nombrada, montar la doc condicionalmente) y `src/config.rs` (campo y parseo de `DOCS_HABILITADO`). Se **anotan con derivaciones de `utoipa`** los DTOs y handlers existentes de los módulos `reportes` y `actividad` (`ToSchema`, `IntoParams`, `#[utoipa::path]`) sin alterar su comportamiento en tiempo de ejecución. No se toca la lógica de dominio ni la persistencia.

### Dependencias autorizadas (esta spec ordena explícitamente tocar `Cargo.toml`)
`[dependencies]`, a añadir por esta spec y solo estas:
- `utoipa = { version = "6.0", features = ["axum_extras", "chrono"] }` — generación del documento OpenAPI 3.1 vía macros (`ToSchema`, `IntoParams`, `OpenApi`, `#[utoipa::path]`). `axum_extras` resuelve automáticamente `parameter_in` de los extractores `Query<T>` de los handlers; `chrono` documenta `DateTime<Utc>` como `string` con `format: date-time`.
- `utoipa-scalar = { version = "0.4", features = ["axum"] }` — sirve la UI de Scalar (`Scalar::with_url`) sobre axum 0.8. Depende de `utoipa 6`, coherente con la versión anterior.

`[dev-dependencies]`, a añadir por esta spec y solo estas:
- ninguna nueva. Se reutilizan `tower` (`ServiceExt::oneshot`) y `http-body-util`, ya presentes en el árbol.

Queda prohibido añadir cualquier otra dependencia o modificar el resto de `Cargo.toml`.

## 2. Modelos de Datos / Contratos

### Configuración de entorno (`src/config.rs`)
- `Config` incorpora el campo:
  ```rust
  pub struct Config {
      pub database_url: String,
      pub cors_allowed_origin: http::HeaderValue,
      pub docs_habilitado: bool,
  }
  ```
- Parseo de `DOCS_HABILITADO` (función pura `parsear_docs_habilitado(valor: Option<&str>) -> Result<bool, ErrorConfig>`):
  - Ausente o vacía (`""`/solo espacios) ⇒ `false`.
  - `true`/`false` (insensible a mayúsculas) ⇒ el valor booleano.
  - Cualquier otro valor ⇒ `ErrorConfig::DocsInvalido(String)`.
- `ErrorConfig` gana la variante `DocsInvalido(String)`, con `Display` en español (`"DOCS_HABILITADO no es un booleano válido: <valor>"`), e implementa `std::error::Error`.

### Documento OpenAPI (`src/docs.rs`, nuevo)
```rust
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(title = "SinLuz API", version = "0.1.0", description = "API de reportes de cortes de suministro eléctrico."),
    paths(
        crate::health,
        crate::modules::reportes::routes::crear_reporte,
        crate::modules::reportes::routes::listar_reportes,
        crate::modules::actividad::routes::conectar,
    ),
    components(schemas(
        SolicitudCrearReporte,
        RespuestaRegistroReporte,
        SolicitudConsultarReportes,
        ResultadoConsulta,
        ReporteConsulta,
        EventoActividad,
        RespuestaError,
    )),
)]
pub struct ApiDoc;
```

### Esquema de error común (`src/docs.rs`)
```rust
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct RespuestaError {
    pub error: String,
}
```
Todas las respuestas de error de la API (`ErrorHttp`, `ErrorHttpActividad`) comparten este cuerpo; se referencia en las `responses` de cada operación.

### Derivas sobre tipos existentes (sin cambiar su lógica)
- `SolicitudCrearReporte` (`reportes/routes.rs`): `+ ToSchema`.
- `RespuestaRegistroReporte` (`reportes/routes.rs`): `+ ToSchema`.
- `SolicitudConsultarReportes` (`reportes/domain/consulta.rs`): `+ IntoParams`. Los query params numéricos (`limit`, `offset`, `hora`, coordenadas del bbox) se declaran como `Option<String>` en el código; en la doc se sobreescriben con `#[param(value_type = Option<u32>)]` / `Option<u8>` / `Option<f64>` para reflejar su tipo real y sus descripciones del glosario.
- `ResultadoConsulta` (`reportes/application/consultar_reportes.rs`): `+ ToSchema`.
- `ReporteConsulta` (`reportes/domain/consulta.rs`): `+ ToSchema`.
- `EventoActividad` (`actividad/domain/evento.rs`): `+ ToSchema` (mensaje emitido por el canal WS).

## 3. Reglas de Negocio (Criterios de Aceptación)
- [ ] **Regla 1 (Entradas):** `DOCS_HABILITADO` ausente o vacía ⇒ `false`; `true`/`false` (insensible a mayúsculas) ⇒ ese booleano; valor inválido ⇒ error de configuración y el proceso sale con código 1 (mismo tratamiento que `DATABASE_URL` y `CORS_ALLOWED_ORIGIN`).
- [ ] **Regla 2 (Proceso):** el documento OpenAPI se genera en tiempo de compilación a partir de los `#[utoipa::path]` y las derivaciones registradas en `ApiDoc`; no se mantiene un YAML/JSON a mano.
- [ ] **Regla 3 (Montaje condicional):** con `docs_habilitado == true` se montan `GET /api/docs` (UI Scalar) y `GET /api/docs/openapi.json` (documento JSON). Con `false`, el router de documentación **no** se fusiona y ambas rutas responden `404 Not Found` (comportamiento por defecto de axum, sin handler dedicado).
- [ ] **Regla 4 (Cobertura del contrato):** el documento incluye las operaciones `GET /api/health`, `POST /api/v1/reportes`, `GET /api/v1/reportes` y `GET /api/v1/actividad/ws` (handshake WebSocket, descrito textualmente; OpenAPI no modela el protocolo WS, se documenta como upgrade con `101`).
- [ ] **Regla 5 (Salidas):** `GET /api/docs` ⇒ `200 OK` con `Content-Type: text/html`; `GET /api/docs/openapi.json` ⇒ `200 OK` con `Content-Type: application/json` y un documento OpenAPI válido (`openapi` = `3.x`, `info`, `paths`, `components.schemas`). La UI es de solo lectura; no introduce rutas de escritura.

## 4. Comportamiento de la Interfaz / API
- **UI:** `GET /api/docs` ⇒ `200 OK`, `text/html`, página de Scalar que embebe el documento OpenAPI.
- **Documento:** `GET /api/docs/openapi.json` ⇒ `200 OK`, `application/json`, cuerpo generado por `ApiDoc::openapi()` (`#[derive(Serialize)]` de utoipa).
- **Operaciones documentadas:**
  - `GET /api/health` — `200` servicio saludable.
  - `POST /api/v1/reportes` — `201` creado; `400` validación; `429` cooldown; `500` infraestructura.
  - `GET /api/v1/reportes` — `200` con la consulta paginada/agregada; `400` validación; `500` infraestructura.
  - `GET /api/v1/actividad/ws` — handshake WebSocket; `101 Switching Protocols`, `429` límite de conexiones, `500` infraestructura. Descripción explícita de que tras el upgrade el servidor emite eventos `reporte_creado` (`EventoActividad`).
- **Casos de Error:** con `docs_habilitado == false`, `GET /api/docs` y `GET /api/docs/openapi.json` ⇒ `404`. No se define un cuerpo de error propio: es el `404` por defecto de axum.
- **CORS:** la doc queda bajo la capa CORS global existente; al servirse desde el mismo origen que la API no requiere cabeceras adicionales. No se modifica `src/cors.rs`.

## 5. Estrategia de Pruebas Obligatoria
> Los tests de documentación viven en `src/docs.rs` bajo `#[cfg(test)]` y **no requieren Postgres**: montan el router de documentación con `tower::ServiceExt::oneshot` + `http-body-util` y parsean el JSON con `serde_json` (ya en el árbol).

- **Pruebas de Camino Feliz:**
  - [ ] `GET /api/docs` con la doc habilitada ⇒ `200` y `Content-Type: text/html` con contenido de Scalar.
  - [ ] `GET /api/docs/openapi.json` con la doc habilitada ⇒ `200`, `application/json`; el JSON tiene `openapi` que empieza por `3.`, e incluye los paths `/api/health`, `/api/v1/reportes` y `/api/v1/actividad/ws`.
  - [ ] El JSON incluye en `components.schemas` los esquemas `SolicitudCrearReporte`, `ResultadoConsulta` y `EventoActividad`.
- **Pruebas de Borde y Errores:**
  - [ ] Doc deshabilitada (`router_opcional(false)`) ⇒ `GET /api/docs` y `GET /api/docs/openapi.json` devuelven `404`.
  - [ ] `parsear_docs_habilitado(None)` ⇒ `Ok(false)`; `Some("")`/`Some("   ")` ⇒ `Ok(false)`.
  - [ ] `parsear_docs_habilitado(Some("true"))`/`Some("TRUE")` ⇒ `Ok(true)`; `Some("false")` ⇒ `Ok(false)`.
  - [ ] `parsear_docs_habilitado(Some("sí"))` ⇒ `ErrorConfig::DocsInvalido(_)`.

## 6. Pasos Sugeridos para la Implementación (Orden de Ejecución)
1. Añadir a `Cargo.toml` las dependencias autorizadas (§1): `utoipa` (features `axum_extras`, `chrono`) y `utoipa-scalar` (feature `axum`).
2. Modificar `src/config.rs`: añadir `docs_habilitado`, la función pura `parsear_docs_habilitado` y la variante `ErrorConfig::DocsInvalido` con `Display`; tests de parseo de la §5.
3. Anotar los tipos existentes con `ToSchema`/`IntoParams` y los handlers con `#[utoipa::path]` (con `request_body`, `params` y `responses` explícitos; usar `value_type` para los query numéricos). No modificar la lógica de dominio ni los códigos HTTP.
4. Extraer en `src/main.rs` el handler inline `health` a `pub(crate) async fn health() -> StatusCode` con su `#[utoipa::path]`, y enrutar `/api/health` con `get(health)`.
5. Crear `src/docs.rs`: `ApiDoc` (`#[derive(OpenApi)]`), `RespuestaError`, `fn router() -> Router` (merge de `Scalar::with_url("/api/docs", ApiDoc::openapi())` y la ruta `GET /api/docs/openapi.json` que devuelve `Json(ApiDoc::openapi())`), y `fn router_opcional(habilitado: bool) -> Router`.
6. Modificar `src/main.rs`: declarar `mod docs;` y fusionar `docs::router_opcional(config.docs_habilitado)` antes de aplicar la capa CORS global. No introducir `unwrap`/`expect`/`panic!` nuevos.
7. Añadir `DOCS_HABILITADO=false` a `.env.example`.
8. Escribir los tests de la §5 en `src/docs.rs` (no requieren Postgres).
9. Actualizar `server/specs/GLOSSARY.md` con los términos «documentación de la API» y «docs habilitado».
10. Ejecutar la Fase de Validación completa y autocorregir hasta el 100%:
    - `cargo fmt --all -- --check`
    - `cargo clippy --all-targets -- -D warnings`
    - `cargo check --all-targets`
    - `cargo test`
