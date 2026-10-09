# Spec: Capa CORS Configurable por Entorno

## 1. Visión General e Impacto
- **Objetivo:** Aplicar una capa CORS global al Router de axum para que la SPA (origen distinto al de la API) pueda consumir `/api/v1`. El origen permitido se define por la variable de entorno `CORS_ALLOWED_ORIGIN`, de modo que el mismo binario sirva en desarrollo (`http://localhost:5173`) y producción (dominio real) sin recompilar.
- **Contexto:** No existe hoy ninguna capa CORS en el servidor. Se **crean**: `src/cors.rs` (construcción de la `CorsLayer` y sus tests). Se **modifican**: `src/config.rs` (nuevo campo y parseo del origen), `src/main.rs` (declarar `mod cors;` y aplicar `.layer(...)` al Router) y `.env.example` (documentar la variable). No se toca ningún módulo de dominio (`reportes`, `actividad`); los routers de módulo permanecen sin cambios y la capa se aplica una sola vez, global, en la composición del Router.

### Dependencias autorizadas (esta spec ordena explícitamente tocar `Cargo.toml`)
- **`[dependencies]`:**
  - `tower-http = { version = "0.6", features = ["cors"] }` — provee `CorsLayer`, la implementación estándar de CORS sobre `tower`. Compatible con `axum 0.8.9` (stack HTTP 1.x). Sin la feature `cors` no se puede construir la capa.
- **`[dev-dependencies]`:** ninguna nueva. Se reutilizan `tower` (`ServiceExt::oneshot`) y `http-body-util`, ya presentes en el árbol.
- Queda prohibido añadir cualquier otra librería o modificar el resto de `Cargo.toml` fuera de lo aquí listado.

## 2. Modelos de Datos / Contratos

### Configuración de entorno (`src/config.rs`)
- `Config` incorpora el campo:
  ```rust
  pub struct Config {
      pub database_url: String,
      pub cors_allowed_origin: http::HeaderValue,
  }
  ```
- Origen de la variable `CORS_ALLOWED_ORIGIN`:
  - **Definida y no vacía:** se parsea con `HeaderValue::from_str`. Si no es un valor de cabecera válido ⇒ `ErrorConfig::OrigenInvalido`.
  - **Ausente o vacía (`""`/solo espacios):** fallback a `http://localhost:5173`.
- `ErrorConfig` gana la variante `OrigenInvalido`, con `Display` en español (`"CORS_ALLOWED_ORIGIN no es un origen válido: <valor>"`), e implementa `std::error::Error`.

### Construcción de la capa CORS (`src/cors.rs`)
```rust
pub fn capa(origen: http::HeaderValue) -> tower_http::cors::CorsLayer
```
- `CorsLayer::new()`
  - `.allow_origin(origen)` — origen **exacto** (nunca `Any`).
  - `.allow_methods([Method::GET, Method::POST, Method::OPTIONS])`.
  - `.allow_headers([header::CONTENT_TYPE])`.
  - Sin `allow_credentials` (no se habilitan cookies ni credenciales).

## 3. Reglas de Negocio (Criterios de Aceptación)
- [ ] **Regla 1 (Entradas):** el origen permitido se lee de `CORS_ALLOWED_ORIGIN`. Ausente o vacía ⇒ `http://localhost:5173`. Con valor inválido como cabecera ⇒ error de configuración y el proceso sale con código 1 (mismo tratamiento que `DATABASE_URL`).
- [ ] **Regla 2 (Proceso):** la capa CORS se aplica **una sola vez**, global, sobre el Router final compuesto en `main.rs` (afecta a `/api/health`, a las rutas de `reportes` y al handshake HTTP del WS de `actividad`). Los routers de módulo no añaden CORS.
- [ ] **Regla 3 (Límites):** origen exacto, sin wildcard; métodos limitados a `GET`, `POST`, `OPTIONS`; cabeceras de petición limitadas a `Content-Type`; sin credenciales.
- [ ] **Regla 4 (Salidas):** las peticiones con `Origin` coincidente reciben `Access-Control-Allow-Origin` con ese origen; las peticiones con `Origin` distinto **no** reciben la cabecera. El preflight `OPTIONS` con origen válido responde satisfactoriamente (`200`/`204`) con `Access-Control-Allow-Methods` y `Access-Control-Allow-Headers`. No se introducen errores de dominio nuevos: un origen no permitido no es un error de la aplicación, es un rechazo de navegador (sin cabecera CORS).

