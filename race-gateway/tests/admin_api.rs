use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use race_gateway::{
    app::{AppState, build_admin_router},
    config::AppConfig,
    domain::{
        AuthStrategy, KeySelectionStrategy, ProtocolFamily, RaceCandidate, RaceGroup, RaceKey,
        RaceKeyPool, RaceModelDescriptor, RaceTargetEndpoint,
    },
};
use serde_json::{Value, json};
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

async fn seed_admin_fixture(state: &AppState) {
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

    let model = RaceModelDescriptor {
        id: "model-a".to_string(),
        display_name: "Model A".to_string(),
        upstream_model: "vendor/model-a".to_string(),
        description: "desc".to_string(),
        enabled: true,
        endpoints: vec![RaceTargetEndpoint {
            protocol_family: ProtocolFamily::OpenAi,
            base_url: "http://127.0.0.1:1/v1".to_string(),
            auth_strategy: AuthStrategy::Bearer,
            key_pool_id: "pool-a".to_string(),
            request_timeout_ms: Some(30_000),
            extra_headers: Default::default(),
            extra_query: Default::default(),
            enabled: true,
        }],
        metadata: json!({}),
    };
    state
        .store
        .put_model(None, model.clone())
        .await
        .expect("put model");
    state.config_cache.put_model(model);

    let group = RaceGroup {
        id: "group-a".to_string(),
        display_name: "Group A".to_string(),
        fallback_ratio: 0.0,
        decay_factor: 1.0,
        penalty_rate: 40.0,
        recovery_rate: 0.0,
        race_max_wait_time_ms: Some(15_000),
        enabled: true,
        candidates: vec![RaceCandidate {
            id: "cand-a".to_string(),
            group_id: "group-a".to_string(),
            name: "A".to_string(),
            model_id: Some("model-a".to_string()),
            upstream_model: "vendor/model-a".to_string(),
            inline_endpoint_overrides: vec![],
            initial_weight: 100.0,
            response_protection_timeout_ms: 1_000,
            enabled: true,
            metadata: json!({}),
        }],
    };
    state
        .store
        .put_group(None, group.clone())
        .await
        .expect("put group");
    state.config_cache.put_group(group);
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("json body")
}

async fn text_body(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    String::from_utf8(bytes.to_vec()).expect("utf-8 body")
}

