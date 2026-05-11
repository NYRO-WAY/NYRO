mod request;
mod usage;

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use futures_util::StreamExt;
use serde_json::{Value, json};
use tracing::{error, warn};

use crate::app::AppState;
use crate::errors::RateLimitError;
use crate::estimator;
use crate::provider::{ProviderVendor, UpstreamProtocol};
use crate::runtime::{SelectedUpstreamKey, SettlementUsage};
use crate::storage::SharedProviderBundle;
use crate::upstream::{
    OutboundRequestParts, OutboundRequestPreview, UpstreamRouteKind, build_upstream_request,
    preview_request,
};
use request::extract_request_metadata;
use usage::{StreamUsageTracker, parse_settlement_usage};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/providers/:provider_id/openai/v1/chat/completions",
            post(openai_chat),
        )
        .route(
            "/providers/:provider_id/openai/v1/responses",
            post(openai_responses),
        )
        .route(
            "/providers/:provider_id/openai/v1/embeddings",
            post(openai_embeddings),
        )
        .route(
            "/providers/:provider_id/anthropic/v1/messages",
            post(anthropic_messages),
        )
        .route(
            "/providers/:provider_id/google/v1beta/models/:model_action",
            post(google_generate_v1beta),
        )
        .route(
            "/providers/:provider_id/google/models/:model_action",
            post(google_generate_v1),
        )
}

async fn openai_chat(
    State(state): State<Arc<AppState>>,
    Path(provider_id): Path<String>,
    body: Bytes,
) -> Response {
    handle_native_route(
        state,
        "openai_chat",
        provider_id,
        None,
        UpstreamProtocol::OpenAIChatCompletions,
        UpstreamRouteKind::OpenAIChatCompletions,
        body,
    )
    .await
}

async fn openai_responses(
    State(state): State<Arc<AppState>>,
    Path(provider_id): Path<String>,
    body: Bytes,
) -> Response {
    handle_native_route(
        state,
        "openai_responses",
        provider_id,
        None,
        UpstreamProtocol::OpenAIResponses,
        UpstreamRouteKind::OpenAIResponses,
        body,
    )
    .await
}

async fn openai_embeddings(
    State(state): State<Arc<AppState>>,
    Path(provider_id): Path<String>,
    body: Bytes,
) -> Response {
    handle_native_route(
        state,
        "openai_embeddings",
        provider_id,
        None,
        UpstreamProtocol::OpenAIEmbeddings,
        UpstreamRouteKind::OpenAIEmbeddings,
        body,
    )
    .await
}

async fn anthropic_messages(
    State(state): State<Arc<AppState>>,
    Path(provider_id): Path<String>,
    body: Bytes,
) -> Response {
    handle_native_route(
        state,
        "anthropic_messages",
        provider_id,
        None,
        UpstreamProtocol::AnthropicMessages,
        UpstreamRouteKind::AnthropicMessages,
        body,
    )
    .await
}

