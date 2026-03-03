use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use super::auth::{self, AuthKey};
use super::handler;
use crate::Gateway;

pub fn create_router(gateway: Gateway) -> Router {
    let mut router = Router::new()
        .route("/v1/chat/completions", post(handler::openai_proxy))
        .route("/health", get(health));

    if let Some(ref key) = gateway.config.auth_key {
        if !key.is_empty() {
            router = router
                .layer(axum::Extension(AuthKey(key.clone())))
                .layer(middleware::from_fn(auth::bearer_auth));
        }
    }

    router
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(gateway)
}

async fn health() -> &'static str {
    r#"{"status":"ok"}"#
}
