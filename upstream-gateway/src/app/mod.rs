use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use axum::Router;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::config::AppConfig;
use crate::runtime::{InMemoryUpstreamRateLimiter, SharedRateLimiter};
use crate::storage::{SharedGatewayConfigStore, SqliteGatewayConfigStore, load_provider_bundles};

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub http_client: reqwest::Client,
    pub config_store: SharedGatewayConfigStore,
    pub rate_limiter: SharedRateLimiter,
}

impl AppState {
    pub async fn new(config: AppConfig) -> anyhow::Result<Self> {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .build()
            .context("failed to build reqwest client")?;
        let sqlite_store = SqliteGatewayConfigStore::connect(&config.database_url).await?;
        if let Some(path) = config.bootstrap_json_path.as_deref() {
            let bundles = load_provider_bundles(std::path::Path::new(path)).await?;
            let seeded = sqlite_store.seed_if_empty(&bundles).await?;
            if seeded > 0 {
                info!(
                    "seeded sqlite config store with {} provider bundle(s) from {}",
                    seeded, path
                );
            }
        }
        let config_store: SharedGatewayConfigStore = Arc::new(sqlite_store);
        let rate_limiter: SharedRateLimiter = Arc::new(InMemoryUpstreamRateLimiter::default());

        Ok(Self {
            config,
            http_client,
            config_store,
            rate_limiter,
        })
    }
}

pub fn build_proxy_router(state: AppState) -> Router {
    let shared_state = Arc::new(state);

    Router::new()
        .merge(crate::control_plane::proxy_router())
        .merge(crate::data_plane::router())
        .layer(TraceLayer::new_for_http())
        .with_state(shared_state)
}

pub fn build_admin_router(state: AppState) -> Router {
    let shared_state = Arc::new(state);

    Router::new()
        .merge(crate::web::router())
        .merge(crate::control_plane::admin_router())
        .layer(TraceLayer::new_for_http())
        .with_state(shared_state)
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use tower::util::ServiceExt;

    use super::{AppState, build_admin_router, build_proxy_router};
    use crate::config::AppConfig;
    use crate::runtime::{InMemoryUpstreamRateLimiter, SharedRateLimiter};
    use crate::storage::{InMemoryGatewayConfigStore, SharedGatewayConfigStore};
    use std::sync::Arc;

    #[tokio::test]
    async fn proxy_router_exposes_health_but_not_admin_shell() {
        let app = build_proxy_router(test_state());

        let health = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        let body = to_bytes(health.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("proxy_bind_addr"));

        let admin = app
            .oneshot(
                Request::builder()
                    .uri("/admin")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(admin.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn admin_router_exposes_admin_shell_but_not_provider_data_plane() {
        let app = build_admin_router(test_state());

        let admin = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(admin.status(), StatusCode::OK);

        let provider = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/providers/test/openai/v1/chat/completions")
                    .body(axum::body::Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(provider.status(), StatusCode::NOT_FOUND);
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
