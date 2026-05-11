use std::{collections::BTreeMap, net::SocketAddr, time::Duration};

use async_stream::stream;
use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::Path,
    http::{Request, StatusCode},
    response::IntoResponse,
    routing::post,
};
use race_gateway::{
    app::{AppState, build_proxy_router},
    config::AppConfig,
    domain::{
        AuthStrategy, KeySelectionStrategy, ProtocolFamily, RaceCandidate, RaceGroup, RaceKey,
        RaceKeyPool, RaceTargetEndpoint,
    },
};
use serde_json::json;
use tokio::net::TcpListener;
use tower::ServiceExt;

fn temp_database_url(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    format!("sqlite://./target/{tag}-{nanos}.db")
}

async fn new_state(tag: &str) -> AppState {
    AppState::new(AppConfig {
        proxy_bind_addr: "127.0.0.1:0".to_string(),
        admin_bind_addr: "127.0.0.1:0".to_string(),
        database_url: temp_database_url(tag),
        bootstrap_json_path: None,
    })
    .await
    .expect("create app state")
}

async fn spawn_server(router: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let addr: SocketAddr = listener.local_addr().expect("local addr");
    let handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("serve test router");
    });
    (format!("http://{addr}"), handle)
}

fn openai_sse(text: &'static str, delay_ms: u64) -> impl IntoResponse {
    let body = Body::from_stream(stream! {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        yield Ok::<Bytes, std::convert::Infallible>(Bytes::from(format!(
            "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{text}\"}},\"finish_reason\":null}}]}}\n\n"
        )));
        yield Ok::<Bytes, std::convert::Infallible>(Bytes::from(
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n".to_string(),
        ));
        yield Ok::<Bytes, std::convert::Infallible>(Bytes::from("data: [DONE]\n\n".to_string()));
    });
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        body,
    )
}

fn openai_sse_parts(parts: Vec<(&'static str, u64)>) -> impl IntoResponse {
    let body = Body::from_stream(stream! {
        for (text, delay_ms) in parts {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            yield Ok::<Bytes, std::convert::Infallible>(Bytes::from(format!(
                "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{text}\"}},\"finish_reason\":null}}]}}\n\n"
            )));
        }
        yield Ok::<Bytes, std::convert::Infallible>(Bytes::from(
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n".to_string(),
        ));
        yield Ok::<Bytes, std::convert::Infallible>(Bytes::from("data: [DONE]\n\n".to_string()));
    });
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        body,
    )
}

fn anthropic_sse(text: &'static str, delay_ms: u64) -> impl IntoResponse {
    let body = Body::from_stream(stream! {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        yield Ok::<Bytes, std::convert::Infallible>(Bytes::from(
            format!("event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{text}\"}}}}\n\n")
        ));
        yield Ok::<Bytes, std::convert::Infallible>(Bytes::from(
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n".to_string(),
        ));
        yield Ok::<Bytes, std::convert::Infallible>(Bytes::from(
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string(),
        ));
    });
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        body,
    )
}

fn google_sse(text: &'static str, delay_ms: u64) -> impl IntoResponse {
    let body = Body::from_stream(stream! {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        yield Ok::<Bytes, std::convert::Infallible>(Bytes::from(
            format!("data: {{\"candidates\":[{{\"content\":{{\"role\":\"model\",\"parts\":[{{\"text\":\"{text}\"}}]}}}}]}}\n\n")
        ));
        yield Ok::<Bytes, std::convert::Infallible>(Bytes::from(
            "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[]},\"finishReason\":\"STOP\"}]}\n\n".to_string(),
        ));
    });
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        body,
    )
}

