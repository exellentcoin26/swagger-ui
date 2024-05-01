use axum::extract::OriginalUri;
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::{headers::ContentType, typed_header::TypedHeader};
use std::sync::Arc;
use swagger_ui::{Assets, Config, SpecOrUrl};

/// Helper trait to allow `route.swagger_ui_route(...)`
pub trait SwaggerUiExt {
    fn swagger_ui(
        self,
        path: &str,
        spec: impl Into<SpecOrUrl>,
        config: impl Into<Option<Config>>,
    ) -> Self;
}

impl<S> SwaggerUiExt for Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    fn swagger_ui(
        self,
        path: &str,
        spec: impl Into<SpecOrUrl>,
        config: impl Into<Option<Config>>,
    ) -> Self {
        self.nest(path, swagger_ui_route(spec, config))
    }
}

/// creates a route that is configured to serve the specified spec and config with swagger_ui
pub fn swagger_ui_route<S>(
    spec: impl Into<SpecOrUrl>,
    config: impl Into<Option<Config>>,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let config = Arc::new(config.into().unwrap_or_default());
    let spec = Arc::new(spec.into());
    Router::new().route("/", get(redirect_index)).route(
        "/*path",
        get(move |uri: Uri, original: OriginalUri| {
            let config = config.clone();
            let spec = spec.clone();
            async move { handle_path(uri, original, &spec, &config).await }
        }),
    )
}

async fn redirect_index(uri: OriginalUri) -> Redirect {
    let p = uri.path().trim_end_matches("/");
    let query = uri.query();
    Redirect::permanent(&if let Some(q) = query {
        format!("{p}/index.html?{q}")
    } else {
        format!("{p}/index.html")
    })
}

fn mime_type(filename: &str) -> TypedHeader<ContentType> {
    TypedHeader(ContentType::from(
        mime_guess::from_ext(filename.split(".").last().unwrap_or_default())
            .first_or_octet_stream(),
    ))
}

async fn handle_path(
    uri: Uri,
    original: OriginalUri,
    spec: &SpecOrUrl,
    config: &Config,
) -> Response {
    let path = uri.path().trim_start_matches("/");
    if let Some(asset) = Assets::get(path) {
        let t = mime_type(path);
        return (t, asset).into_response();
    }
    if path == "swagger-ui-config.json" {
        let mut config = config.clone();
        match spec {
            SpecOrUrl::Spec(spec) => {
                config.url = original
                    .path()
                    .replace("swagger-ui-config.json", &spec.name)
            }
            SpecOrUrl::Url(url) => config.url = url.to_string(),
        }
        return Json(config).into_response();
    }
    if let SpecOrUrl::Spec(spec) = spec {
        if path == spec.name.trim_start_matches("/") {
            return (TypedHeader(ContentType::json()), spec.content.clone()).into_response();
        }
    }
    StatusCode::NOT_FOUND.into_response()
}

#[cfg(test)]
mod tests {
    use crate::swagger_ui_route;
    use axum::body::Body;
    use axum::http::header::CONTENT_TYPE;
    use axum::http::{Method, Request, StatusCode};
    use axum::Router;
    use axum_extra::headers::ContentType;
    use swagger_ui::Config;
    use tower::Service;
    use tower::ServiceExt;

    fn app() -> Router {
        swagger_ui_route(
            swagger_ui::swagger_spec_file!("../../swagger-ui/examples/openapi.json"),
            None,
        )
    }

    #[tokio::test]
    async fn does_redirect() {
        let app = app();
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
    }

    #[tokio::test]
    async fn does_index() {
        let app = app();

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/index.html")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let header: ContentType = response
            .headers()
            .get(CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(header, ContentType::html());
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn does_config() {
        let app = app();

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/swagger-ui-config.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let header: ContentType = response
            .headers()
            .get(CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(header, ContentType::json());
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let config: Config =
            serde_json::from_str(std::str::from_utf8(body.as_ref()).unwrap()).unwrap();
    }
}
