# Spec: Filtro del Feed por Hora del Día

> **Actualización (spec `08_horario_zona_configurable`):** la franja horaria de `hora` ya no se interpreta en UTC fijo, sino en la **zona horaria configurada** por la variable de entorno `HORARIO_ZONA` (offset fijo `±HH:MM`, por defecto `-04:00`, Caracas). Con `HORARIO_ZONA=+00:00` el comportamiento es idéntico al descrito originalmente en UTC. Las referencias a UTC de este documento se leen como "hora local de `HORARIO_ZONA`".

## 1. Visión General e Impacto
- **Objetivo:** Añadir al feed de lectura un **filtro por hora del día** mediante el query param `hora` (entero `0..=23`, en la zona horaria configurada `HORARIO_ZONA`, por defecto Caracas UTC-04:00), que acota la consulta a los reportes registrados **entre esa hora y la siguiente**: la franja semiabierta `[hora:00:00, hora+1:00:00)` local de cada día incluido en la ventana consultada. Sirve para responder "lo que pasó a las 14:00" sin construir ventanas RFC 3339 a mano.
- **Contexto:** Extiende el módulo `reportes` existente (specs `approved/01_registrar_reporte` y `approved/02_consultar_reportes`). Es **aditivo y opt-in**: no crea endpoint nuevo, **no añade ni modifica campos de la respuesta** (el body sigue siendo exactamente el de la spec `02`) y solo filtra. Respeta los mismos filtros que el feed (ventana temporal + bbox) y, al ser una condición de filtrado más, también afecta a `total` y a la paginación `limit`/`offset`.
- **Reemplaza a la agregación por intervalos** que esta spec describía en su versión anterior. Se **eliminan**: el query param `intervalo_horas`, los campos de respuesta `intervalo_horas` e `intervalos`, la entidad `IntervaloReportes`, sus constantes (`INTERVALO_HORAS_DEFECTO`, `INTERVALO_HORAS_MAXIMO`, `MAX_INTERVALOS`), la variante `IntervaloInvalido` y el método de conteo agrupado. **Cambio de compatibilidad:** una petición con `intervalo_horas=...` ahora responde `400` (param desconocido, `deny_unknown_fields`).
  - Se **modifican**: `src/modules/reportes/domain/consulta.rs` (param `hora` y su validación; eliminación de la agregación), `src/modules/reportes/domain/mod.rs` (eliminar re-export de `IntervaloReportes`), `src/modules/reportes/application/consultar_reportes.rs` (eliminar `intervalo_horas`/`intervalos` de `ResultadoConsulta` y `construir_serie_intervalos`), `src/modules/reportes/infra/postgres_repositorio.rs` (eliminar `contar_reportes_por_intervalo`; añadir la condición de hora a `listar_reportes` y `contar_reportes`), `src/modules/reportes/routes.rs` (solo comentario doc) y `src/docs.rs` (quitar `IntervaloReportes` de los componentes OpenAPI de la spec `07`). Se **modifican** también los tests en `src/modules/reportes/routes/tests.rs`.
  - **Consistencia documental** (misma ejecución): `specs/GLOSSARY.md` (sustituir las entradas `intervalo de horas`, `agregación por intervalos` y `segmento` por la entrada `hora del día`), `specs/ARCHITECT.md` (referencia a `intervalo_horas` en la línea del filtro `dia`), `specs/approved/06_filtro_por_dia.md` y `specs/approved/07_documentacion_openapi.md` (referencias a `intervalo_horas`/`IntervaloReportes`).
  - No se **crean** módulos nuevos, no hay migración (reutiliza la tabla `reportes`) ni autenticación en esta spec.

### Dependencias autorizadas (esta spec ordena explícitamente tocar `Cargo.toml`)
Ninguna. Queda prohibido añadir librerías o modificar `Cargo.toml`. El filtrado se resuelve con un binding opcional más en las queries estáticas de SQLx ya presentes.

## 2. Modelos de Datos / Contratos

