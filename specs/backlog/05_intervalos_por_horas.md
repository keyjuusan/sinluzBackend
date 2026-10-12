# Spec: Agregación de Reportes por Intervalos de Horas

## 1. Visión General e Impacto
- **Objetivo:** Añadir al feed de lectura un **filtro de agregación temporal** que agrupa los reportes en intervalos de horas dentro de la ventana consultada, devolviendo el conteo de reportes por segmento (serie temporal). Sirve para que el cliente dibuje histogramas/evolución de cortes por franjas horarias, no solo el mapa de puntos.
- **Contexto:** Extiende el módulo `reportes` existente (specs `approved/01_registrar_reporte` y `approved/02_consultar_reportes`). Es **aditivo y opt-in**: no crea endpoint nuevo; el feed `GET /api/v1/reportes` sigue funcionando igual y solo cuando el cliente envía `intervalo_horas` se añade el campo `intervalos` a la respuesta. La agregación respeta **los mismos filtros** que el feed (ventana temporal + bbox), ignorando `limit`/`offset`.
  - Se **modifican**: `src/modules/reportes/domain/consulta.rs` (nuevo param y su validación), `src/modules/reportes/domain/mod.rs` (re-export de `IntervaloReportes`), `src/modules/reportes/application/consultar_reportes.rs` (campo opcional `intervalos` en `ResultadoConsulta` y construcción de la serie completa) y `src/modules/reportes/infra/postgres_repositorio.rs` (método de conteo agrupado). Se **modifican** también los tests en `src/modules/reportes/routes/tests.rs`.
  - No se **crean** módulos nuevos, no hay migración (reutiliza la tabla `reportes`) ni autenticación en esta spec. `src/modules/reportes/routes.rs` no cambia (los DTOs ya viven en `domain`/`application`).

### Dependencias autorizadas (esta spec ordena explícitamente tocar `Cargo.toml`)
Ninguna. Queda prohibido añadir librerías o modificar `Cargo.toml` sin orden explícita de una spec. La agregación se resuelve con `chrono` y SQL de SQLx ya presentes.

## 2. Modelos de Datos / Contratos

### Constantes y entidad de dominio (`src/modules/reportes/domain/consulta.rs`)
```rust
pub const INTERVALO_HORAS_DEFECTO: u32 = 1;
pub const INTERVALO_HORAS_MAXIMO: u32 = 24;
pub const MAX_INTERVALOS: i64 = 1000;

pub struct IntervaloReportes {
    pub inicio: DateTime<Utc>,
    pub fin: DateTime<Utc>,
    pub total: i64,
}
```
- `SolicitudConsultarReportes` gana el campo `pub intervalo_horas: Option<String>` (el resto de campos no cambia).
- `FiltrosConsultaReporte` gana el campo `pub intervalo_horas: Option<u32>` (`None` = agregación desactivada).
- `ErrorValidacionConsulta` gana la variante `IntervaloInvalido` (mensaje en español) para valores fuera de `1..=24` o no numéricos.
- `IntervaloReportes` representa un segmento contiguo **semiabierto** `[inicio, fin)`.

> Extensibilidad (filtros futuros): el punto único de extensión sigue siendo `SolicitudConsultarReportes` + `FiltrosConsultaReporte` + `construir_filtros`. Un filtro nuevo se añade como campo opcional en ambos structs, su validación pura en `construir_filtros` y (si aplica) un binding opcional más en las queries estáticas. No se toca el handler ni la composición del Router.

### Contratos HTTP
- **Request** `GET /api/v1/reportes` (query params ya existentes sin cambios; se añade):
  | Parámetro | Tipo | Default | Regla |
  |---|---|---|---|
  | `intervalo_horas` | u32 | ausente ⇒ agregación desactivada | opt-in; si está presente con valor, entero `1..=24`; valor vacío (`intervalo_horas=`) ⇒ `1`; la ventana debe producir `≤ MAX_INTERVALOS` (1000) segmentos |
- **Response éxito** `200 OK` (mismo contrato de `02`, con un campo opcional nuevo):
```json
{
  "reportes": [
    { "id": "9b0e...", "lat": -33.4489, "lng": -70.6693, "creado": "2026-10-06T14:30:00Z", "horas_duracion": 2 }
  ],
  "total": 123,
  "limit": 1000,
  "offset": 0,
  "intervalo_horas": 3,
  "intervalos": [
    { "inicio": "2026-10-05T00:00:00Z", "fin": "2026-10-05T03:00:00Z", "total": 12 },
    { "inicio": "2026-10-05T03:00:00Z", "fin": "2026-10-05T06:00:00Z", "total": 0 }
  ]
}
```
- **Sin** `intervalo_horas` en la petición, la respuesta **omite** `intervalo_horas` e `intervalos` (compatibilidad total con clientes actuales de la spec `02`).
- Los campos `intervalo_horas` e `intervalos` se serializan solo cuando la agregación está activa (`#[serde(skip_serializing_if = "Option::is_none")]`).
- `id_usuario` **no** se expone en la respuesta.