async fn google_generate_v1beta(
    State(state): State<Arc<AppState>>,
    Path((provider_id, model_action)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    google_generate_internal(
        state,
        provider_id,
        model_action,
        UpstreamRouteKind::GoogleV1BetaModels,
        body,
    )
    .await
}

async fn google_generate_v1(
    State(state): State<Arc<AppState>>,
    Path((provider_id, model_action)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    google_generate_internal(
        state,
        provider_id,
        model_action,
        UpstreamRouteKind::GoogleModels,
        body,
    )
    .await
}

async fn google_generate_internal(
    state: Arc<AppState>,
    provider_id: String,
    model_action: String,
    route_kind: UpstreamRouteKind,
    body: Bytes,
) -> Response {
    let upstream_protocol = if model_action.contains(":streamGenerateContent") {
        UpstreamProtocol::GoogleStreamGenerateContent
    } else {
        UpstreamProtocol::GoogleGenerateContent
    };

    handle_native_route(
        state,
        "google_generate",
        provider_id,
        Some(model_action),
        upstream_protocol,
        route_kind,
        body,
    )
    .await
}

async fn handle_native_route(
    state: Arc<AppState>,
    route: &str,
    provider_id: String,
    model_action: Option<String>,
    upstream_protocol: UpstreamProtocol,
    route_kind: UpstreamRouteKind,
    body: Bytes,
) -> Response {
    let parsed_body = match serde_json::from_slice::<Value>(&body) {
        Ok(value) => value,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "message": format!("request body must be valid json: {error}"),
                        "type": "invalid_request",
                    },
                    "route": route,
                    "provider_id": provider_id,
                })),
            )
                .into_response();
        }
    };

    let metadata =
        match extract_request_metadata(upstream_protocol, model_action.as_deref(), &parsed_body) {
            Ok(metadata) => metadata,
            Err(message) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": {
                            "message": message,
                            "type": "invalid_request",
                        },
                        "route": route,
                        "provider_id": provider_id,
                        "model_action": model_action,
                    })),
                )
                    .into_response();
            }
        };

    let bundle = match load_provider_bundle(&state, &provider_id, upstream_protocol).await {
        Ok(bundle) => bundle,
        Err(response) => return response,
    };
    let rate_limit_config = match bundle.normalized_rate_limit_config() {
        Ok(config) => config,
        Err(error) => {
            return map_rate_limit_error(route, &provider_id, model_action.as_ref(), error);
        }
    };
    let matched_rule = rate_limit_config.matching_rule(&metadata.actual_model);

    let request_input_tokens = if rate_limit_config.needs_tpm_estimation(&metadata.actual_model) {
        match estimator::estimate_input_tokens(
            metadata.upstream_protocol,
            &metadata.actual_model,
            &parsed_body,
            matched_rule,
        ) {
            Ok(tokens) => tokens,
            Err(error) => {
                return map_rate_limit_error(route, &provider_id, model_action.as_ref(), error);
            }
        }
    } else {
        0
    };

    let selected = match state
        .rate_limiter
        .acquire(
            &bundle.provider,
            rate_limit_config,
            &metadata.actual_model,
            request_input_tokens,
            metadata.request_output_reservation,
        )
        .await
    {
        Ok(selected) => selected,
        Err(error) => {
            return map_rate_limit_error(route, &provider_id, model_action.as_ref(), error);
        }
    };

    let (outbound_request, outbound_preview) = match build_outbound_request(
        &state,
        &bundle,
        &selected,
        route_kind,
        model_action.as_deref(),
        metadata.stream,
        body,
    ) {
        Ok(value) => value,
        Err(error) => {
            rollback_lease(&state, &selected, route, &provider_id).await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": {
                        "message": format!("upstream request build failed: {error}"),
                        "type": "internal_error",
                    },
                    "route": route,
                    "provider_id": provider_id,
                    "model_action": model_action,
                })),
            )
                .into_response();
        }
    };

    if metadata.stream {
        return dispatch_stream_request(
            &state,
            route,
            provider_id,
            model_action,
            upstream_protocol,
            selected,
            outbound_request,
            outbound_preview,
        )
        .await;
    }

    dispatch_non_stream_request(
        &state,
        route,
        provider_id,
        model_action,
        upstream_protocol,
        selected,
        outbound_request,
        outbound_preview,
    )
    .await
}

fn build_outbound_request(
    state: &AppState,
    bundle: &SharedProviderBundle,
    selected: &SelectedUpstreamKey,
    route_kind: UpstreamRouteKind,
    model_action: Option<&str>,
    stream: bool,
    body: Bytes,
) -> anyhow::Result<(reqwest::Request, OutboundRequestPreview)> {
    let request = build_upstream_request(
        &state.http_client,
        &bundle.provider,
        selected,
        OutboundRequestParts {
            route_kind,
            model_action: model_action.map(ToString::to_string),
            stream,
            body,
        },
    )?;
    let preview = preview_request(&request);
    Ok((request, preview))
}

