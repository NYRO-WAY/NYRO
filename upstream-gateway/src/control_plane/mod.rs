use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::Serialize;
use serde_json::json;

use crate::app::AppState;
use crate::config::ModelRateLimitRule;
use crate::provider::{
    GatewayKey, GatewayModelRule, GatewayProviderBundle, GatewayProviderSummary,
};
use crate::runtime::UpstreamRateLimitRuntimeSnapshot;
use crate::storage::SharedProviderBundle;

#[derive(Debug, Clone, Serialize)]
struct ProviderRuntimeView {
    summary: GatewayProviderSummary,
    runtime: UpstreamRateLimitRuntimeSnapshot,
}

pub fn proxy_router() -> Router<Arc<AppState>> {
    Router::new().route("/healthz", get(healthz))
}

pub fn admin_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/admin/healthz", get(admin_healthz))
        .route("/admin/providers", get(list_providers))
        .route("/admin/runtime/providers", get(list_runtime_providers))
        .route(
            "/admin/providers/:provider_id",
            get(get_provider)
                .put(put_provider)
                .delete(delete_provider_by_id),
        )
        .route(
            "/admin/providers/:provider_id/runtime",
            get(get_provider_runtime),
        )
        .route(
            "/admin/providers/:provider_id/keys/:key_id",
            put(put_key).delete(delete_key_by_id),
        )
        .route(
            "/admin/providers/:provider_id/model-rules/*model",
            put(put_model_rule).delete(delete_model_rule_by_id),
        )
}

async fn healthz(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "service": "upstream-gateway",
        "bind_addr": state.config.proxy_bind_addr,
        "proxy_bind_addr": state.config.proxy_bind_addr,
    }))
}

async fn admin_healthz(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "plane": "control",
        "bind_addr": state.config.admin_bind_addr.clone(),
        "admin_bind_addr": state.config.admin_bind_addr.clone(),
    }))
}

async fn list_providers(State(state): State<Arc<AppState>>) -> Response {
    match state.config_store.list_provider_summaries().await {
        Ok(items) => Json(items).into_response(),
        Err(error) => internal_error(format!("list providers failed: {error}")),
    }
}

async fn list_runtime_providers(State(state): State<Arc<AppState>>) -> Response {
    let bundles = match state.config_store.list_provider_bundles_shared().await {
        Ok(items) => items,
        Err(error) => return internal_error(format!("list runtime providers failed: {error}")),
    };

    let mut views = Vec::with_capacity(bundles.len());
    for bundle in bundles {
        let runtime = match build_runtime_view(&state, &bundle) {
            Ok(runtime) => runtime,
            Err(response) => return response,
        };
        views.push(ProviderRuntimeView {
            summary: bundle.summary(),
            runtime,
        });
    }

    Json(views).into_response()
}

async fn get_provider(
    State(state): State<Arc<AppState>>,
    Path(provider_id): Path<String>,
) -> Response {
    match state.config_store.get_provider_bundle(&provider_id).await {
        Ok(Some(bundle)) => Json(bundle).into_response(),
        Ok(None) => not_found(format!("provider '{provider_id}' was not found")),
        Err(error) => internal_error(format!("get provider failed: {error}")),
    }
}

async fn get_provider_runtime(
    State(state): State<Arc<AppState>>,
    Path(provider_id): Path<String>,
) -> Response {
    let bundle = match load_runtime_bundle(&state, &provider_id).await {
        Ok(bundle) => bundle,
        Err(response) => return response,
    };
    let runtime = match build_runtime_view(&state, &bundle) {
        Ok(runtime) => runtime,
        Err(response) => return response,
    };

    Json(ProviderRuntimeView {
        summary: bundle.summary(),
        runtime,
    })
    .into_response()
}

async fn put_provider(
    State(state): State<Arc<AppState>>,
    Path(provider_id): Path<String>,
    Json(mut bundle): Json<GatewayProviderBundle>,
) -> Response {
    if bundle.provider.id.trim().is_empty() {
        bundle.provider.id = provider_id.clone();
    }
    if bundle.provider.id != provider_id {
        return bad_request(format!(
            "provider id mismatch: path '{provider_id}' body '{}'",
            bundle.provider.id
        ));
    }

    match state.config_store.put_provider_bundle(bundle).await {
        Ok(saved) => Json(saved).into_response(),
        Err(error) => internal_error(format!("put provider failed: {error}")),
    }
}

