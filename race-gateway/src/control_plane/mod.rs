use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
};
use serde::Serialize;

use crate::app::AppState;
use crate::domain::{
    RaceGroup, RaceKeyPool, RaceModelDescriptor, RaceSettings, ValidationBuilder,
    ValidationErrorResponse, candidate_effective_upstream_model, resolve_model_for_candidate,
    validate_group, validate_key_pool, validate_model_descriptor,
};
use crate::runtime::build_group_runtime_snapshot;
use crate::web::{ADMIN_PLACEHOLDER_CSS, ADMIN_PLACEHOLDER_HTML, ADMIN_PLACEHOLDER_JS};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/admin/healthz", get(healthz))
        .route("/admin/metrics", get(metrics))
        .route("/admin", get(admin_root))
        .route("/admin/assets/admin.css", get(admin_css))
        .route("/admin/assets/admin.js", get(admin_js))
        .route("/admin/models", get(list_models))
        .route(
            "/admin/models/:model_id",
            get(get_model).put(put_model).delete(delete_model),
        )
        .route("/admin/groups", get(list_groups))
        .route(
            "/admin/groups/:group_id",
            get(get_group).put(put_group).delete(delete_group),
        )
        .route("/admin/key-pools", get(list_key_pools))
        .route(
            "/admin/key-pools/:key_pool_id",
            get(get_key_pool).put(put_key_pool).delete(delete_key_pool),
        )
        .route("/admin/runtime/groups", get(list_runtime_groups))
        .route("/admin/runtime/groups/:group_id", get(get_runtime_group))
        .route("/admin/settings", get(get_settings).put(put_settings))
        .route("/admin/validate/model", post(validate_model))
        .route("/admin/validate/group", post(validate_group_payload))
        .route("/admin/validate/key-pool", post(validate_key_pool_payload))
        .with_state(state)
}

async fn healthz(State(state): State<AppState>) -> Json<HealthResponse> {
    let bind_addr = state.config.admin_bind_addr.clone();
    Json(HealthResponse {
        status: "ok",
        service: "race-gateway",
        bind_addr: bind_addr.clone(),
        admin_bind_addr: bind_addr,
        proxy_bind_addr: state.config.proxy_bind_addr.clone(),
    })
}

async fn admin_root() -> Html<&'static str> {
    Html(ADMIN_PLACEHOLDER_HTML)
}

async fn admin_css() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8")],
        ADMIN_PLACEHOLDER_CSS,
    )
}

async fn admin_js() -> impl IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        ADMIN_PLACEHOLDER_JS,
    )
}

async fn metrics(State(state): State<AppState>) -> ApiResult<impl IntoResponse> {
    let body = state
        .observability
        .render()
        .map_err(|error| ApiError::internal(format!("failed to render metrics: {error}")))?;
    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    ))
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
    bind_addr: String,
    admin_bind_addr: String,
    proxy_bind_addr: String,
}

type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    body: serde_json::Value,
}

impl ApiError {
    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: serde_json::json!({ "message": message.into() }),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            body: serde_json::json!({ "message": message.into() }),
        }
    }

    fn validation(payload: ValidationErrorResponse) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            body: serde_json::to_value(payload)
                .unwrap_or_else(|_| serde_json::json!({ "valid": false, "issues": [] })),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            body: serde_json::json!({ "message": message.into() }),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(self.body)).into_response()
    }
}

