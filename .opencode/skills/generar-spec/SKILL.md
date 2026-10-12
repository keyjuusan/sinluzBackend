---
name: generar-spec
description: Genera una nueva spec de funcionalidad en server/specs/backlog siguiendo las convenciones del proyecto SinLuz. Usar cuando el usuario pida "crear spec", "nueva spec", "escribir spec", "espec para X", "añadir idea al backlog" o quiera convertir una idea pendiente en spec formal.
---

# Generar Spec (SinLuz)

Eres el encargado de transformar una idea de funcionalidad en una **spec atómica** lista para implementar más adelante. Trabajas con la carpeta `server/specs/` del repositorio SinLuz.

## Contexto obligatorio antes de escribir

Lee siempre, en este orden, antes de redactar cualquier spec:

1. `server/specs/ARCHITECT.md` — stack autorizado, estructura de módulos y roadmap. Respeta el stack: no propongas librerías fuera de él.
2. `server/specs/GLOSSARY.md` — terminología de negocio. Usa SIEMPRE estos términos para nombrar variables, funciones, tablas y claves JSON. Si un término de negocio no está definido, proponlo en la spec y añádelo al glosario.
3. `server/specs/approved/` y `server/specs/backlog/` — specs existentes para mantener coherencia, numeración y evitar duplicados.
4. `server/AGENTS.md` — reglas operativas (especialmente §3 dependencias y §4 estilo).

## Número y nombre de archivo

- Nombre: `NN_nombre_en_snake_case.md` (ej.: `02_feed_heatmap.md`).
- `NN` = siguiente número secuencial: máx. `NN` existente en `specs/active`, `specs/approved` y `specs/backlog` + 1. El archivo se crea SIEMPRE en `server/specs/backlog/`.
- No muevas la spec a `active/` ni la implementes: eso lo decide el usuario después.

## Flujo de trabajo

### Fase 1 — Preguntas de clarificación (obligatorio)

Antes de redactar, usa la herramienta `question` para resolver las decisiones que condicionan la spec. No supongas. Cubre al menos:

- **Objetivo e impacto:** ¿Qué construye y para qué sirve? ¿Interactúa con código existente o es módulo nuevo?
- **Alcance:** ¿Cuánta superficie entra en esta spec? No sobre-incluyas todo el backlog.
- **Identidad/autorización:** ¿dispositivo anónimo, usuario autenticado (auth está pendiente en roadmap), o sin identidad?
- **Límites/validaciones:** rangos, longitudes máximas, cooldown/anti-spam, idempotencia.
- **Contratos:** endpoints (ruta, método, request/response, códigos de error).
- **Nomenclatura:** ¿términos del glosario aplican? ¿duplicar términos a GLOSSARY?
- **Tests:** ¿se permite tocar `Cargo.toml`? ¿dev-deps de tests autorizadas (tower, http-body-util)? Si la spec necesita librerías nuevas, debe quedar EXPRESADO explícitamente en la sección de dependencias.
- **SQLx/Postgres:** si la spec añade persistencia, ¿migración nueva en `server/migrations/`? ¿README del arranque con `sqlx::migrate!`?

### Fase 2 — Redactar la spec

Escribe el archivo en `server/specs/backlog/<NN>_<nombre>.md` con esta estructura EXACTA de 6 secciones (en español, lenguaje técnico y conciso):

```markdown
# Spec: <Nombre Corto y Claro de la Funcionalidad>

## 1. Visión General e Impacto
- **Objetivo:** [QUÉ construye y para QUÉ sirve]
- **Contexto:** [interacción con código existente o módulo nuevo]

### Dependencias autorizadas (esta spec ordena explícitamente tocar `Cargo.toml`)
[SOLO si aplica y con justificación de cada una. `[dependencies]` y `[dev-dependencies]` por separado. Sin esto, queda prohibido añadir librerías.]

## 2. Modelos de Datos / Contratos (Si aplica)
[estructuras: tablas SQL/migraciones, entidades Rust (tipado, snake_case), DTOs request/response]

## 3. Reglas de Negocio (Criterios de Aceptación)
- [ ] **Regla 1 (Entradas):** [validaciones de entrada]
- [ ] **Regla 2 (Proceso):** [lógica central]
- [ ] **Regla 3 (Límites):** [límites, cooldown, idempotencia]
- [ ] **Regla 4 (Salidas):** [códigos/errores esperados]

## 4. Comportamiento de la Interfaz / API (Si aplica)
- **Ruta / Endpoint / Función:** [ruta, verbo]
- **Casos de Éxito:** [status + body]
- **Casos de Error:** [códigos + body de error, mensajes en español]

## 5. Estrategia de Pruebas Obligatoria (Tests)
- **Pruebas de Camino Feliz:** [escenarios]
- **Pruebas de Borde y Errores:** [escenarios]

## 6. Pasos Sugeridos para la Implementación (Orden de Ejecución)
[numerados, desde dependencias/tipos hasta tests y la Fase de Validación de AGENTS §5: cargo fmt --all -- --check, cargo clippy --all-targets -- -D warnings, cargo check --all-targets, cargo test]
```

### Reglas de redacción inquebrantables

- **Idioma:** todo en español; mensajes de error de la API en español.
- **Tipado:** tipos propios explícitos; nunca `serde_json::Value` ni tipos dinámicos como tipos de dominio.
- **Nomenclatura:** `snake_case` (módulos, funciones, variables, claves JSON y tablas); `PascalCase` (struct/enum/trait); `SCREAMING_SNAKE_CASE` (constantes). Respeta GLOSSARY.md.
- **Ámbito mínimo:** UNA funcionalidad atómica por spec. No mezcles features de roadmap.
- **Dependencias:** si la spec necesita librerías nuevas, decláralas explícitamente en la Sección 1 con justificación; si no las necesita, deja claro que está prohibido tocar `Cargo.toml` sin orden explícita.
- **Errores:** modelar como `enum` propios que implementan `std::error::Error` y `Display`; conversión a HTTP en un único punto.
- **HTTP:** rutas versionadas bajo `/api/v1`; códigos de estado concretos y coherentes con los ya usados (400 validación, 429 cooldown, 500 infra).
- **Existente vs nuevo:** marca qué módulos/archivos se crean y cuáles se modifican (ej.: `main.rs`, `migrations/`).

### Fase 3 — Glosario y cierre

- Si introdujiste términos de negocio nuevos, actualiza `server/specs/GLOSSARY.md` (mismo commit lógico o menciónalo al usuario).
- Confirma la ruta creada y resume los puntos clave de la spec al usuario en ≤5 líneas.
- Recuerda: NO implementes, NO muevas a `active/` y NO hagas commits salvo orden explícita.