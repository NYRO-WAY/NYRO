use std::sync::Arc;

use axum::Router;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use include_dir::{Dir, include_dir};

use crate::app::AppState;

static WEB_DIST: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/webui/dist");

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(root_redirect))
        .route("/admin", get(admin_index))
        .route("/admin/", get(admin_index))
        .route("/admin/assets/*path", get(admin_asset))
}

async fn root_redirect() -> Response {
    (
        StatusCode::TEMPORARY_REDIRECT,
        [(header::LOCATION, HeaderValue::from_static("/admin"))],
    )
        .into_response()
}

async fn admin_index() -> Response {
    serve_asset("index.html")
}

async fn admin_asset(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    let normalized = format!("assets/{}", path.trim_start_matches('/'));
    serve_asset(&normalized)
}

fn serve_asset(path: &str) -> Response {
    let Some(file) = WEB_DIST.get_file(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_str(mime.as_ref())
                .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
        )],
        file.contents(),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use tower::util::ServiceExt;

    use crate::app::AppState;
    use crate::config::AppConfig;
    use crate::runtime::{InMemoryUpstreamRateLimiter, SharedRateLimiter};
    use crate::storage::{InMemoryGatewayConfigStore, SharedGatewayConfigStore};

    use super::router;

    #[tokio::test]
    async fn admin_route_serves_html_shell() {
        let app = router().with_state(Arc::new(test_state()));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/admin")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("<div id=\"root\"></div>"));
        assert!(text.contains("/admin/assets/"));
    }

    #[tokio::test]
    async fn asset_route_serves_built_dist_file() {
        let app = router().with_state(Arc::new(test_state()));
        let index = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(index.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();

        let marker = "/admin/assets/";
        let start = text.find(marker).unwrap();
        let rest = &text[start + marker.len()..];
        let end = rest.find('"').unwrap();
        let asset_uri = format!("/admin/assets/{}", &rest[..end]);

        let asset = app
            .oneshot(
                Request::builder()
                    .uri(asset_uri)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(asset.status(), StatusCode::OK);
        assert!(asset.headers().get("content-type").is_some());
    }

    fn test_state() -> AppState {
        AppState {
            config: AppConfig::default(),
            http_client: reqwest::Client::new(),
            config_store: Arc::new(InMemoryGatewayConfigStore::default())
                as SharedGatewayConfigStore,
            rate_limiter: Arc::new(InMemoryUpstreamRateLimiter::default()) as SharedRateLimiter,
        }
    }
}
