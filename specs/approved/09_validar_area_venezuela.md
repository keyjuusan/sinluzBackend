# Spec: Validar que el Reporte Está Dentro del Área de Venezuela

## 1. Visión General e Impacto
- **Objetivo:** Rechazar con `400` todo reporte de corte cuya ubicación (`lat`, `lng`) quede fuera del territorio venezolano, evitando que datos de otros países o del mar contaminen el heatmap.
- **Contexto:** Extiende el caso de uso de registro existente de la spec `approved/01_registrar_reporte`. La validación se ejecuta **solo** en `POST /api/v1/reportes`, después de validar rangos globales e `id_usuario` y antes del cooldown y de la persistencia. No cambia el feed `GET /api/v1/reportes`, ni el cooldown, ni la tabla `reportes`, ni la configuración del servidor. No hay autenticación implicada.
  - Se **crean**: `src/modules/reportes/domain/ubicacion.rs` (nueva geocerca) y `server/README.md` (atribución de datos).
  - Se **modifican**: `Cargo.toml`, `src/modules/reportes/domain/mod.rs`, `src/modules/reportes/domain/reporte.rs`, `src/modules/reportes/application/registrar_reporte.rs`, `src/modules/reportes/routes.rs`, `src/main.rs`, `src/modules/reportes/routes/tests.rs`, `src/modules/actividad/routes/tests.rs`, `specs/GLOSSARY.md` y `specs/ARCHITECT.md`.
  - **No** se crea migración nueva ni se añaden variables de entorno.

### Dependencias autorizadas (esta spec ordena explícitamente tocar `Cargo.toml`)
`[dependencies]`, a añadir por esta spec y solo esta:
- `country-boundaries = "1.2.0"` — geocodificador invertido offline con dataset embebido de fronteras (raster `360x180`) derivado de OpenStreetMap. Resuelve la pertenencia de un punto a un país con una sola llamada (`is_in(..., "VE")`), sin dependencias transitivas ni mantenimiento de polígonos propios. Se elige frente a `geo` o a un polígono hecho a mano porque reduce la implementación al mínimo y ya incluye territorio insular (Margarita, Los Roques).
  - Licencia del crate: Apache-2.0. Licencia de los datos: ODbL, © OpenStreetMap contributors; **exige atribución visible** (ver §6).

`[dev-dependencies]`: ninguna nueva. No se autoriza ninguna otra dependencia ni modificar el resto de `Cargo.toml`.

## 2. Modelos de Datos / Contratos

### Geocerca de dominio (`src/modules/reportes/domain/ubicacion.rs`)
```rust
use country_boundaries::{BOUNDARIES_ODBL_360X180, CountryBoundaries, LatLon, ReadError};

pub const ID_PAIS_VENEZUELA: &str = "VE";

pub struct GeocercaVenezuela {
    limites: CountryBoundaries,
}

impl GeocercaVenezuela {
    /// Carga el dataset embebido una única vez (se invoca en el arranque).
    pub fn desde_datos_embebidos() -> Result<Self, ReadError>;

    /// `Ok(())` si el punto está dentro de Venezuela; error tipado en caso contrario.
    pub fn validar(&self, lat: f64, lng: f64) -> Result<(), ErrorValidacion>;
}
```
- `validar` construye `LatLon::new(lat, lng)`; si falla (valores no finitos) devuelve `ErrorValidacion::CoordenadaInvalida`. En caso contrario consulta `is_in(posicion, ID_PAIS_VENEZUELA)`: dentro ⇒ `Ok(())`, fuera ⇒ `ErrorValidacion::UbicacionFueraDeVenezuela`.
- Sin `unwrap`/`expect`/`panic!` en el dominio. La construcción se hace una sola vez y se comparte como `Arc<GeocercaVenezuela>` (el raster no se clona por petición).

### Errores y validación (`src/modules/reportes/domain/reporte.rs`)
- `ErrorValidacion` gana dos variantes:
  - `CoordenadaInvalida` → `Display`: `"coordenadas lat/lng inválidas"`.
  - `UbicacionFueraDeVenezuela` → `Display`: `"ubicación fuera de Venezuela: el reporte debe registrarse dentro del territorio venezolano"`.