async fn seed_group_with_inline_endpoints(
    state: &AppState,
    group_id: &str,
    a_openai: &str,
    b_openai: &str,
    a_anthropic: &str,
    b_anthropic: &str,
    a_google: &str,
    b_google: &str,
) {
    let key_pool = RaceKeyPool {
        id: "pool-a".to_string(),
        display_name: "Pool A".to_string(),
        auth_strategy: AuthStrategy::Bearer,
        selection_strategy: KeySelectionStrategy::Random,
        enabled: true,
        keys: vec![RaceKey {
            id: "key-a".to_string(),
            key_pool_id: "pool-a".to_string(),
            secret: "secret-a".to_string(),
            enabled: true,
            metadata: json!({}),
        }],
    };
    state
        .store
        .put_key_pool(None, key_pool.clone())
        .await
        .expect("put key pool");
    state.config_cache.put_key_pool(key_pool);

    let endpoint = |protocol_family: ProtocolFamily, base_url: String| RaceTargetEndpoint {
        protocol_family,
        base_url,
        auth_strategy: AuthStrategy::Bearer,
        key_pool_id: "pool-a".to_string(),
        request_timeout_ms: Some(10_000),
        extra_headers: Default::default(),
        extra_query: BTreeMap::new(),
        enabled: true,
    };

    let group = RaceGroup {
        id: group_id.to_string(),
        display_name: "Group A".to_string(),
        fallback_ratio: 0.0,
        decay_factor: 1.0,
        penalty_rate: 40.0,
        recovery_rate: 0.0,
        race_max_wait_time_ms: Some(2_000),
        enabled: true,
        candidates: vec![
            RaceCandidate {
                id: "cand-a".to_string(),
                group_id: group_id.to_string(),
                name: "A".to_string(),
                model_id: None,
                upstream_model: "model-a".to_string(),
                inline_endpoint_overrides: vec![
                    endpoint(ProtocolFamily::OpenAi, format!("{a_openai}/v1")),
                    endpoint(ProtocolFamily::Anthropic, a_anthropic.to_string()),
                    endpoint(ProtocolFamily::Google, a_google.to_string()),
                ],
                initial_weight: 100.0,
                response_protection_timeout_ms: 50,
                enabled: true,
                metadata: json!({}),
            },
            RaceCandidate {
                id: "cand-b".to_string(),
                group_id: group_id.to_string(),
                name: "B".to_string(),
                model_id: None,
                upstream_model: "model-b".to_string(),
                inline_endpoint_overrides: vec![
                    endpoint(ProtocolFamily::OpenAi, format!("{b_openai}/v1")),
                    endpoint(ProtocolFamily::Anthropic, b_anthropic.to_string()),
                    endpoint(ProtocolFamily::Google, b_google.to_string()),
                ],
                initial_weight: 90.0,
                response_protection_timeout_ms: 50,
                enabled: true,
                metadata: json!({}),
            },
        ],
    };
    state
        .store
        .put_group(None, group.clone())
        .await
        .expect("put group");
    state.config_cache.put_group(group);
}

async fn seed_single_openai_group(
    state: &AppState,
    group_id: &str,
    base_url: &str,
    request_timeout_ms: u64,
) {
    let key_pool = RaceKeyPool {
        id: "pool-timeout".to_string(),
        display_name: "Pool Timeout".to_string(),
        auth_strategy: AuthStrategy::Bearer,
        selection_strategy: KeySelectionStrategy::Random,
        enabled: true,
        keys: vec![RaceKey {
            id: "key-timeout".to_string(),
            key_pool_id: "pool-timeout".to_string(),
            secret: "secret-timeout".to_string(),
            enabled: true,
            metadata: json!({}),
        }],
    };
    state
        .store
        .put_key_pool(None, key_pool.clone())
        .await
        .expect("put timeout key pool");
    state.config_cache.put_key_pool(key_pool);

    let group = RaceGroup {
        id: group_id.to_string(),
        display_name: "Timeout Group".to_string(),
        fallback_ratio: 0.0,
        decay_factor: 1.0,
        penalty_rate: 5.0,
        recovery_rate: 0.0,
        race_max_wait_time_ms: Some(2_000),
        enabled: true,
        candidates: vec![RaceCandidate {
            id: "cand-timeout".to_string(),
            group_id: group_id.to_string(),
            name: "A".to_string(),
            model_id: None,
            upstream_model: "model-timeout".to_string(),
            inline_endpoint_overrides: vec![RaceTargetEndpoint {
                protocol_family: ProtocolFamily::OpenAi,
                base_url: format!("{base_url}/v1"),
                auth_strategy: AuthStrategy::Bearer,
                key_pool_id: "pool-timeout".to_string(),
                request_timeout_ms: Some(request_timeout_ms),
                extra_headers: Default::default(),
                extra_query: Default::default(),
                enabled: true,
            }],
            initial_weight: 100.0,
            response_protection_timeout_ms: 10,
            enabled: true,
            metadata: json!({}),
        }],
    };
    state
        .store
        .put_group(None, group.clone())
        .await
        .expect("put timeout group");
    state.config_cache.put_group(group);
}

async fn body_text(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    String::from_utf8(bytes.to_vec()).expect("utf8 body")
}