## 3. Reglas de Negocio (Criterios de Aceptación)
- [ ] **Regla 1 (Entradas):** `intervalo_horas` ausente ⇒ agregación desactivada (campo `intervalos` ausente en la respuesta, comportamiento idéntico a `02`). Presente y vacío ⇒ `INTERVALO_HORAS_DEFECTO` (1). Presente con valor ⇒ debe ser entero en `1..=INTERVALO_HORAS_MAXIMO`; cualquier otro valor (no numérico, `0`, `>24`, negativo, decimal) ⇒ `400` con `ErrorValidacionConsulta::IntervaloInvalido`.
- [ ] **Regla 2 (Proceso):** la agregación agrupa los reportes que cumplen **la misma ventana temporal y bbox** que el feed. `limit`/`offset` NO afectan a la agregación. El `total` de todos los segmentos es igual al `total` del feed con esos mismos filtros.
- [ ] **Regla 3 (Segmentación):** los segmentos se alinean a múltiplos del intervalo **desde `desde`** (inicio de la ventana) y avanzan en pasos de `intervalo_horas`. El segmento `i` es `[desde + i·N h, desde + (i+1)·N h)`; el último se recorta a `hasta`. Por lo tanto el primer `inicio` es `desde`, los segmentos son contiguos (`fin` de uno == `inicio` del siguiente) y el último `fin` es `hasta`. El número de segmentos es `ceil((hasta - desde)/N)`. Se incluyen **todos** los segmentos de la ventana, también los de conteo `0`.
- [ ] **Regla 4 (Límites):** la solicitud no puede generar más de `MAX_INTERVALOS` (1000) segmentos; si `ceil((hasta - desde)/N) > 1000` ⇒ `400` con `ErrorValidacionConsulta::IntervaloInvalido` (mensaje que indique la ventana/intervalo excesivo). Evita respuestas y cómputos desbordados con ventanas muy amplias.
- [ ] **Regla 5 (Salidas):** éxito ⇒ `200` con `{reportes, total, limit, offset}` y, si aplica, `intervalo_horas` + `intervalos` (lista ordenada por `inicio` ASC). Errores convertidos a respuestas HTTP en el único punto del módulo: validación ⇒ `400`, fallo de persistencia/infra ⇒ `500`. Los handlers no construyen respuestas a mano.

## 4. Comportamiento de la Interfaz / API
- **Endpoint:** `GET /api/v1/reportes` (sin cambios de ruta ni de verbo).
- **Casos de Éxito:**
  - `GET /api/v1/reportes` (sin `intervalo_horas`) ⇒ `200` con el body actual; **sin** `intervalos`.
  - `GET /api/v1/reportes?desde=...&hasta=...&intervalo_horas=3` ⇒ `200` con `intervalos` (segmentos de 3 h, alineados a `desde`, con ceros incluidos) y `intervalo_horas: 3`.
  - `GET /api/v1/reportes?intervalo_horas=` ⇒ `200` con segmentos de `1` h (default).
  - `GET /api/v1/reportes?intervalo_horas=2&min_lat=...&max_lat=...&min_lng=...&max_lng=...` ⇒ los segmentos cuentan solo los reportes dentro del bbox.
  - Ventana válida sin reportes ⇒ `200` con `intervalos` de conteos `0` (serie completa) y `total: 0`.
- **Casos de Error:**
  - `400 Bad Request` — `intervalo_horas` no numérico, `0`, `>24` o negativo; ventana/intervalo que produciría más de 1000 segmentos; (y los ya existentes de la spec `02`: fechas inválidas, `desde >= hasta`, bbox inválido, `limit`/`offset` fuera de rango, params desconocidos).
  - `500 Internal Server Error` — error de infraestructura (conexión/consulta a la BD).
  - Toda respuesta de error usa `application/json` con body `{ "error": "<motivo en español>" }`, p. ej. `{ "error": "intervalo_horas inválido: debe estar entre 1 y 24" }`.