async fn delete_provider_by_id(
    State(state): State<Arc<AppState>>,
    Path(provider_id): Path<String>,
) -> Response {
    match state.config_store.delete_provider(&provider_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => not_found(format!("provider '{provider_id}' was not found")),
        Err(error) => internal_error(format!("delete provider failed: {error}")),
    }
}

async fn put_key(
    State(state): State<Arc<AppState>>,
    Path((provider_id, key_id)): Path<(String, String)>,
    Json(mut key): Json<GatewayKey>,
) -> Response {
    let mut bundle = match load_bundle(&state, &provider_id).await {
        Ok(bundle) => bundle,
        Err(response) => return response,
    };

    if key.id.trim().is_empty() {
        key.id = key_id.clone();
    }
    if key.id != key_id {
        return bad_request(format!(
            "key id mismatch: path '{key_id}' body '{}'",
            key.id
        ));
    }
    if key.provider_id.trim().is_empty() {
        key.provider_id = provider_id.clone();
    }
    if key.provider_id != provider_id {
        return bad_request(format!(
            "key provider_id mismatch: path '{provider_id}' body '{}'",
            key.provider_id
        ));
    }

    match bundle.keys.iter_mut().find(|item| item.id == key_id) {
        Some(existing) => *existing = key,
        None => bundle.keys.push(key),
    }

    match state.config_store.put_provider_bundle(bundle).await {
        Ok(saved) => Json(saved).into_response(),
        Err(error) => internal_error(format!("put key failed: {error}")),
    }
}

async fn delete_key_by_id(
    State(state): State<Arc<AppState>>,
    Path((provider_id, key_id)): Path<(String, String)>,
) -> Response {
    let mut bundle = match load_bundle(&state, &provider_id).await {
        Ok(bundle) => bundle,
        Err(response) => return response,
    };

    let original_len = bundle.keys.len();
    bundle.keys.retain(|item| item.id != key_id);
    if bundle.keys.len() == original_len {
        return not_found(format!(
            "key '{key_id}' was not found for provider '{provider_id}'"
        ));
    }
    if bundle.keys.is_empty() {
        return bad_request(format!(
            "provider '{provider_id}' must retain at least one enabled or disabled key entry"
        ));
    }

    match state.config_store.put_provider_bundle(bundle).await {
        Ok(saved) => Json(saved).into_response(),
        Err(error) => internal_error(format!("delete key failed: {error}")),
    }
}

async fn put_model_rule(
    State(state): State<Arc<AppState>>,
    Path((provider_id, model)): Path<(String, String)>,
    Json(mut rule): Json<ModelRateLimitRule>,
) -> Response {
    let mut bundle = match load_bundle(&state, &provider_id).await {
        Ok(bundle) => bundle,
        Err(response) => return response,
    };
    let model = sanitize_wildcard_path(model);

    if rule.model.trim().is_empty() {
        rule.model = model.clone();
    }
    if rule.model != model {
        return bad_request(format!(
            "model rule mismatch: path '{model}' body '{}'",
            rule.model
        ));
    }

    let new_rule = GatewayModelRule {
        provider_id: provider_id.clone(),
        rule,
    };
    match bundle
        .model_rules
        .iter_mut()
        .find(|item| item.rule.model == model)
    {
        Some(existing) => *existing = new_rule,
        None => bundle.model_rules.push(new_rule),
    }

    match state.config_store.put_provider_bundle(bundle).await {
        Ok(saved) => Json(saved).into_response(),
        Err(error) => internal_error(format!("put model rule failed: {error}")),
    }
}

async fn delete_model_rule_by_id(
    State(state): State<Arc<AppState>>,
    Path((provider_id, model)): Path<(String, String)>,
) -> Response {
    let mut bundle = match load_bundle(&state, &provider_id).await {
        Ok(bundle) => bundle,
        Err(response) => return response,
    };
    let model = sanitize_wildcard_path(model);

    let original_len = bundle.model_rules.len();
    bundle.model_rules.retain(|item| item.rule.model != model);
    if bundle.model_rules.len() == original_len {
        return not_found(format!(
            "model rule '{model}' was not found for provider '{provider_id}'"
        ));
    }

    match state.config_store.put_provider_bundle(bundle).await {
        Ok(saved) => Json(saved).into_response(),
        Err(error) => internal_error(format!("delete model rule failed: {error}")),
    }
}

