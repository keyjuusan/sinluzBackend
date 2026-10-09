use axum::http::{HeaderValue, Method, header};
use tower_http::cors::{AllowOrigin, CorsLayer};

pub fn capa(origen: HeaderValue) -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::list([origen]))
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE])
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::Body,
        http::{HeaderValue, Request, StatusCode, header},
        routing::get,
    };
    use tower::ServiceExt;

    use super::capa;

    const ORIGEN_PERMITIDO: &str = "http://localhost:5173";

    fn aplicacion() -> Router {
        Router::new()
            .route("/", get(|| async { StatusCode::OK }))
            .layer(capa(HeaderValue::from_static(ORIGEN_PERMITIDO)))
    }

    #[tokio::test]
    async fn preflight_con_origen_permitido_devuelve_cabeceras_cors() {
        let peticion = Request::builder()
            .method("OPTIONS")
            .uri("/")
            .header(header::ORIGIN, ORIGEN_PERMITIDO)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .body(Body::empty())
            .expect("petición válida");

        let respuesta = aplicacion().oneshot(peticion).await.expect("respuesta");

        assert!(
            respuesta.status() == StatusCode::OK || respuesta.status() == StatusCode::NO_CONTENT
        );
        assert_eq!(
            respuesta
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .map(|valor| valor.to_str().expect("origen ASCII")),
            Some(ORIGEN_PERMITIDO)
        );
        assert!(
            respuesta
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_METHODS)
                .is_some()
        );
        assert_eq!(
            respuesta
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
                .map(|valor| valor.to_str().expect("cabeceras ASCII")),
            Some("content-type")
        );
    }

    #[tokio::test]
    async fn peticion_con_origen_permitido_recibe_la_cabecera() {
        let peticion = Request::builder()
            .uri("/")
            .header(header::ORIGIN, ORIGEN_PERMITIDO)
            .body(Body::empty())
            .expect("petición válida");

        let respuesta = aplicacion().oneshot(peticion).await.expect("respuesta");

        assert_eq!(respuesta.status(), StatusCode::OK);
        assert_eq!(
            respuesta
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .map(|valor| valor.to_str().expect("origen ASCII")),
            Some(ORIGEN_PERMITIDO)
        );
    }

    #[tokio::test]
    async fn peticion_con_origen_distinto_no_recibe_cabecera() {
        let peticion = Request::builder()
            .uri("/")
            .header(header::ORIGIN, "http://malicioso.example")
            .body(Body::empty())
            .expect("petición válida");

        let respuesta = aplicacion().oneshot(peticion).await.expect("respuesta");

        assert!(
            respuesta
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none()
        );
    }

    #[tokio::test]
    async fn preflight_con_origen_distinto_no_recibe_cabecera() {
        let peticion = Request::builder()
            .method("OPTIONS")
            .uri("/")
            .header(header::ORIGIN, "http://malicioso.example")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .body(Body::empty())
            .expect("petición válida");

        let respuesta = aplicacion().oneshot(peticion).await.expect("respuesta");

        assert!(
            respuesta
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none()
        );
    }
}