async fn load_provider_bundle(
    state: &AppState,
    provider_id: &str,
    upstream_protocol: UpstreamProtocol,
) -> Result<SharedProviderBundle, Response> {
    let Some(bundle) = state
        .config_store
        .get_provider_bundle_shared(provider_id)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": {
                        "message": format!("provider lookup failed: {error}"),
                        "type": "internal_error",
                    },
                    "provider_id": provider_id,
                })),
            )
                .into_response()
        })?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": {
                    "message": format!("provider '{provider_id}' was not found"),
                    "type": "not_found",
                },
                "provider_id": provider_id,
            })),
        )
            .into_response());
    };

    let expected_vendor = vendor_for_protocol(upstream_protocol);
    if bundle.provider.vendor != expected_vendor {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "message": format!(
                        "provider '{}' vendor mismatch: expected {:?}, got {:?}",
                        provider_id,
                        expected_vendor,
                        bundle.provider.vendor
                    ),
                    "type": "invalid_request",
                },
                "provider_id": provider_id,
            })),
        )
            .into_response());
    }
    if !bundle.provider.enabled {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": {
                    "message": format!("provider '{provider_id}' is disabled"),
                    "type": "temporary_unavailable",
                },
                "provider_id": provider_id,
            })),
        )
            .into_response());
    }

    Ok(bundle)
}

fn map_rate_limit_error(
    route: &str,
    provider_id: &str,
    model_action: Option<&String>,
    error: RateLimitError,
) -> Response {
    let (status, error_type) = match error {
        RateLimitError::NoAvailableKey { .. } => {
            (StatusCode::SERVICE_UNAVAILABLE, "temporary_unavailable")
        }
        RateLimitError::InvalidConfig { .. }
        | RateLimitError::LeaseNotFound { .. }
        | RateLimitError::TokenEstimation { .. } => {
            (StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
        }
    };

    (
        status,
        Json(json!({
            "error": {
                "message": error.to_string(),
                "type": error_type,
            },
            "route": route,
            "provider_id": provider_id,
            "model_action": model_action,
        })),
    )
        .into_response()
}

fn vendor_for_protocol(protocol: UpstreamProtocol) -> ProviderVendor {
    match protocol {
        UpstreamProtocol::OpenAIChatCompletions
        | UpstreamProtocol::OpenAIResponses
        | UpstreamProtocol::OpenAIEmbeddings => ProviderVendor::OpenAI,
        UpstreamProtocol::AnthropicMessages => ProviderVendor::Anthropic,
        UpstreamProtocol::GoogleGenerateContent | UpstreamProtocol::GoogleStreamGenerateContent => {
            ProviderVendor::Gemini
        }
    }
}

async fn dispatch_non_stream_request(
    state: &AppState,
    route: &str,
    provider_id: String,
    model_action: Option<String>,
    upstream_protocol: UpstreamProtocol,
    selected: SelectedUpstreamKey,
    outbound_request: reqwest::Request,
    outbound_preview: OutboundRequestPreview,
) -> Response {
    let response = match state.http_client.execute(outbound_request).await {
        Ok(response) => response,
        Err(error) => {
            rollback_lease(state, &selected, route, &provider_id).await;
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "error": {
                        "message": format!("upstream request failed: {error}"),
                        "type": "bad_gateway",
                    },
                    "route": route,
                    "provider_id": provider_id,
                    "model_action": model_action,
                    "outbound_request": outbound_preview,
                })),
            )
                .into_response();
        }
    };

    let status = response.status();
    let headers = response.headers().clone();
    let body = match response.bytes().await {
        Ok(body) => body,
        Err(error) => {
            settle_lease(state, &selected, route, &provider_id).await;
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "error": {
                        "message": format!("upstream response read failed: {error}"),
                        "type": "bad_gateway",
                    },
                    "route": route,
                    "provider_id": provider_id,
                    "model_action": model_action,
                    "outbound_request": outbound_preview,
                })),
            )
                .into_response();
        }
    };

    let settlement_usage = match serde_json::from_slice::<Value>(&body) {
        Ok(parsed) => parse_settlement_usage(upstream_protocol, &parsed)
            .unwrap_or_else(|| conservative_settlement_usage(&selected.lease)),
        Err(error) => {
            warn!(
                "non-stream usage parse skipped for route {} provider {}: {}",
                route, provider_id, error
            );
            conservative_settlement_usage(&selected.lease)
        }
    };

    settle_lease_with_usage(state, &selected, route, &provider_id, settlement_usage).await;

    let mut builder = axum::http::Response::builder().status(status);
    for (name, value) in &headers {
        if should_forward_response_header(name.as_str()) {
            builder = builder.header(name, value);
        }
    }

    builder
        .body(axum::body::Body::from(body))
        .unwrap_or_else(|error| {
            error!("response build failed for route {route}: {error}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": {
                        "message": "failed to build downstream response",
                        "type": "internal_error",
                    },
                    "route": route,
                    "provider_id": provider_id,
                    "model_action": model_action,
                })),
            )
                .into_response()
        })
}