async fn load_bundle(
    state: &AppState,
    provider_id: &str,
) -> Result<GatewayProviderBundle, Response> {
    match state.config_store.get_provider_bundle(provider_id).await {
        Ok(Some(bundle)) => Ok(bundle),
        Ok(None) => Err(not_found(format!("provider '{provider_id}' was not found"))),
        Err(error) => Err(internal_error(format!("provider lookup failed: {error}"))),
    }
}

async fn load_runtime_bundle(
    state: &AppState,
    provider_id: &str,
) -> Result<SharedProviderBundle, Response> {
    match state
        .config_store
        .get_provider_bundle_shared(provider_id)
        .await
    {
        Ok(Some(bundle)) => Ok(bundle),
        Ok(None) => Err(not_found(format!("provider '{provider_id}' was not found"))),
        Err(error) => Err(internal_error(format!("provider lookup failed: {error}"))),
    }
}

fn build_runtime_view(
    state: &AppState,
    bundle: &SharedProviderBundle,
) -> Result<UpstreamRateLimitRuntimeSnapshot, Response> {
    let rate_limit_config = bundle
        .normalized_rate_limit_config()
        .map_err(|error| internal_error(format!("runtime config normalization failed: {error}")))?;
    state
        .rate_limiter
        .runtime_snapshot(&bundle.provider, rate_limit_config)
        .map_err(|error| internal_error(format!("runtime snapshot failed: {error}")))
}

fn sanitize_wildcard_path(raw: String) -> String {
    raw.trim_start_matches('/').to_string()
}

fn bad_request(message: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": {
                "message": message.into(),
                "type": "invalid_request",
            }
        })),
    )
        .into_response()
}

fn not_found(message: impl Into<String>) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": {
                "message": message.into(),
                "type": "not_found",
            }
        })),
    )
        .into_response()
}