## 4. Comportamiento de la Interfaz / API
- **Afecta a:** todas las rutas del Router final (`/api/health`, `/api/v1/reportes`, `/api/v1/actividad/ws`).
- **Casos de Éxito:**
  - `OPTIONS /api/v1/reportes` con `Origin: <origen permitido>` y `Access-Control-Request-Method: POST` ⇒ `200`/`204` + `Access-Control-Allow-Origin`, `Access-Control-Allow-Methods`, `Access-Control-Allow-Headers: content-type`.
  - `GET /api/v1/reportes` con `Origin: <origen permitido>` ⇒ respuesta normal + `Access-Control-Allow-Origin`.
- **Casos de Error / rechazo:**
  - `GET`/`OPTIONS` con `Origin` distinto ⇒ respuesta sin la cabecera `Access-Control-Allow-Origin` (el navegador bloquea la lectura del recurso). El servidor no responde `403` por CORS.
  - `CORS_ALLOWED_ORIGIN` inválida ⇒ el proceso no arranca (error de configuración en `main`).

## 5. Estrategia de Pruebas Obligatoria
> Los tests viven en `src/cors.rs` bajo `#[cfg(test)]`. Montan un `Router::new().route("/", get(...)).layer(cors::capa(origen))` y usan `tower::ServiceExt::oneshot` + `http-body-util`. **No requieren Postgres** (no tocan dominio ni BD).

- **Pruebas de Camino Feliz:**
  - [ ] Preflight `OPTIONS` con `Origin` permitido ⇒ `200`/`204` y cabeceras `Access-Control-Allow-Origin` (+ `Allow-Methods`/`Allow-Headers`).
  - [ ] `GET` con `Origin` permitido ⇒ cabecera `Access-Control-Allow-Origin` presente y con el valor exacto.
- **Pruebas de Borde y Errores:**
  - [ ] `GET` con `Origin` distinto ⇒ sin cabecera `Access-Control-Allow-Origin`.
  - [ ] `CORS_ALLOWED_ORIGIN` ausente ⇒ `Config` usa `http://localhost:5173` (test de parseo, sin variables de entorno reales: función de parseo pura o helper que reciba `Option<&str>`).
  - [ ] `CORS_ALLOWED_ORIGIN=""` (vacía) ⇒ fallback `http://localhost:5173`.
  - [ ] `CORS_ALLOWED_ORIGIN` inválida (p. ej. con `\n`) ⇒ `ErrorConfig::OrigenInvalido`.

## 6. Pasos Sugeridos para la Implementación (Orden de Ejecución)
1. Añadir a `Cargo.toml` la dependencia `tower-http = { version = "0.6", features = ["cors"] }` en `[dependencies]`.
2. Crear `src/cors.rs` con `pub fn capa(origen: http::HeaderValue) -> CorsLayer` y su bloque `#[cfg(test)]`; declarar `mod cors;` en `src/main.rs`.
3. Modificar `src/config.rs`: añadir `cors_allowed_origin`, extraer una función de parseo del origen (con fallback `http://localhost:5173` para ausente/vacía) que devuelva `Result<HeaderValue, ErrorConfig>`, y añadir `ErrorConfig::OrigenInvalido` con `Display`.
4. Modificar `src/main.rs`: aplicar `.layer(cors::capa(config.cors_allowed_origin.clone()))` al Router compuesto. No introducir `unwrap`/`expect`/`panic!` nuevos.
5. Añadir `CORS_ALLOWED_ORIGIN=http://localhost:5173` a `.env.example`.
6. Ejecutar la Fase de Validación completa y autocorregir hasta el 100%:
   - `cargo fmt --all -- --check`
   - `cargo clippy --all-targets -- -D warnings`
   - `cargo check --all-targets`
   - `cargo test`
