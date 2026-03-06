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
        .route("/v1/messages", post(handler::anthropic_proxy))
        .route(
            "/v1beta/models/:model_action",
            post(handler::gemini_proxy),
        )
        .route("/health", get(health));

    if let Some(ref key) = gateway.config.auth_key {
        if !key.is_empty() {
            router = router
                .layer(middleware::from_fn(auth::bearer_auth))
                .layer(axum::Extension(AuthKey(key.clone())));
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
