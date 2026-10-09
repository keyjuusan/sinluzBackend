## 1. Visión General del Sistema
- **Nombre del Proyecto:** SinLuz
- **Propósito:** Registrar y visualizar de forma crowdsourced los cortes de suministro eléctrico. Los usuarios reportan apagones; el cliente los muestra sobre un mapa de calor (heatmap) con Leaflet.
- **Patrón de Arquitectura:** Monolito Modular (backend) + SPA con API REST versionada bajo `/api/v1`.

## 2. Stack Tecnológico Autorizado
El agente de IA no debe utilizar herramientas, frameworks o librerías fuera de este listado:
- **Backend / API:** Rust, axum (features `["ws"]` para WebSockets), tokio, serde / serde_json. Entry point: `server/src/main.rs`.
- **Base de Datos / Persistencia:** PostgreSQL con SQLx, ya integrado en `Cargo.toml` por la spec `01_registrar_reporte` (approved). Features activas: `postgres`, `runtime-tokio-rustls`, `migrate`, `chrono`. Soporte de fechas con `chrono` (decodificación de `TIMESTAMPTZ`) y carga de `.env` con `dotenvy`. Migraciones en `server/migrations/`, aplicadas en el arranque con `sqlx::migrate!`.
- **Tests de integración:** `tower` (oneshot sobre el Router), `http-body-util` y `tokio-tungstenite` (cliente WebSocket e2e, sin features TLS) como `[dev-dependencies]`.
- **Frontend / UI:** React, Vite, TypeScript, Tailwind CSS, Leaflet + leaflet.heat, vite-plugin-pwa, lucide-react.
- **Gestión de Estado / Caché:** sin librería de estado global definida todavía; preferir `useReducer`/hooks nativos de React hasta que una spec justifique otra.
- **Datos de prueba:** @faker-js/faker ya está instalado en `client/` para generar datos sintéticos en desarrollo.

## 3. Estructura Global de Carpetas
```text
sinluz/
├── client/                     # SPA React/Vite (pendiente de scaffolding: sin package.json)
│   ├── src/
│   └── public/
├── server/                     # API Rust/axum
│   ├── src/
│   │   ├── main.rs             # Arranque del servidor: config, migraciones y composición del Router
│   │   ├── config.rs           # Carga de .env (dotenvy) y DATABASE_URL
│   │   ├── modules/            # Módulos encapsulados por dominio de negocio
│   │   │   ├── reportes/       # routes → application → domain → infra
│   │   │   └── actividad/      # WebSocket de actividad en tiempo real (domain → infra → application → routes)
│   │   └── shared/             # (reservado) tipos y utilidades reutilizables
│   ├── migrations/             # Migraciones SQLx (0001_crear_reportes.sql)
│   ├── specs/
│   │   ├── active/             # Spec en curso (objetivo actual de construcción)
│   │   ├── approved/           # Specs completadas y validadas
│   │   ├── backlog/            # Ideas pendientes
│   │   ├── ARCHITECT.md        # Este documento
│   │   └── GLOSSARY.md         # Terminología de negocio
│   ├── docker-compose.yml      # postgres:17 de desarrollo (puerto 5432, credenciales sinluz/sinluz)
│   ├── .env.example            # Plantilla de variables de entorno (DATABASE_URL)
│   ├── Cargo.toml
│   └── AGENTS.md               # Reglas operativas del agente
└── .gitignore
```

**Capas dentro de un módulo** (`src/modules/<dominio>/`): `routes` (handlers axum) → `application` (casos de uso) → `domain` (entidades y reglas) → `infra` (persistencia y clientes externos). Los handlers no tocan `domain` directamente.

## 4. Convenciones de Código y Estándares de Ingeniería
Directrices técnicas de alto nivel para mantener la consistencia en todo el repositorio:
- **Nomenclatura:** `snake_case` para módulos, funciones y variables; `PascalCase` para `struct`, `enum` y `trait`; `SCREAMING_SNAKE_CASE` para constantes; `snake_case` en claves de JSON y tablas.
- **Paradigma:** errores como tipos de dominio explícitos (`enum` que implementa `std::error::Error` y `Display`); prohibido `unwrap()`/`expect()`/`panic!` en código de producción; prohibido el uso de tipos dinámicos sin estructura (`serde_json::Value` no es un tipo de dominio válido).
- **Capas HTTP:** los errores se convierten a respuestas en un único punto de la aplicación; los handlers no construyen respuestas a mano.
- **Estrategia Git:** Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`). Detalle completo en `server/AGENTS.md` §6.
- **Seguridad:** nunca guardar credenciales en código, usar siempre variables de entorno (`.env`). Validar y sanitizar todas las entradas de usuario. No stagear secretos ni `.env`.

## 5. Estado Actual del Sistema y Roadmap
- [x] Entorno Rust 2024 configurado (`Cargo.toml`, toolchain 1.97).
- [x] Servidor axum arrancando en `127.0.0.1:1234` con endpoint `/api/health`.
- [x] Registrar reportes de corte — spec `approved/01_registrar_reporte.md`: `POST /api/v1/reportes` con validaciones, cooldown anti-spam (15 min / ~100 m) y persistencia SQLx.
- [x] Persistencia PostgreSQL/SQLx con migraciones y entorno de desarrollo (docker-compose).
- [x] `specs/GLOSSARY.md` con la terminología de negocio en español.
- [ ] Autenticación y autorización de usuarios.
- [x] Feed de datos para el heatmap del cliente — spec `approved/02_consultar_reportes.md`: `GET /api/v1/reportes` con ventana temporal, bbox opcional y paginación `limit`/`offset`.
- [x] Actividad en tiempo real — spec `approved/03_websocket_actividad.md`: `GET /api/v1/actividad/ws` emite `reporte_creado` al crear un reporte; 100 conexiones máx. (429), keepalive ping 30 s / inactividad 60 s, sin `id_usuario`.
- [ ] Scaffolding del cliente (no existe `package.json`).
- [ ] Offline de la PWA (registro de reportes sin conexión).