### Dominio (`src/modules/reportes/domain/consulta.rs`)
```rust
pub const HORA_MIN: u8 = 0;
pub const HORA_MAX: u8 = 23;
```
- `SolicitudConsultarReportes` **reemplaza** `intervalo_horas: Option<String>` por `pub hora: Option<String>` (el resto de campos no cambia).
- `FiltrosConsultaReporte` **reemplaza** `intervalo_horas: Option<u32>` por `pub hora: Option<u8>` (`None` = filtro desactivado).
- `ErrorValidacionConsulta` **sustituye** la variante `IntervaloInvalido` por `HoraInvalida` con `Display` en español: `"hora inválida: debe estar entre 0 y 23"`.
- Se **eliminan**: `IntervaloReportes`, las constantes `INTERVALO_HORAS_DEFECTO`/`INTERVALO_HORAS_MAXIMO`/`MAX_INTERVALOS` y los helpers `parse_intervalo_horas`/`segmentos_de_intervalo`.

> Extensibilidad (filtros futuros): el punto único de extensión sigue siendo `SolicitudConsultarReportes` + `FiltrosConsultaReporte` + `construir_filtros`. Un filtro nuevo se añade como campo opcional en ambos structs, su validación pura en `construir_filtros` y (si aplica) un binding opcional más en las queries estáticas. No se toca el handler ni la composición del Router. La spec `06_filtro_por_dia` ya reutilizó este punto.

### Contratos HTTP
- **Request** `GET /api/v1/reportes` (query params ya existentes sin cambios; se añade y se retira):
  | Parámetro | Tipo | Default | Regla |
  |---|---|---|---|
  | `hora` | u8 (`0..=23`) | ausente ⇒ sin filtro | opt-in; entero en `0..=23` interpretado en la zona `HORARIO_ZONA` (por defecto Caracas UTC-04:00; `+00:00` ⇒ UTC); cualquier valor no numérico, vacío, negativo, decimal o `>23` ⇒ `400` |
  | `intervalo_horas` | — | — | **eliminado** (residuo de la agregación): su uso ⇒ `400` por param desconocido |
- **Response éxito** `200 OK`: **idéntica** a la de la spec `02` (sin campos nuevos; `hora` no se refleja en el body, igual que `dia`):
```json
{
  "reportes": [
    { "id": "9b0e...", "lat": -33.4489, "lng": -70.6693, "creado": "2026-10-06T14:30:00Z", "horas_duracion": 2 }
  ],
  "total": 1,
  "limit": 1000,
  "offset": 0
}
```
- `id_usuario` **no** se expone en la respuesta.

## 3. Reglas de Negocio (Criterios de Aceptación)
- [ ] **Regla 1 (Entradas):** `hora` ausente ⇒ comportamiento idéntico a la spec `02`. Presente ⇒ debe ser un entero en `HORA_MIN..=HORA_MAX` (`0..=23`); cualquier otro valor (`hora=` vacío, `abc`, `-1`, `14.5`, `24`) ⇒ `400` con `ErrorValidacionConsulta::HoraInvalida`.
- [ ] **Regla 2 (Proceso):** el filtro conserva los reportes cuyo `creado` cumple simultáneamente: (a) pertenece a la ventana `desde <= creado < hasta`, (b) cae en la franja `[hora:00:00, hora+1:00:00)` **local** (zona `HORARIO_ZONA`) **de algún día**, y (c) cumple el bbox si está presente. Es decir, intersecta la ventana con la repetición diaria de la franja horaria. `total` es el conteo de ese conjunto y `limit`/`offset` paginan sobre él (a diferencia de la agregación eliminada, que ignoraba la paginación).
- [ ] **Regla 3 (Bordes de la franja):** la franja es **semiabierta** y se interpreta en la **zona `HORARIO_ZONA`** (por defecto UTC-04:00; `+00:00` reproduce el comportamiento UTC original): un reporte con `creado` a la `hora:00:00.000` local **entra**; uno a la `hora+1:00:00.000` local **no entra** (a menos que pertenezca a otra hora filtrada). Con `hora=23` la franja es `[23:00, 00:00)` (hasta el cambio de día); con `hora=0` es `[00:00, 01:00)`.
- [ ] **Regla 4 (Composición):** `hora` es combinable con `desde`/`hasta`, `dia` (la ventana resuelve a la jornada ⇒ una única franja dentro de ese día), bbox y `limit`/`offset`. Con una ventana multi-día entra una franja por cada día intersectada con la ventana: p. ej. ventana `2026-10-05T14:30Z .. 2026-10-06T14:30Z` con `hora=14` ⇒ entran los reportes de `05T14:30–15:00` y de `06T14:00–14:30`.
- [ ] **Regla 5 (Salidas):** éxito ⇒ `200` con `{reportes, total, limit, offset}` (body exacto de la `02`, sin claves nuevas). Errores convertidos a respuestas HTTP en el único punto del módulo: validación ⇒ `400`, fallo de persistencia/infra ⇒ `500`. Los handlers no construyen respuestas a mano.