async fn list_models(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<crate::domain::RaceModelSummary>>> {
    let mut summaries = state
        .config_cache
        .list_models()
        .into_iter()
        .map(|model| model.summary())
        .collect::<Vec<_>>();
    summaries.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(Json(summaries))
}

async fn get_model(
    State(state): State<AppState>,
    axum::extract::Path(model_id): axum::extract::Path<String>,
) -> ApiResult<Json<RaceModelDescriptor>> {
    match state.config_cache.get_model(&model_id) {
        Some(model) => Ok(Json(model)),
        None => Err(ApiError::not_found(format!("model '{model_id}' not found"))),
    }
}

async fn put_model(
    State(state): State<AppState>,
    axum::extract::Path(model_id): axum::extract::Path<String>,
    Json(mut model): Json<RaceModelDescriptor>,
) -> ApiResult<Json<RaceModelDescriptor>> {
    let previous_model_id = model_id.trim().to_string();
    if model.id.trim().is_empty() {
        model.id = previous_model_id.clone();
    }
    if previous_model_id != model.id {
        if state.config_cache.get_model(&previous_model_id).is_none() {
            return Err(ApiError::not_found(format!(
                "model '{previous_model_id}' not found"
            )));
        }
        if state.config_cache.get_model(&model.id).is_some() {
            return Err(ApiError::conflict(format!(
                "model '{}' already exists",
                model.id
            )));
        }
    }
    let validation = validate_model_request(&state, &model, Some(&previous_model_id));
    if !validation.valid {
        return Err(ApiError::validation(validation));
    }
    let saved = state
        .store
        .put_model(Some(&previous_model_id), model)
        .await
        .map_err(|error| ApiError::internal(format!("failed to save model: {error}")))?;
    reload_config_cache(&state).await?;
    Ok(Json(saved))
}

async fn delete_model(
    State(state): State<AppState>,
    axum::extract::Path(model_id): axum::extract::Path<String>,
) -> ApiResult<StatusCode> {
    if let Some(reference) = find_model_reference(&state, &model_id) {
        return Err(ApiError::conflict(format!(
            "模型 '{model_id}' 仍被竞速组 '{}' 的候选 '{}' 引用，请先解除引用后再删除",
            reference.0, reference.1
        )));
    }

    let deleted = state.store.delete_model(&model_id).await.map_err(|error| {
        ApiError::internal(format!("failed to delete model '{model_id}': {error}"))
    })?;
    if deleted {
        reload_config_cache(&state).await?;
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found(format!("model '{model_id}' not found")))
    }
}

async fn list_groups(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<crate::domain::RaceGroupSummary>>> {
    let mut summaries = state
        .config_cache
        .list_groups()
        .into_iter()
        .map(|group| summarize_group_from_cache(&state, &group))
        .collect::<Vec<_>>();
    summaries.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(Json(summaries))
}

async fn get_group(
    State(state): State<AppState>,
    axum::extract::Path(group_id): axum::extract::Path<String>,
) -> ApiResult<Json<RaceGroup>> {
    match state.config_cache.get_group(&group_id) {
        Some(group) => Ok(Json(group)),
        None => Err(ApiError::not_found(format!("group '{group_id}' not found"))),
    }
}

async fn put_group(
    State(state): State<AppState>,
    axum::extract::Path(group_id): axum::extract::Path<String>,
    Json(mut group): Json<RaceGroup>,
) -> ApiResult<Json<RaceGroup>> {
    let previous_group_id = group_id.trim().to_string();
    if group.id.trim().is_empty() {
        group.id = previous_group_id.clone();
    }
    if previous_group_id != group.id {
        if state.config_cache.get_group(&previous_group_id).is_none() {
            return Err(ApiError::not_found(format!(
                "group '{previous_group_id}' not found"
            )));
        }
        if state.config_cache.get_group(&group.id).is_some() {
            return Err(ApiError::conflict(format!(
                "group '{}' already exists",
                group.id
            )));
        }
    }
    normalize_group_request(&state, &mut group);
    let validation = validate_group_request(&state, &group);
    if !validation.valid {
        return Err(ApiError::validation(validation));
    }
    let saved = state
        .store
        .put_group(Some(&previous_group_id), group)
        .await
        .map_err(|error| ApiError::internal(format!("failed to save group: {error}")))?;
    if previous_group_id != saved.id {
        state.runtime.delete_group(&previous_group_id);
    }
    reload_config_cache(&state).await?;
    Ok(Json(saved))
}

async fn delete_group(
    State(state): State<AppState>,
    axum::extract::Path(group_id): axum::extract::Path<String>,
) -> ApiResult<StatusCode> {
    let deleted = state.store.delete_group(&group_id).await.map_err(|error| {
        ApiError::internal(format!("failed to delete group '{group_id}': {error}"))
    })?;
    if deleted {
        state.runtime.delete_group(&group_id);
        reload_config_cache(&state).await?;
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found(format!("group '{group_id}' not found")))
    }
}