#[tokio::test]
async fn admin_crud_runtime_and_masking_work() {
    let state = new_state("admin-api").await;
    seed_admin_fixture(&state).await;
    let router = build_admin_router(state.clone());

    let list_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/groups")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("groups request");
    assert_eq!(list_response.status(), StatusCode::OK);

    let key_pool_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/key-pools/pool-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("key pool request");
    let key_pool_json = json_body(key_pool_response).await;
    assert_eq!(key_pool_json["keys"][0]["secret"], "secret-a***");

    let invalid_group = json!({
        "id": "group-b",
        "display_name": "",
        "fallback_ratio": 0.0,
        "decay_factor": 1.0,
        "penalty_rate": 40.0,
        "recovery_rate": 0.0,
        "race_max_wait_time_ms": 15000,
        "enabled": true,
        "candidates": []
    });
    let invalid_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/admin/groups/group-b")
                .header("content-type", "application/json")
                .body(Body::from(invalid_group.to_string()))
                .unwrap(),
        )
        .await
        .expect("invalid put");
    assert_eq!(invalid_response.status(), StatusCode::BAD_REQUEST);
    let invalid_json = json_body(invalid_response).await;
    assert_eq!(invalid_json["valid"], false);
    assert!(
        invalid_json["issues"]
            .as_array()
            .is_some_and(|issues| !issues.is_empty())
    );

    let runtime_response = router
        .oneshot(
            Request::builder()
                .uri("/admin/runtime/groups/group-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("runtime request");
    assert_eq!(runtime_response.status(), StatusCode::OK);
    let runtime_json = json_body(runtime_response).await;
    assert_eq!(runtime_json["group_id"], "group-a");
    assert!(runtime_json["effective_weights"]["A"].is_object());

    let metrics_response = build_admin_router(state)
        .oneshot(
            Request::builder()
                .uri("/admin/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("metrics request");
    assert_eq!(metrics_response.status(), StatusCode::OK);
    let metrics_text = text_body(metrics_response).await;
    assert!(metrics_text.contains("race_gateway_http_requests_total"));
}

#[tokio::test]
async fn admin_put_group_supports_renaming_group_id() {
    let state = new_state("admin-group-rename").await;
    seed_admin_fixture(&state).await;
    let router = build_admin_router(state);

    let renamed_group = json!({
        "id": "group-b",
        "display_name": "Group B",
        "fallback_ratio": 0.0,
        "decay_factor": 1.0,
        "penalty_rate": 40.0,
        "recovery_rate": 0.0,
        "race_max_wait_time_ms": 15000,
        "enabled": true,
        "candidates": [{
            "id": "cand-a",
            "group_id": "group-b",
            "name": "A",
            "model_id": "model-a",
            "upstream_model": "vendor/model-a",
            "inline_endpoint_overrides": [],
            "initial_weight": 100.0,
            "response_protection_timeout_ms": 1000,
            "enabled": true,
            "metadata": {}
        }]
    });

    let rename_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/admin/groups/group-a")
                .header("content-type", "application/json")
                .body(Body::from(renamed_group.to_string()))
                .unwrap(),
        )
        .await
        .expect("rename put");
    assert_eq!(rename_response.status(), StatusCode::OK);
    let renamed_json = json_body(rename_response).await;
    assert_eq!(renamed_json["id"], "group-b");
    assert_eq!(renamed_json["candidates"][0]["group_id"], "group-b");

    let old_group_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/groups/group-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("old group request");
    assert_eq!(old_group_response.status(), StatusCode::NOT_FOUND);

    let new_group_response = router
        .oneshot(
            Request::builder()
                .uri("/admin/groups/group-b")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("new group request");
    assert_eq!(new_group_response.status(), StatusCode::OK);
}

#[tokio::test]
async fn admin_put_model_supports_renaming_model_id_and_rebinding_groups() {
    let state = new_state("admin-model-rename").await;
    seed_admin_fixture(&state).await;
    let router = build_admin_router(state);

    let renamed_model = json!({
        "id": "model-b",
        "display_name": "Model B",
        "upstream_model": "vendor/model-a",
        "description": "desc",
        "enabled": true,
        "endpoints": [{
            "protocol_family": "openai",
            "base_url": "http://127.0.0.1:1/v1",
            "auth_strategy": { "kind": "bearer" },
            "key_pool_id": "pool-a",
            "request_timeout_ms": 30000,
            "extra_headers": {},
            "extra_query": {},
            "enabled": true
        }],
        "metadata": {}
    });

    let rename_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/admin/models/model-a")
                .header("content-type", "application/json")
                .body(Body::from(renamed_model.to_string()))
                .unwrap(),
        )
        .await
        .expect("rename model");
    assert_eq!(rename_response.status(), StatusCode::OK);

    let old_model_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/models/model-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("old model request");
    assert_eq!(old_model_response.status(), StatusCode::NOT_FOUND);

    let new_model_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/models/model-b")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("new model request");
    assert_eq!(new_model_response.status(), StatusCode::OK);

    let group_response = router
        .oneshot(
            Request::builder()
                .uri("/admin/groups/group-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("group request");
    let group_json = json_body(group_response).await;
    assert_eq!(group_json["candidates"][0]["model_id"], "model-b");
}

#[tokio::test]
async fn admin_put_key_pool_supports_renaming_and_rebinding_model_endpoints() {
    let state = new_state("admin-key-pool-rename").await;
    seed_admin_fixture(&state).await;
    let router = build_admin_router(state);

    let renamed_key_pool = json!({
        "id": "pool-b",
        "display_name": "Pool B",
        "auth_strategy": { "kind": "bearer" },
        "selection_strategy": "random",
        "enabled": true,
        "keys": [{
            "id": "key-a",
            "key_pool_id": "pool-b",
            "secret": "secret-a",
            "enabled": true,
            "metadata": {}
        }]
    });

    let rename_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/admin/key-pools/pool-a")
                .header("content-type", "application/json")
                .body(Body::from(renamed_key_pool.to_string()))
                .unwrap(),
        )
        .await
        .expect("rename key pool");
    assert_eq!(rename_response.status(), StatusCode::OK);

    let old_pool_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/key-pools/pool-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("old key pool request");
    assert_eq!(old_pool_response.status(), StatusCode::NOT_FOUND);

    let new_pool_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/key-pools/pool-b")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("new key pool request");
    assert_eq!(new_pool_response.status(), StatusCode::OK);

    let model_response = router
        .oneshot(
            Request::builder()
                .uri("/admin/models/model-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("model request");
    let model_json = json_body(model_response).await;
    assert_eq!(model_json["endpoints"][0]["key_pool_id"], "pool-b");
}

#[tokio::test]
async fn admin_delete_group_clears_runtime_handle() {
    let state = new_state("admin-group-delete-runtime").await;
    seed_admin_fixture(&state).await;
    let _ = state.runtime.ensure_group(
        &state
            .config_cache
            .get_group("group-a")
            .expect("group in cache"),
    );
    assert!(state.runtime.get_group("group-a").is_some());

    let router = build_admin_router(state.clone());
    let delete_response = router
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/admin/groups/group-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("delete group");
    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);
    assert!(state.runtime.get_group("group-a").is_none());
}