- `validar_datos` pasa a recibir la geocerca:
```rust
pub fn validar_datos(
    geocerca: &GeocercaVenezuela,
    id_usuario: &str,
    lat: f64,
    lng: f64,
    horas_duracion: Option<u16>,
) -> Result<(), ErrorValidacion>;
```
- Orden de validación **invariable**: `id_usuario` → rango global de `lat`/`lng` (spec `01`) → `horas_duracion` → pertenencia a Venezuela. Un punto con `lat = 90.1` sigue devolviendo `"lat fuera de rango"`, no el error de área.

### Aplicación y composición
- `registrar_reporte(repositorio, geocerca, id_usuario, lat, lng, horas_duracion)`; propaga `geocerca` a `validar_datos`. El cooldown no se consulta si la ubicación es inválida.
- `EstadoReportes` gana `geocerca: Arc<GeocercaVenezuela>`; `router(pool, canal, horario_zona, geocerca: Arc<GeocercaVenezuela>)`.
- `main.rs` carga la geocerca antes de componer el router; si falla, escribe el error en `stderr` y termina con código 1 (fallo de arranque, no de petición).

### Persistencia y contratos HTTP
- **Sin migración**: la tabla `reportes` y sus `CHECK` de rango global no cambian. La verificación geográfica es de dominio, no de BD.
- **Request** `POST /api/v1/reportes`: mismos campos y tipos de la spec `01` (`id_usuario`, `lat`, `lng`, `horas_duracion`). No hay campos nuevos.
- **Response éxito** `201 Created`: mismo body `{ "id": "<uuid>", "creado": "<RFC3339>" }`.
- **Response error de área** `400 Bad Request`:
```json
{ "error": "ubicación fuera de Venezuela: el reporte debe registrarse dentro del territorio venezolano" }
```

## 3. Reglas de Negocio (Criterios de Aceptación)
- [ ] **Regla 1 (Entradas):** se mantienen las validaciones de la spec `01`: `id_usuario` 8–64 `[A-Za-z0-9_-]`, `lat` ∈ [-90, 90], `lng` ∈ [-180, 180], `horas_duracion` opcional 0–168. Los límites globales siguen siendo inclusivos y se comprueban antes que el área.
- [ ] **Regla 2 (Proceso — pertenencia):** un reporte es válido solo si `(lat, lng)` está dentro del territorio de Venezuela según el dataset embebido de `country-boundaries` con id ISO `VE`. Se incluye territorio insular poblado (p. ej. Margarita, Los Roques) porque el dataset lo contiene.
- [ ] **Regla 3 (Límites):** la validación aplica **solo al registro** (`POST`). El feed (`GET /api/v1/reportes`) sigue devolviendo cualquier fila persistida, incluidos reportes históricos fuera del área. No hay tolerancia configurable, ni variable de entorno, ni parámetro de API; el contorno es fijo y se actualiza solo al actualizar la dependencia.
- [ ] **Regla 4 (Semántica del dataset):** el dataset distingue tierra de mar y solo resuelve correctamente puntos en tierra/islas. Un punto en el mar se rechaza como fuera de Venezuela. El contorno está simplificado para uso de mapas; no se garantiza precisión catastral en la línea fronteriza.
- [ ] **Regla 5 (Proceso — orden e idempotencia):** el área se valida después de `id_usuario`, rangos y duración, y antes del cooldown y del `INSERT`. Un reporte fuera de área **no se persiste** y **no emite** el evento `reporte_creado`. Reintentar la misma petición inválida devuelve siempre `400` (no hay consumo de cooldown).
- [ ] **Regla 6 (Salidas):** ubicación fuera de área ⇒ `400` con mensaje en español. Los demás códigos no cambian: `201` éxito, `400` validación/body/query, `429` cooldown, `500` infraestructura. La conversión a HTTP sigue centralizada en `ErrorHttp` (spec `01`); los handlers no construyen respuestas de error.

