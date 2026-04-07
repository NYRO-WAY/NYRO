use axum::routing::{get, post};
use axum::Router;
use axum::http::{HeaderValue, Method, header};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

use super::handler;
use crate::Gateway;

pub fn create_router(gateway: Gateway) -> Router {
    let router = Router::new()
        .route("/v1/chat/completions", post(handler::openai_proxy))
        .route("/chat/completions", post(handler::openai_proxy))
        .route("/v1/responses", post(handler::responses_proxy))
        .route("/responses", post(handler::responses_proxy))
        .route("/v1/messages", post(handler::anthropic_proxy))
        .route("/messages", post(handler::anthropic_proxy))
        .route("/v1/embeddings", post(handler::embeddings_proxy))
        .route("/embeddings", post(handler::embeddings_proxy))
        .route("/v1beta/models/:model_action",post(handler::gemini_proxy))
        .route("/models/:model_action", post(handler::gemini_proxy))
        .route("/v1/models", get(handler::models_list))
        .route("/models", get(handler::models_list))
        .route("/health", get(health));

    let cors = build_proxy_cors_layer(&gateway.config.proxy_cors_origins, gateway.config.proxy_port);

    router
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(gateway)
}

async fn health() -> &'static str {
    r#"{"status":"ok"}"#
}

fn build_proxy_cors_layer(origins: &[String], proxy_port: u16) -> CorsLayer {
    let source_origins = if origins.is_empty() {
        default_proxy_origins(proxy_port)
    } else {
        origins.to_vec()
    };

    CorsLayer::new()
        .allow_origin(parse_allow_origin(&source_origins))
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ACCEPT,
            header::HeaderName::from_static("x-api-key"),
            header::HeaderName::from_static("x-nyro-cache"),
            header::HeaderName::from_static("x-nyro-cache-ttl"),
            header::HeaderName::from_static("anthropic-version"),
        ])
}

fn default_proxy_origins(proxy_port: u16) -> Vec<String> {
    vec![
        format!("http://127.0.0.1:{proxy_port}"),
        format!("http://localhost:{proxy_port}"),
        "tauri://localhost".to_string(),
        "http://tauri.localhost".to_string(),
    ]
}

fn parse_allow_origin(origins: &[String]) -> AllowOrigin {
    if origins.iter().any(|o| o.trim() == "*") {
        return AllowOrigin::any();
    }

    let values = origins
        .iter()
        .filter_map(|o| HeaderValue::from_str(o.trim()).ok())
        .collect::<Vec<_>>();

    if values.is_empty() {
        AllowOrigin::any()
    } else {
        AllowOrigin::list(values)
    }
}
