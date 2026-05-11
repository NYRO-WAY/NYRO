mod request;
mod runner;

use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, Response, StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use bytes::Bytes;
use futures_util::StreamExt;
use serde::Serialize;

use crate::app::AppState;
use crate::domain::DownstreamRouteKind;

use self::request::ProxyRouteRequest;
use self::runner::RaceRunner;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route(
            "/groups/:group_id/openai/v1/chat/completions",
            post(openai_chat_completions),
        )
        .route(
            "/groups/:group_id/openai/v1/responses",
            post(openai_responses),
        )
        .route(
            "/groups/:group_id/anthropic/v1/messages",
            post(anthropic_messages),
        )
        .route(
            "/groups/:group_id/google/v1beta/models/:model_action",
            post(google_v1beta_models),
        )
        .route(
            "/groups/:group_id/google/models/:model_action",
            post(google_v1_models),
        )
        .with_state(state)
}

async fn healthz(State(state): State<AppState>) -> axum::Json<HealthResponse> {
    let bind_addr = state.config.proxy_bind_addr.clone();
    axum::Json(HealthResponse {
        status: "ok",
        service: "race-gateway",
        bind_addr: bind_addr.clone(),
        proxy_bind_addr: bind_addr,
    })
}

async fn openai_chat_completions(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response<Body>, ProxyError> {
    proxy_route(
        state,
        ProxyRouteRequest::new(
            group_id,
            DownstreamRouteKind::OpenAiChatCompletions,
            None,
            headers,
            body,
        ),
    )
    .await
}

async fn openai_responses(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response<Body>, ProxyError> {
    proxy_route(
        state,
        ProxyRouteRequest::new(
            group_id,
            DownstreamRouteKind::OpenAiResponses,
            None,
            headers,
            body,
        ),
    )
    .await
}

async fn anthropic_messages(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response<Body>, ProxyError> {
    proxy_route(
        state,
        ProxyRouteRequest::new(
            group_id,
            DownstreamRouteKind::AnthropicMessages,
            None,
            headers,
            body,
        ),
    )
    .await
}

async fn google_v1beta_models(
    State(state): State<AppState>,
    Path((group_id, model_action)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response<Body>, ProxyError> {
    proxy_route(
        state,
        ProxyRouteRequest::new(
            group_id,
            DownstreamRouteKind::GoogleV1BetaModels,
            Some(model_action),
            headers,
            body,
        ),
    )
    .await
}

async fn google_v1_models(
    State(state): State<AppState>,
    Path((group_id, model_action)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response<Body>, ProxyError> {
    proxy_route(
        state,
        ProxyRouteRequest::new(
            group_id,
            DownstreamRouteKind::GoogleV1Models,
            Some(model_action),
            headers,
            body,
        ),
    )
    .await
}

async fn proxy_route(
    state: AppState,
    request: ProxyRouteRequest,
) -> Result<Response<Body>, ProxyError> {
    let runner = RaceRunner::new(state.clone());
    let execution = runner
        .race_stream(request)
        .await
        .map_err(ProxyError::internal)?;

    let stream = execution.stream.map(Ok::<Bytes, std::convert::Infallible>);
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, execution.content_type);
    if let Some(diagnostics) = execution.diagnostics_header_value {
        response = response.header(crate::group::RACE_DIAGNOSTICS_HEADER, diagnostics);
    }
    response
        .body(Body::from_stream(stream))
        .map_err(|error| ProxyError::internal(anyhow::Error::new(error)))
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
    bind_addr: String,
    proxy_bind_addr: String,
}

#[derive(Debug)]
struct ProxyError {
    status: StatusCode,
    message: String,
}

impl ProxyError {
    fn internal(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response<Body> {
        let body = serde_json::json!({
            "error": {
                "code": self.status.as_u16(),
                "message": self.message,
                "type": "RACE_GATEWAY_ERROR"
            }
        });
        Response::builder()
            .status(self.status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("build proxy error response")
    }
}