async fn list_key_pools(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<crate::domain::RaceKeyPoolSummary>>> {
    let mut summaries = state
        .config_cache
        .list_key_pools()
        .into_iter()
        .map(|pool| pool.summary())
        .collect::<Vec<_>>();
    summaries.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(Json(summaries))
}

async fn get_key_pool(
    State(state): State<AppState>,
    axum::extract::Path(key_pool_id): axum::extract::Path<String>,
) -> ApiResult<Json<RaceKeyPool>> {
    match state.config_cache.get_key_pool(&key_pool_id) {
        Some(pool) => Ok(Json(mask_key_pool_for_response(pool))),
        None => Err(ApiError::not_found(format!(
            "key pool '{key_pool_id}' not found"
        ))),
    }
}

async fn put_key_pool(
    State(state): State<AppState>,
    axum::extract::Path(key_pool_id): axum::extract::Path<String>,
    Json(mut pool): Json<RaceKeyPool>,
) -> ApiResult<Json<RaceKeyPool>> {
    let previous_key_pool_id = key_pool_id.trim().to_string();
    if pool.id.trim().is_empty() {
        pool.id = previous_key_pool_id.clone();
    }
    if previous_key_pool_id != pool.id {
        if state
            .config_cache
            .get_key_pool(&previous_key_pool_id)
            .is_none()
        {
            return Err(ApiError::not_found(format!(
                "key pool '{previous_key_pool_id}' not found"
            )));
        }
        if state.config_cache.get_key_pool(&pool.id).is_some() {
            return Err(ApiError::conflict(format!(
                "key pool '{}' already exists",
                pool.id
            )));
        }
    }
    let existing = state.config_cache.get_key_pool(&previous_key_pool_id);
    for key in &mut pool.keys {
        key.key_pool_id = pool.id.clone();
        if key.secret.trim().is_empty() || key.secret.contains("***") {
            if let Some(existing_pool) = &existing
                && let Some(existing_key) = existing_pool
                    .keys
                    .iter()
                    .find(|candidate| candidate.id == key.id)
            {
                key.secret = existing_key.secret.clone();
            }
        }
    }
    let validation = validate_key_pool(&pool);
    if !validation.valid {
        return Err(ApiError::validation(validation));
    }
    let saved = state
        .store
        .put_key_pool(Some(&previous_key_pool_id), pool)
        .await
        .map_err(|error| ApiError::internal(format!("failed to save key pool: {error}")))?;
    reload_config_cache(&state).await?;
    Ok(Json(mask_key_pool_for_response(saved)))
}

async fn delete_key_pool(
    State(state): State<AppState>,
    axum::extract::Path(key_pool_id): axum::extract::Path<String>,
) -> ApiResult<StatusCode> {
    if let Some(reference) = find_key_pool_reference(&state, &key_pool_id) {
        return Err(ApiError::conflict(reference));
    }

    let deleted = state
        .store
        .delete_key_pool(&key_pool_id)
        .await
        .map_err(|error| {
            ApiError::internal(format!(
                "failed to delete key pool '{key_pool_id}': {error}"
            ))
        })?;
    if deleted {
        reload_config_cache(&state).await?;
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found(format!(
            "key pool '{key_pool_id}' not found"
        )))
    }
}

async fn get_settings(State(state): State<AppState>) -> ApiResult<Json<RaceSettings>> {
    Ok(Json(state.config_cache.get_settings()))
}