## 5. Estrategia de Pruebas Obligatoria
> **Precondición:** los tests requieren Postgres corriendo (`docker compose up -d db`, `DATABASE_URL`) y siguen el patrón de `routes/tests.rs`: `tower::ServiceExt::oneshot`, `http-body-util::BodyExt`, inserción directa con `creado`/`id_usuario` únicos y limpieza por banda de coordenadas. No se añaden `dev-dependencies`.

- **Pruebas de Camino Feliz:**
  - [ ] `intervalo_horas=1` con ventana explícita acotada y reportes en horas conocidas ⇒ la respuesta trae `intervalos` con la longitud `ceil((hasta-desde)/1h)`, los conteos correctos por segmento y los segmentos vacíos con `total: 0`.
  - [ ] La suma de `total` de todos los segmentos es igual al `total` del feed (invariante de la Regla 2).
  - [ ] Alineación: primer `inicio == desde`, segmentos contiguos (`fin[i] == inicio[i+1]`), paso exacto de `intervalo_horas`, último `fin == hasta`.
  - [ ] Con `intervalo_horas=3` (varios reportes en el mismo segmento) el conteo se agrupa en el segmento correcto.
  - [ ] Con bbox los segmentos solo cuentan reportes dentro del rectángulo.
  - [ ] `intervalo_horas=` (vacío) usa el default (`intervalo_horas: 1` en la respuesta).
  - [ ] Sin `intervalo_horas` la respuesta **no** contiene las claves `intervalos` ni `intervalo_horas` (compatibilidad con la spec `02`).
  - [ ] Ventana válida sin reportes ⇒ `200` con serie completa de ceros y `total: 0`.
- **Pruebas de Borde y Errores:**
  - [ ] `intervalo_horas=0` ⇒ `400` con `intervalo_horas` en el body.
  - [ ] `intervalo_horas=25` ⇒ `400`.
  - [ ] `intervalo_horas=-1` y `intervalo_horas=abc` ⇒ `400`.
  - [ ] `intervalo_horas=1` con ventana amplia que genere más de 1000 segmentos (p. ej. `desde` muy antiguo) ⇒ `400`.
  - [ ] `intervalo_horas=4&limit=1&offset=1` ⇒ `200`: `limit`/`offset` afectan a `reportes` pero la serie de `intervalos` cubre toda la ventana.

## 6. Pasos Sugeridos para la Implementación (Orden de Ejecución)
1. `src/modules/reportes/domain/consulta.rs`: añadir constantes `INTERVALO_HORAS_DEFECTO`, `INTERVALO_HORAS_MAXIMO`, `MAX_INTERVALOS`; campo `intervalo_horas: Option<String>` en `SolicitudConsultarReportes`; campo `intervalo_horas: Option<u32>` en `FiltrosConsultaReporte`; entidad `IntervaloReportes`; variante `ErrorValidacionConsulta::IntervaloInvalido` con `Display` en español; y en `construir_filtros` la validación del parámetro (vacío ⇒ default; fuera de rango/no numérico ⇒ error) junto con el guard `ceil((hasta-desde)/N) ≤ MAX_INTERVALOS`. Reexportar `IntervaloReportes` en `domain/mod.rs`.
2. `src/modules/reportes/infra/postgres_repositorio.rs`: añadir un método de conteo agrupado (p. ej. `contar_reportes_por_intervalo(&self, filtros) -> Result<Vec<IntervaloReportes>, sqlx::Error>`) con **una única query estática** y bindings opcionales de bbox, agrupando por el inicio del segmento calculado a partir de `desde` y el tamaño del intervalo (sin SQL dinámico por entrada de usuario). Devuelve solo los segmentos **no vacíos**, ordenados por `inicio`.
3. `src/modules/reportes/application/consultar_reportes.rs`: ampliar `ResultadoConsulta` con `intervalo_horas: Option<u32>` e `intervalos: Option<Vec<IntervaloReportes>>` (`skip_serializing_if = "Option::is_none"`). En `consultar_reportes`, tras listar y contar, si `filtros.intervalo_horas` es `Some(n)`, obtener los conteos no vacíos del repositorio y **rellenar la serie completa** (incluidos ceros) iterando de `desde` a `hasta` en pasos de `n` horas con `chrono`, recortando el último `fin` a `hasta`.
4. Actualizar los tests de `src/modules/reportes/routes/tests.rs` según la §5 (helpers que construyan ventanas acotadas y parseen `intervalos`). `routes.rs` no requiere cambios.
5. Ejecutar la Fase de Validación completa y autocorregir hasta el 100%:
   - `cargo fmt --all -- --check`
   - `cargo clippy --all-targets -- -D warnings`
   - `cargo check --all-targets`
   - `cargo test`