## 4. Comportamiento de la Interfaz / API
- **Endpoint:** `POST /api/v1/reportes` (sin cambios de ruta, verbo ni campos).
- **Casos de Éxito:** coordenada venezolana y demás campos válidos ⇒ `201 Created` con `{id, creado}`; fila persistida y evento de actividad publicado.
  - Ejemplos aceptados: Caracas `10.4806, -66.9036`; Maracaibo `10.6545, -71.6405`; Porlamar `10.9577, -63.8697`.
- **Casos de Error:**
  - `400 Bad Request` — coordenada fuera de Venezuela, p. ej. Santiago de Chile `-33.4489, -70.6693` o Bogotá `4.7110, -74.0721`; body `{ "error": "ubicación fuera de Venezuela: ..." }`. No se persiste nada.
  - `400 Bad Request` — se mantienen los casos previos: `id_usuario` inválido, `lat`/`lng` fuera de rango global, `horas_duracion` fuera de 0–168, body inválido o con campos desconocidos.
  - `429 Too Many Requests` — cooldown activo (solo puede ocurrir con coordenadas venezolanas válidas).
  - `500 Internal Server Error` — fallo de infraestructura.
- **Documentación:** actualizar el doc comment y la descripción `400` de `crear_reporte` en `routes.rs` para mencionar que la ubicación debe estar dentro de Venezuela.

## 5. Estrategia de Pruebas Obligatoria (Tests)
> **Precondición:** las pruebas HTTP requieren Postgres corriendo (`docker compose up -d db`, `DATABASE_URL`) y siguen el patrón existente (`tower::ServiceExt::oneshot`, `http_body_util::BodyExt`). El helper `aplicacion` de `reportes/routes/tests.rs` y el helper `app_reportes_y_actividad` de `actividad/routes/tests.rs` deben construirse con una geocerca cargada **una sola vez** (por ejemplo `OnceLock<Arc<GeocercaVenezuela>>`) para no reparsear el dataset en cada test. No se añaden `dev-dependencies`.

- **Pruebas de Camino Feliz:**
  - [ ] `GeocercaVenezuela::desde_datos_embebidos()` devuelve `Ok` y el dataset resuelve `VE`.
  - [ ] Unit `GeocercaVenezuela::validar`: `Ok(())` para Caracas, Maracaibo y Porlamar (isla); `Err(UbicacionFueraDeVenezuela)` para Santiago de Chile, Bogotá y `(0.0, 0.0)`.
  - [ ] POST válido en Caracas ⇒ `201`, fila persistida y evento de actividad (test existente actualizado).
  - [ ] POST válido en Maracaibo y en Porlamar ⇒ `201` (cobertura de territorio insular).
  - [ ] Reportes fuera del área insertados **directamente en BD** siguen apareciendo en `GET /api/v1/reportes` ⇒ `200` (el feed no valida área).

- **Pruebas de Borde y Errores:**
  - [ ] Unit `validar_latitud`/`validar_longitud`: `-90`, `90`, `-180`, `180` son válidos (límites inclusivos de la spec `01`); `90.1`, `-90.1`, `180.1`, `-180.1` no. Este unit test **sustituye** el test HTTP `acepta_coordenadas_en_limites`, que ya no puede esperar `201` para los polos.
  - [ ] POST en Santiago de Chile `-33.4489, -70.6693` ⇒ `400` con `"fuera de Venezuela"`; verificar con `id_usuario` único que **no** quedó fila persistida.
  - [ ] POST en Bogotá `4.7110, -74.0721` ⇒ `400` con `"fuera de Venezuela"`.
  - [ ] POST con `lat = 90.0, lng = 0.0` ⇒ `400` de área (rango global válido pero fuera de Venezuela); con `lat = 90.1` ⇒ `400` `"lat fuera de rango"` (el rango precede al área).
  - [ ] `GeocercaVenezuela::validar` con `NaN`/`INFINITY` ⇒ `Err(CoordenadaInvalida)` sin pánico.
  - [ ] `id_usuario` inválido + coordenada fuera de área ⇒ `400` de `id_usuario` (orden de validación intacto).
  - [ ] `horas_duracion = 169` + coordenada fuera de área ⇒ `400` de duración (orden intacto).
  - [ ] Coordenada venezolana válida repetida por el mismo dispositivo dentro de 15 min ⇒ segundo POST `429` (cooldown intacto).
  - [ ] Un reporte rechazado por área **no** emite evento de actividad (suscriptor WS en silencio).