## 4. Comportamiento de la Interfaz / API
- **Endpoint:** `GET /api/v1/reportes` (sin cambios de ruta ni de verbo).
- **Casos de Éxito:**
  - `GET /api/v1/reportes` (sin `hora`) ⇒ `200` con el body actual (regresión de la spec `02`).
  - `GET /api/v1/reportes?desde=...&hasta=...&hora=14` ⇒ `200` solo con los reportes de `14:00–15:00Z` de los días de la ventana y `total` coherente.
  - `GET /api/v1/reportes?dia=2026-10-05&hora=14` ⇒ `200` con los reportes de `14:00–15:00Z` de ese día.
  - `GET /api/v1/reportes?hora=14&min_lat=...&max_lat=...&min_lng=...&max_lng=...` ⇒ intersección franja ∩ bbox.
  - `GET /api/v1/reportes?hora=14&limit=2&offset=2` ⇒ `200`: `limit`/`offset` paginan sobre el conjunto filtrado, `total` sin paginar.
  - `GET /api/v1/reportes?hora=0` ⇒ franja `[00:00, 01:00)` local (medianoche) de la zona `HORARIO_ZONA`.
  - Ventana válida sin reportes en la franja ⇒ `200` con `reportes: []` y `total: 0`.
- **Casos de Error:**
  - `400 Bad Request` — `hora` no numérica, vacía, negativa, decimal o `>23` (`0` y `23` son válidos); `intervalo_horas=...` (param eliminado); (y los ya existentes de las specs `02`/`06`: fechas inválidas, `desde >= hasta`, bbox inválido, `limit`/`offset` fuera de rango, `dia` incompatible con `desde`/`hasta`, params desconocidos).
  - `500 Internal Server Error` — error de infraestructura (conexión/consulta a la BD).
  - Toda respuesta de error usa `application/json` con body `{ "error": "<motivo en español>" }`, p. ej. `{ "error": "hora inválida: debe estar entre 0 y 23" }`.

## 5. Estrategia de Pruebas Obligatoria
> **Precondición:** los tests requieren Postgres corriendo (`docker compose up -d db`, `DATABASE_URL`) y siguen el patrón de `routes/tests.rs`: `tower::ServiceExt::oneshot`, `http-body-util::BodyExt`, inserción directa con `creado`/`id_usuario` únicos y limpieza por banda de coordenadas. No se añaden `dev-dependencies`.

- **Pruebas de Camino Feliz:**
  - [ ] `hora=14` con reportes insertados en `13:59:59`, `14:00:00`, `14:59:59` y `15:00:00Z` ⇒ devuelve solo los de `14:00:00` y `14:59:59`, con `total: 2`.
  - [ ] Borde inclusivo/exclusivo (Regla 3): `creado` exactamente en `hora:00:00.000Z` entra; exactamente en `hora+1:00:00.000Z` no entra.
  - [ ] Ventana de dos días con reportes en la franja de cada día ⇒ devuelve los de ambos días y `total` es la suma.
  - [ ] Ventana multi-día parcial (ej. `..T14:30Z .. ..T14:30Z` con `hora=14`) ⇒ solo las fracciones `14:30–15:00` y `14:00–14:30` (Regla 4).
  - [ ] `dia=...&hora=...` ⇒ una única franja de ese día; combinado con bbox aplica ambos.
  - [ ] Con bbox los resultados cumplen a la vez la franja y el rectángulo.
  - [ ] `hora=23` ⇒ franja `[23:00, 00:00)` correcta (reporte `23:30Z` entra, `00:30Z` del día siguiente no).
  - [ ] `hora=0` ⇒ franja `[00:00, 01:00)` correcta (reporte `00:30Z` entra, `23:30Z` del día anterior no).
  - [ ] `hora=...&limit=2&offset=2` ⇒ pagina sobre el conjunto filtrado sin repetir elementos y con `total` constante.
  - [ ] Ventana válida sin reportes en la franja ⇒ `200` con `reportes: []` y `total: 0`.
  - [ ] Sin `hora` la respuesta es idéntica a la de la spec `02` (regresión).