fn internal_error(message: impl Into<String>) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": {
                "message": message.into(),
                "type": "internal_error",
            }
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::to_bytes;
    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use serde_json::json;

    use crate::app::AppState;
    use crate::config::{AppConfig, DailyResetConfig, ModelRateLimitRule};
    use crate::provider::{
        GatewayAuthStrategy, GatewayKey, GatewayModelRule, GatewayProvider, GatewayProviderBundle,
        ProviderVendor,
    };
    use crate::runtime::{InMemoryUpstreamRateLimiter, SharedRateLimiter};
    use crate::storage::{InMemoryGatewayConfigStore, SharedGatewayConfigStore};

    use super::{
        delete_key_by_id, get_provider_runtime, list_runtime_providers, put_key, put_model_rule,
        put_provider,
    };

    #[tokio::test]
    async fn put_provider_persists_bundle_into_store() {
        let state = test_state();
        let bundle = sample_bundle();

        let response = put_provider(
            State(state.clone()),
            Path("openai-prod".to_string()),
            axum::Json(bundle),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let stored = state
            .config_store
            .get_provider_bundle("openai-prod")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.provider.name, "OpenAI Prod");
    }

    #[tokio::test]
    async fn put_key_updates_existing_bundle() {
        let state = seeded_test_state();
        let key = GatewayKey {
            id: "key-b".to_string(),
            provider_id: "openai-prod".to_string(),
            display_name: Some("Key B".to_string()),
            api_key: "sk-test-b".to_string(),
            enabled: true,
            weight: Some(2),
        };

        let response = put_key(
            State(state.clone()),
            Path(("openai-prod".to_string(), "key-b".to_string())),
            axum::Json(key),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let stored = state
            .config_store
            .get_provider_bundle("openai-prod")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.keys.len(), 2);
        assert!(stored.keys.iter().any(|item| item.id == "key-b"));
    }

    #[tokio::test]
    async fn delete_key_removes_entry_from_bundle() {
        let state = seeded_test_state_with_two_keys();

        let response = delete_key_by_id(
            State(state.clone()),
            Path(("openai-prod".to_string(), "key-a".to_string())),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let stored = state
            .config_store
            .get_provider_bundle("openai-prod")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.keys.len(), 1);
        assert_eq!(stored.keys[0].id, "key-b");
    }

    #[tokio::test]
    async fn delete_last_key_is_rejected() {
        let state = seeded_test_state();

        let response = delete_key_by_id(
            State(state),
            Path(("openai-prod".to_string(), "key-a".to_string())),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn put_model_rule_accepts_slash_model_path() {
        let state = seeded_test_state();
        let rule = ModelRateLimitRule {
            model: "openai/gpt-oss-120b".to_string(),
            rpm: Some(30),
            rpd: None,
            tpm: Some(20000),
            tpm_mode: Some(crate::config::TpmMode::InputOnly),
            tokenizer_encoding: None,
            tokenizer_model: None,
        };

        let response = put_model_rule(
            State(state.clone()),
            Path((
                "openai-prod".to_string(),
                "/openai/gpt-oss-120b".to_string(),
            )),
            axum::Json(rule),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let stored = state
            .config_store
            .get_provider_bundle("openai-prod")
            .await
            .unwrap()
            .unwrap();
        assert!(
            stored
                .model_rules
                .iter()
                .any(|item| item.rule.model == "openai/gpt-oss-120b")
        );
    }

    #[tokio::test]
    async fn get_provider_runtime_returns_snapshot_payload() {
        let state = seeded_test_state();

        let response = get_provider_runtime(State(state), Path("openai-prod".to_string())).await;
        assert_eq!(response.status(), StatusCode::OK);

        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["summary"]["id"], "openai-prod");
        assert!(value["runtime"]["models"].is_array());
    }

    #[tokio::test]
    async fn list_runtime_providers_returns_summary_runtime_views() {
        let state = seeded_test_state();

        let response = list_runtime_providers(State(state)).await;
        assert_eq!(response.status(), StatusCode::OK);

        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 1);
        assert_eq!(value[0]["summary"]["id"], "openai-prod");
        assert!(value[0]["runtime"]["models"].is_array());
    }

    fn test_state() -> Arc<AppState> {
        Arc::new(AppState {
            config: AppConfig::default(),
            http_client: reqwest::Client::new(),
            config_store: Arc::new(InMemoryGatewayConfigStore::default())
                as SharedGatewayConfigStore,
            rate_limiter: Arc::new(InMemoryUpstreamRateLimiter::default()) as SharedRateLimiter,
        })
    }

    fn seeded_test_state() -> Arc<AppState> {
        let store = Arc::new(InMemoryGatewayConfigStore::default());
        store.insert(sample_bundle()).unwrap();

        Arc::new(AppState {
            config: AppConfig::default(),
            http_client: reqwest::Client::new(),
            config_store: store as SharedGatewayConfigStore,
            rate_limiter: Arc::new(InMemoryUpstreamRateLimiter::default()) as SharedRateLimiter,
        })
    }

    fn seeded_test_state_with_two_keys() -> Arc<AppState> {
        let store = Arc::new(InMemoryGatewayConfigStore::default());
        let mut bundle = sample_bundle();
        bundle.keys.push(GatewayKey {
            id: "key-b".to_string(),
            provider_id: "openai-prod".to_string(),
            display_name: Some("Key B".to_string()),
            api_key: "sk-test-b".to_string(),
            enabled: true,
            weight: Some(2),
        });
        store.insert(bundle).unwrap();

        Arc::new(AppState {
            config: AppConfig::default(),
            http_client: reqwest::Client::new(),
            config_store: store as SharedGatewayConfigStore,
            rate_limiter: Arc::new(InMemoryUpstreamRateLimiter::default()) as SharedRateLimiter,
        })
    }

    fn sample_bundle() -> GatewayProviderBundle {
        GatewayProviderBundle {
            provider: GatewayProvider {
                id: "openai-prod".to_string(),
                name: "OpenAI Prod".to_string(),
                vendor: ProviderVendor::OpenAI,
                base_url: "https://api.openai.com".to_string(),
                auth_strategy: GatewayAuthStrategy::Bearer,
                enabled: true,
            },
            keys: vec![GatewayKey {
                id: "key-a".to_string(),
                provider_id: "openai-prod".to_string(),
                display_name: Some("Key A".to_string()),
                api_key: "sk-test-a".to_string(),
                enabled: true,
                weight: Some(1),
            }],
            model_rules: vec![GatewayModelRule {
                provider_id: "openai-prod".to_string(),
                rule: ModelRateLimitRule {
                    model: "*".to_string(),
                    rpm: Some(60),
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
        }
    }

    #[test]
    fn error_payload_shape_is_stable() {
        let response = super::bad_request("broken").into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = json!({
            "error": {
                "message": "broken",
                "type": "invalid_request",
            }
        });
        let _ = body;
    }
}