- **Actualización obligatoria de tests existentes** (hoy usan coordenadas de Chile y fallarían con la nueva regla):
  - `reportes/routes/tests.rs`: sustituir las coordenadas de `registra_reporte_valido`, `acepta_duracion_nula_y_omitida`, `rechaza_duracion_fuera_de_rango` (caso válido `168`), `aplica_cooldown_misma_ubicacion`, `permite_otra_ubicacion_mismo_dispositivo` y `permite_otro_dispositivo_misma_ubicacion` por coordenadas venezolanas que preserven la intención de cada test (misma ubicación / ubicaciones lejanas). Los tests de `GET` que insertan filas directamente no cambian.
  - `actividad/routes/tests.rs`: sustituir las coordenadas de `un_post_valido_emite_el_evento_al_suscriptor`, `dos_suscriptores_reciben_el_mismo_evento`, `el_post_rechazado_no_emite_evento` y `el_mensaje_de_texto_del_cliente_no_cierra_ni_se_retransmite`; actualizar las aserciones de `lat`/`lng` del evento al nuevo punto. Mantener el caso `lat = 91` como inválido.

## 6. Pasos Sugeridos para la Implementación (Orden de Ejecución)
1. `Cargo.toml`: añadir únicamente `country-boundaries = "1.2.0"` en `[dependencies]`; actualizar `Cargo.lock` con cargo (no a mano).
2. Crear `src/modules/reportes/domain/ubicacion.rs` con `ID_PAIS_VENEZUELA`, `GeocercaVenezuela` (`desde_datos_embebidos` con `BOUNDARIES_ODBL_360X180` y `validar` sin `unwrap`/`expect`/`panic!`) y sus unit tests; reexportar en `domain/mod.rs`.
3. Modificar `src/modules/reportes/domain/reporte.rs`: añadir `CoordenadaInvalida` y `UbicacionFueraDeVenezuela` a `ErrorValidacion` con `Display` en español; añadir el parámetro `&GeocercaVenezuela` a `validar_datos` y validar el área al final del orden establecido.
4. Modificar `src/modules/reportes/application/registrar_reporte.rs`: nuevo parámetro `geocerca: &GeocercaVenezuela` propagado a `validar_datos` (sin cambios en el manejo de `ErrorRegistro`; el nuevo error entra por `ErrorRegistro::Validacion`).
5. Modificar `src/modules/reportes/routes.rs`: `EstadoReportes.geocerca: Arc<GeocercaVenezuela>`, parámetro nuevo en `router(...)`, paso de `estado.geocerca.as_ref()` a `registrar_reporte`, y doc comment/`400` de `crear_reporte` actualizados. El mapeo a HTTP no cambia.
6. Modificar `src/main.rs`: cargar `GeocercaVenezuela::desde_datos_embebidos()` antes de componer el router; en error, `eprintln!` y `std::process::exit(1)` (patrón de arranque existente); pasarla como `Arc`.
7. Actualizar los tests según §5: helper de geocerca compartida, coordenadas venezolanas en todos los POST de `reportes/routes/tests.rs` y `actividad/routes/tests.rs`, sustitución de `acepta_coordenadas_en_limites` por unit tests de rango, y escenarios nuevos de área/orden/evento.
8. Crear `server/README.md` con la atribución obligatoria: datos de fronteras derivados de OpenStreetMap, © OpenStreetMap contributors, bajo ODbL, distribuidos por el crate `country-boundaries` (Apache-2.0).
9. Consistencia documental: añadir a `specs/GLOSSARY.md` el término **geocerca de Venezuela**; registrar la funcionalidad como pendiente en `specs/ARCHITECT.md` §5; marcar en `specs/ARCHITECT.md` §2 la dependencia `country-boundaries` como autorizada por esta spec.
10. Ejecutar la Fase de Validación completa y autocorregir hasta el 100%:
    - `cargo fmt --all -- --check`
    - `cargo clippy --all-targets -- -D warnings`
    - `cargo check --all-targets`
    - `cargo test`
