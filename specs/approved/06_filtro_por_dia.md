# Spec: Filtro del Feed por Día Calendario

## 1. Visión General e Impacto
- **Objetivo:** Añadir al feed de lectura un **filtro por día calendario** mediante el query param `dia` (`YYYY-MM-DD`), que acota la consulta a los reportes de esa jornada completa en UTC. Da al cliente una forma directa de consultar "lo que pasó tal día" sin construir timestamps RFC 3339.
- **Contexto:** Extiende el módulo `reportes` existente (specs `approved/01_registrar_reporte` y `approved/02_consultar_reportes`). Es **aditivo y opt-in**: no crea endpoint nuevo, no cambia la forma de la respuesta y no añade campos al body. `dia` actúa como **alternativa** a la ventana `desde`/`hasta`: produce internamente un rango `[dia 00:00:00Z, (dia+1) 00:00:00Z)` y el resto de filtros (bbox, `limit`/`offset`, `intervalo_horas` de la spec `05`) siguen funcionando igual.
  - Se **modifican**: `src/modules/reportes/domain/consulta.rs` (campo del param, errores y validación) y los tests en `src/modules/reportes/routes/tests.rs`.
  - **No** se modifica: `FiltrosConsultaReporte` (sigue con `desde`/`hasta`), `application/consultar_reportes.rs`, `infra/postgres_repositorio.rs` ni `routes.rs` (el filtro se materializa como ventana temporal antes de llegar al repositorio). No hay migración nueva.
  - Reutiliza el punto único de extensión ya definido en la spec `05` (`SolicitudConsultarReportes` + `construir_filtros` + `FiltrosConsultaReporte`), pensado para filtros futuros.

### Dependencias autorizadas (esta spec ordena explícitamente tocar `Cargo.toml`)
Ninguna. Queda prohibido añadir librerías o modificar `Cargo.toml`. El parseo del día se resuelve con `chrono`, ya presente.

## 2. Modelos de Datos / Contratos

### Dominio (`src/modules/reportes/domain/consulta.rs`)
- `SolicitudConsultarReportes` gana el campo `pub dia: Option<String>` (además de los campos existentes y de `intervalo_horas` de la spec `05`).
- `FiltrosConsultaReporte` **no cambia**: `dia` se resuelve a `desde`/`hasta` antes de construir los filtros.
- `ErrorValidacionConsulta` gana dos variantes, con `Display` en español:
  - `DiaInvalido` → `"dia inválido: debe ser una fecha en formato YYYY-MM-DD"`.
  - `DiaIncompatible` → `"dia no es compatible con desde/hasta"`.
- Helper puro `parse_dia(texto: &str) -> Result<(DateTime<Utc>, DateTime<Utc>), ErrorValidacionConsulta>` que devuelve el rango `[dia 00:00:00Z, dia+1 00:00:00Z)`.

### Contratos HTTP
- **Request** `GET /api/v1/reportes` (params existentes sin cambios; se añade):
  | Parámetro | Tipo | Default | Regla |
  |---|---|---|---|
  | `dia` | `YYYY-MM-DD` | ausente ⇒ se usa la ventana `desde`/`hasta` actual | opt-in; día calendario en **UTC**; mutuamente excluyente con `desde` y `hasta` |
- **Response:** **idéntico** al de la spec `02` (`{reportes, total, limit, offset}` y, si aplica, `intervalo_horas`/`intervalos` de la `05`). No se añaden campos ni se refleja `dia` en el body. `id_usuario` sigue sin exponerse.

## 3. Reglas de Negocio (Criterios de Aceptación)
- [ ] **Regla 1 (Entradas):** `dia` ausente ⇒ comportamiento actual (`desde`/`hasta` o ventana por defecto de 24 h). `dia` presente ⇒ debe ser una fecha de calendario válida en formato `YYYY-MM-DD`; malformada o inexistente (p. ej. `2026-13-01`, `2026-02-30`, un RTC 3339 con hora, o vacío) ⇒ `400` con `ErrorValidacionConsulta::DiaInvalido`.
- [ ] **Regla 2 (Exclusividad):** `dia` no puede combinarse con `desde` ni con `hasta`; si aparece junto a cualquiera de ellos ⇒ `400` con `ErrorValidacionConsulta::DiaIncompatible`. La comprobación se hace antes de intentar parsear `desde`/`hasta`.
- [ ] **Regla 3 (Proceso):** `dia` define la ventana semiabierta `[dia 00:00:00Z, (dia+1) 00:00:00Z)` sobre el campo `creado`; la fecha se interpreta en **UTC**. Reportes exactamente a las `00:00:00Z` del día entran; los de `00:00:00Z` del día siguiente quedan fuera. `total` es el conteo con esa ventana.
- [ ] **Regla 4 (Compatibilidad con otros filtros):** `dia` es combinable con `bbox`, `limit`/`offset` e `intervalo_horas` (spec `05`). Con `intervalo_horas`, la agregación se calcula sobre la ventana del día (p. ej. `dia` + `intervalo_horas=1` ⇒ hasta 24 segmentos horarios).
- [ ] **Regla 5 (Salidas):** éxito ⇒ `200` con el body habitual (posiblemente vacío). Errores en el único punto de conversión del módulo: validación ⇒ `400`, fallo de persistencia/infra ⇒ `500`. Los handlers no construyen respuestas de error a mano.