async fn dispatch_stream_request(
    state: &AppState,
    route: &str,
    provider_id: String,
    model_action: Option<String>,
    upstream_protocol: UpstreamProtocol,
    selected: SelectedUpstreamKey,
    outbound_request: reqwest::Request,
    outbound_preview: OutboundRequestPreview,
) -> Response {
    let response = match state.http_client.execute(outbound_request).await {
        Ok(response) => response,
        Err(error) => {
            rollback_lease(state, &selected, route, &provider_id).await;
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "error": {
                        "message": format!("upstream request failed: {error}"),
                        "type": "bad_gateway",
                    },
                    "route": route,
                    "provider_id": provider_id,
                    "model_action": model_action,
                    "outbound_request": outbound_preview,
                })),
            )
                .into_response();
        }
    };

    let status = response.status();
    let headers = response.headers().clone();
    let mut upstream_stream = response.bytes_stream();
    let rate_limiter = state.rate_limiter.clone();
    let lease = selected.lease.clone();
    let route_owned = route.to_string();
    let provider_id_owned = provider_id.clone();
    let mut usage_tracker = StreamUsageTracker::new(upstream_protocol);

    let body_stream = async_stream::stream! {
        while let Some(item) = upstream_stream.next().await {
            match item {
                Ok(chunk) => {
                    usage_tracker.observe_chunk(&chunk);
                    yield Ok::<Bytes, std::io::Error>(chunk);
                }
                Err(error) => {
                    let usage = usage_tracker
                        .finish()
                        .unwrap_or_else(|| conservative_settlement_usage(&lease));
                    settle_lease_with_limiter(
                        &rate_limiter,
                        &lease,
                        &route_owned,
                        &provider_id_owned,
                        usage,
                    )
                    .await;
                    yield Err(std::io::Error::other(error));
                    return;
                }
            }
        }

        let usage = usage_tracker
            .finish()
            .unwrap_or_else(|| conservative_settlement_usage(&lease));
        settle_lease_with_limiter(
            &rate_limiter,
            &lease,
            &route_owned,
            &provider_id_owned,
            usage,
        )
        .await;
    };

    let mut builder = axum::http::Response::builder().status(status);
    for (name, value) in &headers {
        if should_forward_response_header(name.as_str()) {
            builder = builder.header(name, value);
        }
    }

    builder
        .body(Body::from_stream(body_stream))
        .unwrap_or_else(|error| {
            error!("stream response build failed for route {route}: {error}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": {
                        "message": "failed to build downstream stream response",
                        "type": "internal_error",
                    },
                    "route": route,
                    "provider_id": provider_id,
                    "model_action": model_action,
                    "outbound_request": outbound_preview,
                })),
            )
                .into_response()
        })
}

async fn rollback_lease(
    state: &AppState,
    selected: &SelectedUpstreamKey,
    route: &str,
    provider_id: &str,
) {
    if let Err(error) = state.rate_limiter.rollback(&selected.lease).await {
        warn!(
            "rollback lease failed for route {} provider {}: {}",
            route, provider_id, error
        );
    }
}

async fn settle_lease(
    state: &AppState,
    selected: &SelectedUpstreamKey,
    route: &str,
    provider_id: &str,
) {
    settle_lease_with_limiter(
        &state.rate_limiter,
        &selected.lease,
        route,
        provider_id,
        conservative_settlement_usage(&selected.lease),
    )
    .await;
}