async fn list_runtime_groups(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<crate::runtime::GroupRuntimeSnapshot>>> {
    let groups = load_all_groups(&state).await?;
    let models = load_models_for_groups(&state, &groups).await?;
    let snapshots = groups
        .into_iter()
        .map(|group| {
            let handle = state.runtime.ensure_group(&group);
            build_group_runtime_snapshot(&group, &models, &handle)
        })
        .collect::<Vec<_>>();
    Ok(Json(snapshots))
}

async fn get_runtime_group(
    State(state): State<AppState>,
    axum::extract::Path(group_id): axum::extract::Path<String>,
) -> ApiResult<Json<crate::runtime::GroupRuntimeSnapshot>> {
    let group = state
        .config_cache
        .get_group(&group_id)
        .ok_or_else(|| ApiError::not_found(format!("group '{group_id}' not found")))?;
    let models = load_models_for_groups(&state, std::slice::from_ref(&group)).await?;
    let handle = state.runtime.ensure_group(&group);
    Ok(Json(build_group_runtime_snapshot(&group, &models, &handle)))
}

async fn put_settings(
    State(state): State<AppState>,
    Json(settings): Json<RaceSettings>,
) -> ApiResult<Json<RaceSettings>> {
    let saved = state
        .store
        .put_settings(settings.normalized())
        .await
        .map_err(|error| ApiError::internal(format!("failed to save settings: {error}")))?;
    reload_config_cache(&state).await?;
    Ok(Json(saved))
}

async fn validate_model(
    State(state): State<AppState>,
    Json(model): Json<RaceModelDescriptor>,
) -> Json<ValidationErrorResponse> {
    Json(validate_model_request(&state, &model, None))
}

fn validate_model_request(
    state: &AppState,
    model: &RaceModelDescriptor,
    previous_model_id: Option<&str>,
) -> ValidationErrorResponse {
    let mut validation = validate_model_descriptor(model);
    let mut builder = ValidationBuilder::default();

    for (index, endpoint) in model.endpoints.iter().enumerate() {
        validate_key_pool_reference(
            state,
            &mut builder,
            format!("endpoints[{index}].key_pool_id"),
            endpoint.key_pool_id.trim(),
        );
    }

    if let Some(other_model) = state
        .config_cache
        .list_models()
        .into_iter()
        .find(|existing| {
            existing.id != model.id
                && Some(existing.id.as_str()) != previous_model_id
                && existing.upstream_model == model.upstream_model
        })
    {
        builder.push(
            "upstream_model",
            "duplicate_upstream_model",
            format!(
                "upstream_model '{}' is already used by model '{}'",
                model.upstream_model, other_model.id
            ),
        );
    }

    validation.issues.extend(builder.finish().issues);
    validation.valid = validation.issues.is_empty();
    validation
}

fn validate_group_request(state: &AppState, group: &RaceGroup) -> ValidationErrorResponse {
    let mut validation = validate_group(group);
    let mut builder = ValidationBuilder::default();
    let models = state
        .config_cache
        .list_models()
        .into_iter()
        .map(|model| (model.id.clone(), model))
        .collect::<std::collections::BTreeMap<_, _>>();

    for (candidate_index, candidate) in group.candidates.iter().enumerate() {
        if let Some(model_id) = candidate
            .model_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            && state.config_cache.get_model(model_id).is_none()
        {
            builder.push(
                format!("candidates[{candidate_index}].model_id"),
                "unknown_model",
                format!("model '{model_id}' does not exist"),
            );
        }

        let has_inline_endpoint = !candidate.inline_endpoint_overrides.is_empty();
        let has_upstream_model = !candidate.upstream_model.trim().is_empty();
        let has_model_id = candidate
            .model_id
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty());

        if !has_upstream_model && !has_model_id {
            builder.push(
                format!("candidates[{candidate_index}].upstream_model"),
                "required",
                "candidate must provide upstream_model or bind a model_id",
            );
        }

        match resolve_model_for_candidate(candidate, &models) {
            Ok(Some(_)) => {}
            Ok(None) => {
                if !has_inline_endpoint && has_upstream_model && !has_model_id {
                    builder.push(
                        format!("candidates[{candidate_index}].upstream_model"),
                        "unresolved_candidate_target",
                        "candidate upstream_model does not match any model descriptor and no inline endpoint override is configured",
                    );
                }
            }
            Err(error) => {
                builder.push(
                    format!("candidates[{candidate_index}].upstream_model"),
                    "ambiguous_upstream_model",
                    error.to_string(),
                );
            }
        }

        for (endpoint_index, endpoint) in candidate.inline_endpoint_overrides.iter().enumerate() {
            validate_key_pool_reference(
                state,
                &mut builder,
                format!(
                    "candidates[{candidate_index}].inline_endpoint_overrides[{endpoint_index}].key_pool_id"
                ),
                endpoint.key_pool_id.trim(),
            );
        }
    }

    validation.issues.extend(builder.finish().issues);
    validation.valid = validation.issues.is_empty();
    validation
}

fn validate_key_pool_reference(
    state: &AppState,
    builder: &mut ValidationBuilder,
    field: String,
    key_pool_id: &str,
) {
    if key_pool_id.is_empty() {
        builder.push(field, "required", "key_pool_id cannot be empty");
        return;
    }

    if state.config_cache.get_key_pool(key_pool_id).is_none() {
        builder.push(
            field,
            "unknown_key_pool",
            format!("key pool '{key_pool_id}' does not exist"),
        );
    }
}

fn find_model_reference(state: &AppState, model_id: &str) -> Option<(String, String)> {
    let referenced_model = state.config_cache.get_model(model_id)?;

    state
        .config_cache
        .list_groups()
        .into_iter()
        .find_map(|group| {
            group.candidates.into_iter().find_map(|candidate| {
                (candidate.model_id.as_deref() == Some(model_id)
                    || (candidate.model_id.is_none()
                        && !candidate.upstream_model.trim().is_empty()
                        && candidate.upstream_model.trim() == referenced_model.upstream_model))
                    .then(|| (group.id.clone(), candidate.name))
            })
        })
}