## 4. Comportamiento de la Interfaz / API
- **Endpoint:** `GET /api/v1/reportes` (sin cambios de ruta ni de verbo).
- **Casos de Éxito:**
  - `GET /api/v1/reportes?dia=2026-10-05` ⇒ `200` con los reportes de esa jornada UTC y `total` coherente.
  - `GET /api/v1/reportes?dia=2026-10-05&min_lat=...&max_lat=...&min_lng=...&max_lng=...` ⇒ aplica además el bbox.
  - `GET /api/v1/reportes?dia=2026-10-05&limit=100&offset=0` ⇒ paginación dentro del día.
  - `GET /api/v1/reportes?dia=2026-10-05&intervalo_horas=1` ⇒ feed del día + `intervalos` horarios.
  - Día sin reportes ⇒ `200` con `reportes: []` y `total: 0`.
- **Casos de Error:**
  - `400 Bad Request` — `dia` malformado o con fecha inexistente; `dia` junto con `desde` o `hasta`; (y los ya existentes de la spec `02`).
  - `500 Internal Server Error` — error de infraestructura (conexión/consulta a la BD).
  - Body de error uniforme `{ "error": "<motivo en español>" }`, p. ej. `{ "error": "dia inválido: debe ser una fecha en formato YYYY-MM-DD" }` o `{ "error": "dia no es compatible con desde/hasta" }`.

## 5. Estrategia de Pruebas Obligatoria
> **Precondición:** los tests requieren Postgres corriendo (`docker compose up -d db`, `DATABASE_URL`) y siguen el patrón de `routes/tests.rs` (`tower::ServiceExt::oneshot`, `http_body_util::BodyExt`, inserción directa con `creado` únicos y limpieza por banda de coordenadas). No se añaden `dev-dependencies`.

- **Pruebas de Camino Feliz:**
  - [ ] `dia` con reportes insertados dentro del día y fuera de él ⇒ solo devuelve los del día y `total` es correcto.
  - [ ] Borde inclusivo: un reporte con `creado` exactamente en `dia 00:00:00Z` entra; uno con `creado` exactamente en `(dia+1) 00:00:00Z` no entra.
  - [ ] `dia` + bbox devuelve solo los reportes dentro del rectángulo y del día.
  - [ ] `dia` + `limit`/`offset` pagina dentro del día sin repetir elementos y con `total` constante.
  - [ ] `dia` + `intervalo_horas=1` produce la serie horaria del día y la suma de sus segmentos iguala el `total`.
  - [ ] `dia` de una jornada sin reportes ⇒ `200` con `reportes: []` y `total: 0`.
- **Pruebas de Borde y Errores:**
  - [ ] `dia=no-es-fecha` ⇒ `400` con `dia` en el body.
  - [ ] `dia=2026-13-01` y `dia=2026-02-30` (mes/día inexistentes) ⇒ `400`.
  - [ ] `dia=2026-10-05T00:00:00Z` (con hora) ⇒ `400`.
  - [ ] `dia=` (vacío) ⇒ `400`.
  - [ ] `dia=2026-10-05&desde=2026-10-04T00:00:00Z` ⇒ `400` con mensaje de incompatibilidad.
  - [ ] `dia=2026-10-05&hasta=2026-10-06T00:00:00Z` ⇒ `400` con mensaje de incompatibilidad.
  - [ ] `dia=2026-10-05&foo=1` (param desconocido) ⇒ `400` (se mantiene `deny_unknown_fields`).

## 6. Pasos Sugeridos para la Implementación (Orden de Ejecución)
1. `src/modules/reportes/domain/consulta.rs`: añadir `dia: Option<String>` a `SolicitudConsultarReportes`; añadir las variantes `DiaInvalido` y `DiaIncompatible` a `ErrorValidacionConsulta` con su `Display`; implementar `parse_dia` (con `chrono::NaiveDate` + `and_hms_opt(0,0,0)`/`and_utc`, sumando `Duration::days(1)` para el fin) devolviendo el rango UTC.
2. En `construir_filtros`: si `dia` está presente y `desde`/`hasta` también ⇒ `DiaIncompatible`; si solo `dia` ⇒ `desde`/`hasta` se toman de `parse_dia` (sin aplicar el default de 24 h ni la validación `desde < hasta` sobre los params ausentes); si `dia` está ausente ⇒ lógica actual intacta. No introducir `unwrap`/`expect`/`panic!`.
3. Ampliar los tests de `src/modules/reportes/routes/tests.rs` según la §5 (helper para calcular el literal `YYYY-MM-DD` del día a partir de `creado`, y aserciones de borde en `00:00:00Z`).
4. Ejecutar la Fase de Validación completa y autocorregir hasta el 100%:
   - `cargo fmt --all -- --check`
   - `cargo clippy --all-targets -- -D warnings`
   - `cargo check --all-targets`
   - `cargo test`