async fn settle_lease_with_usage(
    state: &AppState,
    selected: &SelectedUpstreamKey,
    route: &str,
    provider_id: &str,
    usage: SettlementUsage,
) {
    settle_lease_with_limiter(
        &state.rate_limiter,
        &selected.lease,
        route,
        provider_id,
        usage,
    )
    .await;
}

fn should_forward_response_header(name: &str) -> bool {
    !matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "content-length"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

async fn settle_lease_with_limiter(
    rate_limiter: &crate::runtime::SharedRateLimiter,
    lease: &crate::runtime::RateLimitLease,
    route: &str,
    provider_id: &str,
    usage: SettlementUsage,
) {
    if let Err(error) = rate_limiter.settle(lease, usage).await {
        warn!(
            "settle lease failed for route {} provider {}: {}",
            route, provider_id, error
        );
    }
}

fn conservative_settlement_usage(lease: &crate::runtime::RateLimitLease) -> SettlementUsage {
    SettlementUsage {
        input_tokens: lease.reserved_input_tokens,
        output_tokens: lease.reserved_output_tokens,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::{Body, Bytes, to_bytes};
    use axum::extract::State;
    use axum::http::HeaderMap;
    use axum::response::IntoResponse;
    use axum::routing::post;
    use axum::{Json, Router};
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use crate::app::AppState;
    use crate::config::{AppConfig, DailyResetConfig};
    use crate::estimator;
    use crate::provider::{
        GatewayAuthStrategy, GatewayKey, GatewayModelRule, GatewayProvider, GatewayProviderBundle,
        ProviderVendor,
    };
    use crate::runtime::{InMemoryUpstreamRateLimiter, SharedRateLimiter, UpstreamRateLimiter};
    use crate::storage::{InMemoryGatewayConfigStore, SharedGatewayConfigStore};

    use super::*;

    #[tokio::test]
    async fn stream_route_proxies_sse_and_settles_after_completion() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|headers: HeaderMap| async move {
                assert_eq!(
                    headers.get("authorization").unwrap(),
                    "Bearer sk-test-key-a"
                );
                let stream = async_stream::stream! {
                    yield Ok::<Bytes, std::io::Error>(Bytes::from_static(b"data: first\n\n"));
                    yield Ok::<Bytes, std::io::Error>(Bytes::from_static(b"data: second\n\n"));
                };
                (
                    StatusCode::OK,
                    [("content-type", "text/event-stream")],
                    Body::from_stream(stream),
                )
                    .into_response()
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let limiter = Arc::new(InMemoryUpstreamRateLimiter::default());
        let state = stream_test_state(format!("http://{}", addr), Some(1), limiter.clone());

        let response = handle_native_route(
            state.clone(),
            "openai_chat",
            "openai-prod".to_string(),
            None,
            UpstreamProtocol::OpenAIChatCompletions,
            UpstreamRouteKind::OpenAIChatCompletions,
            Bytes::from(
                serde_json::to_vec(&json!({
                    "model": "gpt-4o",
                    "stream": true,
                    "max_completion_tokens": 256
                }))
                .unwrap(),
            ),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/event-stream"
        );
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            bytes,
            Bytes::from_static(b"data: first\n\ndata: second\n\n")
        );

        let second = handle_native_route(
            state,
            "openai_chat",
            "openai-prod".to_string(),
            None,
            UpstreamProtocol::OpenAIChatCompletions,
            UpstreamRouteKind::OpenAIChatCompletions,
            Bytes::from(
                serde_json::to_vec(&json!({
                    "model": "gpt-4o",
                    "stream": true
                }))
                .unwrap(),
            ),
        )
        .await;
        assert_eq!(second.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn stream_route_settles_using_observed_usage_when_upstream_emits_it() {
        let request_body = json!({
            "model": "gpt-4o",
            "stream": true,
            "max_completion_tokens": 32,
            "messages": [
                {
                    "role": "user",
                    "content": "Summarize upstream gateway rate limiting in one sentence."
                }
            ]
        });
        let estimated_input_tokens = estimator::estimate_input_tokens(
            UpstreamProtocol::OpenAIChatCompletions,
            "gpt-4o",
            &request_body,
            None,
        )
        .unwrap();
        let tpm_limit = estimated_input_tokens.saturating_mul(2).saturating_add(40);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|headers: HeaderMap| async move {
                assert_eq!(
                    headers.get("authorization").unwrap(),
                    "Bearer sk-test-key-a"
                );
                let stream = async_stream::stream! {
                    yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                        b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
                    ));
                    yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
                        b"data: {\"usage\":{\"prompt_tokens\":0,\"completion_tokens\":3}}\n\n",
                    ));
                };
                (
                    StatusCode::OK,
                    [("content-type", "text/event-stream")],
                    Body::from_stream(stream),
                )
                    .into_response()
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let limiter = Arc::new(InMemoryUpstreamRateLimiter::default());
        let state = stream_test_state_with_rule(
            format!("http://{}", addr),
            crate::config::ModelRateLimitRule {
                model: "*".to_string(),
                rpm: None,
                rpd: None,
                tpm: Some(tpm_limit),
                tpm_mode: Some(crate::config::TpmMode::InputAndOutput),
                tokenizer_encoding: None,
                tokenizer_model: None,
            },
            limiter.clone(),
        );

        let first = handle_native_route(
            state.clone(),
            "openai_chat",
            "openai-prod".to_string(),
            None,
            UpstreamProtocol::OpenAIChatCompletions,
            UpstreamRouteKind::OpenAIChatCompletions,
            Bytes::from(serde_json::to_vec(&request_body).unwrap()),
        )
        .await;
        assert_eq!(first.status(), StatusCode::OK);
        let _ = to_bytes(first.into_body(), usize::MAX).await.unwrap();

        let bundle = state
            .config_store
            .get_provider_bundle("openai-prod")
            .await
            .unwrap()
            .unwrap();
        let snapshot = limiter
            .runtime_snapshot(&bundle.provider, &bundle.rate_limit_config())
            .unwrap();
        let model = snapshot
            .models
            .iter()
            .find(|entry| entry.model == "gpt-4o")
            .unwrap();
        let key = model
            .keys
            .iter()
            .find(|entry| entry.key_id == "key-a")
            .unwrap();
        assert_eq!(key.tpm.used, estimated_input_tokens.saturating_add(3));
        assert!(key.available);

        let second = handle_native_route(
            state,
            "openai_chat",
            "openai-prod".to_string(),
            None,
            UpstreamProtocol::OpenAIChatCompletions,
            UpstreamRouteKind::OpenAIChatCompletions,
            Bytes::from(serde_json::to_vec(&request_body).unwrap()),
        )
        .await;
        assert_eq!(second.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn stream_route_settles_conservatively_when_upstream_breaks_mid_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request_buf = [0u8; 4096];
                let _ = socket.read(&mut request_buf).await.unwrap();
                let chunk = b"data: partial\n\n";
                let header = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n{:x}\r\n",
                    chunk.len()
                );
                socket.write_all(header.as_bytes()).await.unwrap();
                socket.write_all(chunk).await.unwrap();
                socket.write_all(b"\r\n").await.unwrap();
                let _ = socket.shutdown().await;
            }
        });

        let limiter = Arc::new(InMemoryUpstreamRateLimiter::default());
        let state = stream_test_state(format!("http://{}", addr), Some(1), limiter);

        let first = handle_native_route(
            state.clone(),
            "openai_chat",
            "openai-prod".to_string(),
            None,
            UpstreamProtocol::OpenAIChatCompletions,
            UpstreamRouteKind::OpenAIChatCompletions,
            Bytes::from(
                serde_json::to_vec(&json!({
                    "model": "gpt-4o",
                    "stream": true
                }))
                .unwrap(),
            ),
        )
        .await;
        assert_eq!(first.status(), StatusCode::OK);
        let _err = to_bytes(first.into_body(), usize::MAX).await.unwrap_err();

        let second = handle_native_route(
            state,
            "openai_chat",
            "openai-prod".to_string(),
            None,
            UpstreamProtocol::OpenAIChatCompletions,
            UpstreamRouteKind::OpenAIChatCompletions,
            Bytes::from(
                serde_json::to_vec(&json!({
                    "model": "gpt-4o",
                    "stream": true
                }))
                .unwrap(),
            ),
        )
        .await;
        assert_eq!(second.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn non_stream_route_proxies_upstream_response() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/v1/chat/completions",
            post(
                |State(()): State<()>, headers: HeaderMap, body: Bytes| async move {
                    assert_eq!(
                        headers.get("authorization").unwrap(),
                        "Bearer sk-test-key-a"
                    );
                    assert_eq!(
                        serde_json::from_slice::<Value>(&body).unwrap()["model"],
                        "gpt-4o"
                    );
                    (
                        StatusCode::OK,
                        Json(json!({
                            "id": "resp-1",
                            "object": "chat.completion",
                            "choices": [],
                            "usage": {
                                "prompt_tokens": 12,
                                "completion_tokens": 4
                            }
                        })),
                    )
                },
            ),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let store = Arc::new(InMemoryGatewayConfigStore::default());
        store
            .insert(GatewayProviderBundle {
                provider: GatewayProvider {
                    id: "openai-prod".to_string(),
                    name: "OpenAI Prod".to_string(),
                    vendor: ProviderVendor::OpenAI,
                    base_url: format!("http://{}", addr),
                    auth_strategy: GatewayAuthStrategy::Bearer,
                    enabled: true,
                },
                keys: vec![GatewayKey {
                    id: "key-a".to_string(),
                    provider_id: "openai-prod".to_string(),
                    display_name: Some("Key A".to_string()),
                    api_key: "sk-test-key-a".to_string(),
                    enabled: true,
                    weight: None,
                }],
                model_rules: vec![GatewayModelRule {
                    provider_id: "openai-prod".to_string(),
                    rule: crate::config::ModelRateLimitRule {
                        model: "*".to_string(),
                        rpm: Some(10),
                        rpd: None,
                        tpm: None,
                        tpm_mode: None,
                        tokenizer_encoding: None,
                        tokenizer_model: None,
                    },
                }],
                daily_reset: DailyResetConfig {
                    timezone: "+00:00".to_string(),
                    hour: 0,
                    minute: 0,
                },
                normalized_rate_limit_config_cache: None,
            })
            .unwrap();
        let state = Arc::new(AppState {
            config: AppConfig::default(),
            http_client: reqwest::Client::new(),
            config_store: store as SharedGatewayConfigStore,
            rate_limiter: Arc::new(InMemoryUpstreamRateLimiter::default()) as SharedRateLimiter,
        });

        let response = handle_native_route(
            state,
            "openai_chat",
            "openai-prod".to_string(),
            None,
            UpstreamProtocol::OpenAIChatCompletions,
            UpstreamRouteKind::OpenAIChatCompletions,
            Bytes::from(
                serde_json::to_vec(&json!({
                    "model": "gpt-4o",
                    "stream": false,
                    "messages": []
                }))
                .unwrap(),
            ),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn route_returns_not_found_for_unknown_provider() {
        let state = Arc::new(AppState {
            config: AppConfig::default(),
            http_client: reqwest::Client::new(),
            config_store: Arc::new(InMemoryGatewayConfigStore::default())
                as SharedGatewayConfigStore,
            rate_limiter: Arc::new(InMemoryUpstreamRateLimiter::default()) as SharedRateLimiter,
        });

        let response = handle_native_route(
            state,
            "openai_chat",
            "missing".to_string(),
            None,
            UpstreamProtocol::OpenAIChatCompletions,
            UpstreamRouteKind::OpenAIChatCompletions,
            Bytes::from(
                serde_json::to_vec(&json!({
                    "model": "gpt-4o"
                }))
                .unwrap(),
            ),
        )
        .await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn google_tpm_rule_uses_estimated_input_tokens_for_admission() {
        let body = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [
                        { "text": "Explain how upstream rate limiting works in detail." }
                    ]
                }
            ]
        });
        let estimated = estimator::estimate_input_tokens(
            UpstreamProtocol::GoogleGenerateContent,
            "gemini-2.5-pro",
            &body,
            None,
        )
        .unwrap();

        let store = Arc::new(InMemoryGatewayConfigStore::default());
        store
            .insert(GatewayProviderBundle {
                provider: GatewayProvider {
                    id: "gemini-prod".to_string(),
                    name: "Gemini Prod".to_string(),
                    vendor: ProviderVendor::Gemini,
                    base_url: "https://generativelanguage.googleapis.com".to_string(),
                    auth_strategy: GatewayAuthStrategy::QueryApiKey {
                        parameter_name: "key".to_string(),
                    },
                    enabled: true,
                },
                keys: vec![GatewayKey {
                    id: "key-a".to_string(),
                    provider_id: "gemini-prod".to_string(),
                    display_name: Some("Key A".to_string()),
                    api_key: "gem-test-key-a".to_string(),
                    enabled: true,
                    weight: None,
                }],
                model_rules: vec![GatewayModelRule {
                    provider_id: "gemini-prod".to_string(),
                    rule: crate::config::ModelRateLimitRule {
                        model: "*".to_string(),
                        rpm: None,
                        rpd: None,
                        tpm: Some(estimated.saturating_sub(1).max(1)),
                        tpm_mode: Some(crate::config::TpmMode::InputOnly),
                        tokenizer_encoding: None,
                        tokenizer_model: None,
                    },
                }],
                daily_reset: DailyResetConfig {
                    timezone: "+00:00".to_string(),
                    hour: 0,
                    minute: 0,
                },
                normalized_rate_limit_config_cache: None,
            })
            .unwrap();
        let state = Arc::new(AppState {
            config: AppConfig::default(),
            http_client: reqwest::Client::new(),
            config_store: store as SharedGatewayConfigStore,
            rate_limiter: Arc::new(InMemoryUpstreamRateLimiter::default()) as SharedRateLimiter,
        });

        let response = handle_native_route(
            state,
            "google_generate",
            "gemini-prod".to_string(),
            Some("gemini-2.5-pro:generateContent".to_string()),
            UpstreamProtocol::GoogleGenerateContent,
            UpstreamRouteKind::GoogleV1BetaModels,
            Bytes::from(serde_json::to_vec(&body).unwrap()),
        )
        .await;

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    fn stream_test_state(
        base_url: String,
        rpm: Option<u32>,
        limiter: Arc<InMemoryUpstreamRateLimiter>,
    ) -> Arc<AppState> {
        stream_test_state_with_rule(
            base_url,
            crate::config::ModelRateLimitRule {
                model: "*".to_string(),
                rpm,
                rpd: None,
                tpm: None,
                tpm_mode: None,
                tokenizer_encoding: None,
                tokenizer_model: None,
            },
            limiter,
        )
    }

    fn stream_test_state_with_rule(
        base_url: String,
        rule: crate::config::ModelRateLimitRule,
        limiter: Arc<InMemoryUpstreamRateLimiter>,
    ) -> Arc<AppState> {
        let store = Arc::new(InMemoryGatewayConfigStore::default());
        store
            .insert(GatewayProviderBundle {
                provider: GatewayProvider {
                    id: "openai-prod".to_string(),
                    name: "OpenAI Prod".to_string(),
                    vendor: ProviderVendor::OpenAI,
                    base_url,
                    auth_strategy: GatewayAuthStrategy::Bearer,
                    enabled: true,
                },
                keys: vec![GatewayKey {
                    id: "key-a".to_string(),
                    provider_id: "openai-prod".to_string(),
                    display_name: Some("Key A".to_string()),
                    api_key: "sk-test-key-a".to_string(),
                    enabled: true,
                    weight: None,
                }],
                model_rules: vec![GatewayModelRule {
                    provider_id: "openai-prod".to_string(),
                    rule,
                }],
                daily_reset: DailyResetConfig {
                    timezone: "+00:00".to_string(),
                    hour: 0,
                    minute: 0,
                },
                normalized_rate_limit_config_cache: None,
            })
            .unwrap();

        Arc::new(AppState {
            config: AppConfig::default(),
            http_client: reqwest::Client::new(),
            config_store: store as SharedGatewayConfigStore,
            rate_limiter: limiter as SharedRateLimiter,
        })
    }
}