#[tokio::test]
async fn proxy_supports_openai_anthropic_google_and_shared_weights() {
    let (server_a_base, server_a_handle) = spawn_server(
        Router::new()
            .route(
                "/v1/chat/completions",
                post(|| async { openai_sse("A-OPENAI", 80) }),
            )
            .route(
                "/v1/messages",
                post(|| async { anthropic_sse("A-ANTHROPIC", 10) }),
            )
            .route(
                "/v1beta/models/:model_action",
                post(|Path(_): Path<String>| async { google_sse("A-GOOGLE", 30) }),
            ),
    )
    .await;
    let (server_b_base, server_b_handle) = spawn_server(
        Router::new()
            .route(
                "/v1/chat/completions",
                post(|| async { openai_sse("B-OPENAI", 10) }),
            )
            .route(
                "/v1/messages",
                post(|| async { anthropic_sse("B-ANTHROPIC", 20) }),
            )
            .route(
                "/v1beta/models/:model_action",
                post(|Path(_): Path<String>| async { google_sse("B-GOOGLE", 10) }),
            ),
    )
    .await;

    let state = new_state("proxy-pass").await;
    seed_group_with_inline_endpoints(
        &state,
        "group-a",
        &server_a_base,
        &server_b_base,
        &server_a_base,
        &server_b_base,
        &server_a_base,
        &server_b_base,
    )
    .await;
    let router = build_proxy_router(state.clone());

    let openai_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/groups/group-a/openai/v1/chat/completions")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "model": "placeholder",
                        "stream": true,
                        "messages": [{"role": "user", "content": "hello"}]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("openai request");
    assert_eq!(openai_response.status(), StatusCode::OK);
    let openai_text = body_text(openai_response).await;
    assert!(openai_text.contains("B-OPENAI"));
    let weights = state
        .runtime
        .get_group("group-a")
        .expect("group runtime")
        .snapshot_weights();
    assert!(weights["A"].effective_weight < weights["B"].effective_weight);

    let anthropic_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/groups/group-a/anthropic/v1/messages")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "model": "placeholder",
                        "max_tokens": 128,
                        "stream": true,
                        "messages": [{"role": "user", "content": "hello"}]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("anthropic request");
    assert_eq!(anthropic_response.status(), StatusCode::OK);
    let anthropic_text = body_text(anthropic_response).await;
    assert!(anthropic_text.contains("B-ANTHROPIC"));

    let google_response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/groups/group-a/google/v1beta/models/streamGenerateContent")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "contents": [{"role": "user", "parts": [{"text": "hello"}]}]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("google request");
    assert_eq!(google_response.status(), StatusCode::OK);
    let google_text = body_text(google_response).await;
    assert!(google_text.contains("B-GOOGLE"));

    server_a_handle.abort();
    server_b_handle.abort();
}

#[tokio::test]
async fn proxy_returns_protocol_specific_all_failed_streams() {
    let failing_router = Router::new()
        .route(
            "/v1/chat/completions",
            post(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
        )
        .route(
            "/v1/messages",
            post(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
        )
        .route(
            "/v1beta/models/:model_action",
            post(|Path(_): Path<String>| async { StatusCode::INTERNAL_SERVER_ERROR }),
        );
    let (base_url, handle) = spawn_server(failing_router).await;

    let state = new_state("proxy-fail").await;
    seed_group_with_inline_endpoints(
        &state,
        "group-fail",
        &base_url,
        &base_url,
        &base_url,
        &base_url,
        &base_url,
        &base_url,
    )
    .await;
    let router = build_proxy_router(state);

    let openai_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/groups/group-fail/openai/v1/chat/completions")
                .header("content-type", "application/json")
                .body(Body::from(json!({"model":"x","stream":true,"messages":[{"role":"user","content":"hello"}]}).to_string()))
                .unwrap(),
        )
        .await
        .expect("openai fail request");
    let openai_text = body_text(openai_response).await;
    assert!(openai_text.contains("all_candidates_failed"));
    assert!(openai_text.contains("[DONE]"));

    let anthropic_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/groups/group-fail/anthropic/v1/messages")
                .header("content-type", "application/json")
                .body(Body::from(json!({"model":"x","stream":true,"max_tokens":10,"messages":[{"role":"user","content":"hello"}]}).to_string()))
                .unwrap(),
        )
        .await
        .expect("anthropic fail request");
    let anthropic_text = body_text(anthropic_response).await;
    assert!(anthropic_text.contains("message_stop"));

    let google_response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/groups/group-fail/google/v1beta/models/streamGenerateContent")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"contents":[{"role":"user","parts":[{"text":"hello"}]}]}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("google fail request");
    let google_text = body_text(google_response).await;
    assert!(google_text.contains("All candidates failed"));

    handle.abort();
}

#[tokio::test]
async fn proxy_request_timeout_does_not_cap_healthy_long_stream() {
    let (base_url, handle) = spawn_server(Router::new().route(
        "/v1/chat/completions",
        post(|| async { openai_sse_parts(vec![("A", 0), ("B", 50), ("C", 50)]) }),
    ))
    .await;

    let state = new_state("proxy-timeout").await;
    seed_single_openai_group(&state, "group-timeout", &base_url, 80).await;
    let router = build_proxy_router(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/groups/group-timeout/openai/v1/chat/completions")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "model": "placeholder",
                        "stream": true,
                        "messages": [{"role": "user", "content": "hello"}]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("timeout request");
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    assert!(text.contains("\"content\":\"A\""));
    assert!(text.contains("\"content\":\"B\""));
    assert!(text.contains("\"content\":\"C\""));
    assert!(text.contains("[DONE]"));

    handle.abort();
}
