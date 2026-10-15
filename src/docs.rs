use axum::{Json, Router, routing::get};
use serde::Serialize;
use utoipa::OpenApi;
use utoipa_scalar::{Scalar, Servable};

use crate::modules::actividad::domain::EventoActividad;
use crate::modules::reportes::application::ResultadoConsulta;
use crate::modules::reportes::domain::{ReporteConsulta, SolicitudConsultarReportes};
use crate::modules::reportes::routes::{RespuestaRegistroReporte, SolicitudCrearReporte};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "SinLuz API",
        version = "0.1.0",
        description = "API de reportes de cortes de suministro eléctrico."
    ),
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
    ))
)]
pub struct ApiDoc;

/// Cuerpo de error común a todas las respuestas de error de la API.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct RespuestaError {
    /// Mensaje de error legible para el cliente.
    pub error: String,
}

async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

pub fn router() -> Router {
    Router::new()
        .merge(Scalar::with_url("/api/docs", ApiDoc::openapi()))
        .route("/api/docs/openapi.json", get(openapi_json))
}

pub fn router_opcional(habilitado: bool) -> Router {
    if habilitado { router() } else { Router::new() }
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::Body,
        http::{Method, Request, StatusCode, header},
    };
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tower::ServiceExt;

    use super::{router, router_opcional};

    async fn pedir(app: &Router, uri: &str) -> (StatusCode, Option<String>, String) {
        let respuesta = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request inválido"),
            )
            .await
            .expect("el router no respondió");

        let status = respuesta.status();
        let content_type = respuesta
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|valor| valor.to_str().expect("content-type ASCII").to_owned());
        let bytes = respuesta
            .into_body()
            .collect()
            .await
            .expect("no se pudo leer el body")
            .to_bytes();
        (
            status,
            content_type,
            String::from_utf8(bytes.to_vec()).expect("body no es utf8"),
        )
    }

    #[tokio::test]
    async fn la_ui_de_scalar_se_sirve_como_html() {
        let (status, content_type, body) = pedir(&router(), "/api/docs").await;

        assert_eq!(status, StatusCode::OK);
        assert!(
            content_type
                .as_deref()
                .is_some_and(|valor| valor.starts_with("text/html")),
            "content-type inesperado: {content_type:?}"
        );
        assert!(
            body.to_lowercase().contains("scalar"),
            "el html no parece la UI de Scalar"
        );
    }

    #[tokio::test]
    async fn el_documento_openapi_es_json_valido() {
        let (status, content_type, body) = pedir(&router(), "/api/docs/openapi.json").await;

        assert_eq!(status, StatusCode::OK);
        assert!(
            content_type
                .as_deref()
                .is_some_and(|valor| valor.starts_with("application/json")),
            "content-type inesperado: {content_type:?}"
        );

        let documento: Value = serde_json::from_str(&body).expect("documento no es JSON");
        let version = documento["openapi"]
            .as_str()
            .expect("falta el campo openapi");
        assert!(
            version.starts_with("3."),
            "versión OpenAPI inesperada: {version}"
        );

        let paths = documento["paths"].as_object().expect("falta paths");
        for ruta in ["/api/health", "/api/v1/reportes", "/api/v1/actividad/ws"] {
            assert!(paths.contains_key(ruta), "falta la ruta {ruta}");
        }
    }

    #[tokio::test]
    async fn el_documento_incluye_los_esquemas_esperados() {
        let (_, _, body) = pedir(&router(), "/api/docs/openapi.json").await;
        let documento: Value = serde_json::from_str(&body).expect("documento no es JSON");
        let esquemas = documento["components"]["schemas"]
            .as_object()
            .expect("faltan components.schemas");

        for nombre in [
            "SolicitudCrearReporte",
            "ResultadoConsulta",
            "EventoActividad",
        ] {
            assert!(esquemas.contains_key(nombre), "falta el esquema {nombre}");
        }
    }

    #[tokio::test]
    async fn con_la_doc_deshabilitada_las_rutas_responden_404() {
        let app = router_opcional(false);

        let (status, _, _) = pedir(&app, "/api/docs").await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _, _) = pedir(&app, "/api/docs/openapi.json").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