- **Pruebas de Borde y Errores:**
  - [ ] `hora=24` ⇒ `400` con `hora` en el body.
  - [ ] `hora=-1` y `hora=abc` ⇒ `400`.
  - [ ] `hora=14.5` (decimal) ⇒ `400`.
  - [ ] `hora=` (vacío) ⇒ `400`.
  - [ ] `hora=14&foo=1` (param desconocido) ⇒ `400` (se mantiene `deny_unknown_fields`).
  - [ ] `intervalo_horas=1` (param eliminado) ⇒ `400` con `intervalo_horas` en el body.

## 6. Pasos Sugeridos para la Implementación (Orden de Ejecución)
1. `src/modules/reportes/domain/consulta.rs`: añadir `HORA_MIN`/`HORA_MAX`; reemplazar el campo `intervalo_horas` por `hora: Option<String>` en `SolicitudConsultarReportes` y por `hora: Option<u8>` en `FiltrosConsultaReporte`; reemplazar `IntervaloInvalido` por `HoraInvalida` (Display `"hora inválida: debe estar entre 0 y 23"`); eliminar `IntervaloReportes`, las constantes de intervalos y los helpers `parse_intervalo_horas`/`segmentos_de_intervalo`; en `construir_filtros` validar `hora` (ausente ⇒ `None`; entero fuera de `0..=23` o no numérico ⇒ `HoraInvalida`). No introducir `unwrap`/`expect`/`panic!`.
2. `src/modules/reportes/domain/mod.rs`: eliminar el re-export de `IntervaloReportes`.
3. `src/modules/reportes/infra/postgres_repositorio.rs`: eliminar `contar_reportes_por_intervalo`; en `listar_reportes` y `contar_reportes` añadir el binding opcional `$7::int` con la condición `AND ($7::int IS NULL OR EXTRACT(HOUR FROM ((creado + ($8::int * interval '1 second')) AT TIME ZONE 'UTC'))::int = $7)` (mismo patrón estático que el bbox, sin SQL dinámico por entrada de usuario; `$8` es el offset fijo de `HORARIO_ZONA` en segundos — ver spec `08_horario_zona_configurable`; con offset `0` la condición equivale al `AT TIME ZONE 'UTC'` original).
4. `src/modules/reportes/application/consultar_reportes.rs`: eliminar `intervalo_horas`/`intervalos` de `ResultadoConsulta`, `construir_serie_intervalos` y el bloque de agregación de `consultar_reportes` (vuelve al contrato exacto de la spec `02`).
5. `src/modules/reportes/routes.rs` (actualizar el comentario doc que menciona la agregación) y `src/docs.rs` (quitar `IntervaloReportes` de los componentes y del registro OpenAPI de la spec `07`; el param `intervalo_horas` desaparece de `IntoParams` al eliminarse el campo).
6. Reemplazar las pruebas de agregación de `src/modules/reportes/routes/tests.rs` (incluida `dia_con_intervalo_horas_genera_la_serie_del_dia`) por las de la §5.
7. Consistencia documental: `specs/GLOSSARY.md` (sustituir `intervalo de horas`, `agregación por intervalos` y `segmento` por: **hora del día (`hora`):** query param opt-in del feed (entero `0–23`, UTC) que acota la consulta a la franja semiabierta `[hora:00, hora+1:00)` de cada día incluido en la ventana temporal), `specs/ARCHITECT.md` y las specs `approved/06_filtro_por_dia.md` y `approved/07_documentacion_openapi.md` (referencias a `intervalo_horas`/`IntervaloReportes` ⇒ `hora`).
8. Ejecutar la Fase de Validación completa y autocorregir hasta el 100%:
   - `cargo fmt --all -- --check`
   - `cargo clippy --all-targets -- -D warnings`
   - `cargo check --all-targets`
   - `cargo test`
