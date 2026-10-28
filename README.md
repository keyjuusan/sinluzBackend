# SinLuz — Servidor

API REST y WebSocket del proyecto SinLuz, construida con Rust y axum.

## Atribución de datos geográficos

La validación de que un reporte cae dentro del territorio venezolano utiliza el
dataset embebido del crate [`country-boundaries`](https://crates.io/crates/country-boundaries)
(licencia Apache-2.0).

Los datos de fronteras derivan de OpenStreetMap, © OpenStreetMap contributors, y
se distribuyen bajo la [Open Database License (ODbL)](https://opendatacommons.org/licenses/odbl/).