async fn validate_group_payload(
    State(state): State<AppState>,
    Json(mut group): Json<RaceGroup>,
) -> Json<ValidationErrorResponse> {
    normalize_group_request(&state, &mut group);
    Json(validate_group_request(&state, &group))
}

fn find_key_pool_reference(state: &AppState, key_pool_id: &str) -> Option<String> {
    for model in state.config_cache.list_models() {
        if let Some(endpoint) = model
            .endpoints
            .iter()
            .find(|endpoint| endpoint.key_pool_id == key_pool_id)
        {
            return Some(format!(
                "Key 池 '{key_pool_id}' 仍被模型 '{}' 的 {:?} 端点引用，请先解除引用后再删除",
                model.id, endpoint.protocol_family
            ));
        }
    }

    for group in state.config_cache.list_groups() {
        for candidate in group.candidates {
            if let Some(endpoint) = candidate
                .inline_endpoint_overrides
                .iter()
                .find(|endpoint| endpoint.key_pool_id == key_pool_id)
            {
                return Some(format!(
                    "Key 池 '{key_pool_id}' 仍被竞速组 '{}' 的候选 '{}' 覆盖端点 {:?} 引用，请先解除引用后再删除",
                    group.id, candidate.name, endpoint.protocol_family
                ));
            }
        }
    }

    None
}

async fn validate_key_pool_payload(Json(pool): Json<RaceKeyPool>) -> Json<ValidationErrorResponse> {
    Json(validate_key_pool(&pool))
}

fn mask_key_pool_for_response(mut pool: RaceKeyPool) -> RaceKeyPool {
    for key in &mut pool.keys {
        key.secret = crate::group::mask_key(&key.secret);
    }
    pool
}

async fn load_all_groups(state: &AppState) -> ApiResult<Vec<RaceGroup>> {
    Ok(state.config_cache.list_groups())
}

async fn reload_config_cache(state: &AppState) -> ApiResult<()> {
    state
        .config_cache
        .reload_from_store(state.store.as_ref())
        .await
        .map_err(|error| ApiError::internal(format!("failed to reload config cache: {error}")))
}

fn summarize_group_from_cache(
    state: &AppState,
    group: &RaceGroup,
) -> crate::domain::RaceGroupSummary {
    let models = state.config_cache.models_for_candidates(&group.candidates);
    let mut protocol_families = std::collections::BTreeSet::new();

    for candidate in &group.candidates {
        for endpoint in candidate
            .inline_endpoint_overrides
            .iter()
            .filter(|endpoint| endpoint.enabled)
        {
            protocol_families.insert(endpoint.protocol_family);
        }

        if let Ok(Some(model)) = resolve_model_for_candidate(candidate, &models) {
            for endpoint in model.endpoints.iter().filter(|endpoint| endpoint.enabled) {
                protocol_families.insert(endpoint.protocol_family);
            }
        }
    }

    crate::domain::RaceGroupSummary {
        id: group.id.clone(),
        display_name: group.display_name.clone(),
        enabled: group.enabled,
        candidate_count: group.candidates.len(),
        enabled_candidate_count: group
            .candidates
            .iter()
            .filter(|candidate| candidate.enabled)
            .count(),
        protocol_families: protocol_families.into_iter().collect(),
        candidate_names: group
            .candidates
            .iter()
            .map(|candidate| candidate.name.clone())
            .collect(),
    }
}

async fn load_models_for_groups(
    state: &AppState,
    groups: &[RaceGroup],
) -> ApiResult<std::collections::BTreeMap<String, RaceModelDescriptor>> {
    Ok(state.config_cache.models_for_groups(groups))
}

fn normalize_group_request(state: &AppState, group: &mut RaceGroup) {
    for (index, candidate) in group.candidates.iter_mut().enumerate() {
        candidate.group_id = group.id.clone();
        if candidate.id.trim().is_empty() {
            candidate.id = generated_candidate_id(&group.id, &candidate.name, index);
        }
        if let Some(model_id) = candidate.model_id.as_deref()
            && candidate.upstream_model.trim().is_empty()
            && let Some(model) = state.config_cache.get_model(model_id)
        {
            candidate.upstream_model = candidate_effective_upstream_model(candidate, Some(&model));
        }
    }
}

fn generated_candidate_id(group_id: &str, candidate_name: &str, index: usize) -> String {
    let slug = candidate_name
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() {
                value.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    if slug.is_empty() {
        format!("{group_id}-candidate-{}", index + 1)
    } else {
        format!("{group_id}-{slug}")
    }
}
