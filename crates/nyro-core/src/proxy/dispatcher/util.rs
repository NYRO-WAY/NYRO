//! Pure utility helpers for the proxy dispatcher.
//!
//! Covers: route-target loading, retryability, cache configuration helpers,
//! request introspection, and semantic embedding input extraction.

use tokio::time::Duration;

use reqwest::header::{HeaderMap as ReqwestHeaderMap, HeaderValue as ReqwestHeaderValue};

use crate::Gateway;
use crate::cache::entry::CacheEntry;
use crate::db::models::{
    Route, RouteCacheConfig, RouteExactCacheConfig, RouteSemanticCacheConfig, RouteTarget,
};
use crate::protocol::types::{ContentBlock, InternalRequest, MessageContent, Role};

// ── Semantic write context ─────────────────────────────────────────────────────

/// Carry-along context that allows the response handler to write a semantic
/// cache entry after a successful upstream call.
#[derive(Clone)]
pub(super) struct SemanticWriteContext {
    pub(super) partition: String,
    pub(super) embedding_text: String,
    pub(super) key: String,
    pub(super) query_vector: Option<Vec<f32>>,
}

// ── Route target loading ────────────────────────────────────────────────────────

pub(super) async fn load_route_targets(gw: &Gateway, route: &Route) -> Vec<RouteTarget> {
    if let Some(store) = gw.storage.route_targets()
        && let Ok(targets) = store.list_targets_by_route(&route.id).await
        && !targets.is_empty()
    {
        return targets;
    }
    // Fallback: synthesize a single target from the legacy
    // `route.target_provider` / `route.target_model` columns.
    if route.target_provider.trim().is_empty() {
        return Vec::new();
    }
    vec![RouteTarget {
        id: String::new(),
        route_id: route.id.clone(),
        provider_id: route.target_provider.clone(),
        model: route.target_model.clone(),
        weight: 100,
        priority: 1,
        created_at: String::new(),
    }]
}

// ── Retry ─────────────────────────────────────────────────────────────────────

pub(super) fn is_retryable(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 529)
}

// ── Runtime-binding extra headers ─────────────────────────────────────────────

pub(super) fn runtime_binding_headers(
    binding: &crate::auth::RuntimeBinding,
) -> anyhow::Result<ReqwestHeaderMap> {
    let mut headers = ReqwestHeaderMap::new();
    for (key, value) in &binding.extra_headers {
        headers.insert(
            reqwest::header::HeaderName::from_bytes(key.as_bytes())?,
            ReqwestHeaderValue::from_str(value)?,
        );
    }
    Ok(headers)
}

// ── Route-level cache configuration helpers ────────────────────────────────────

pub(super) fn resolve_route_cache(route: &Route) -> RouteCacheConfig {
    let exact = route.cache_exact_ttl.map(|ttl| RouteExactCacheConfig {
        ttl: if ttl > 0 { Some(ttl) } else { None },
    });
    let semantic = route
        .cache_semantic_ttl
        .map(|ttl| RouteSemanticCacheConfig {
            ttl: if ttl > 0 { Some(ttl) } else { None },
            threshold: route.cache_semantic_threshold,
        });
    RouteCacheConfig { exact, semantic }
}

pub(super) fn route_exact_ttl(cache: &RouteCacheConfig, default_ttl: Duration) -> Duration {
    cache
        .exact
        .as_ref()
        .and_then(|e| e.ttl)
        .and_then(|ttl| (ttl > 0).then_some(Duration::from_secs(ttl as u64)))
        .unwrap_or(default_ttl)
}

pub(super) fn route_semantic_ttl(cache: &RouteCacheConfig, default_ttl: Duration) -> Duration {
    cache
        .semantic
        .as_ref()
        .and_then(|s| s.ttl)
        .and_then(|ttl| (ttl > 0).then_some(Duration::from_secs(ttl as u64)))
        .unwrap_or(default_ttl)
}

pub(super) fn route_semantic_threshold(cache: &RouteCacheConfig, default_threshold: f64) -> f64 {
    cache
        .semantic
        .as_ref()
        .and_then(|s| s.threshold)
        .filter(|t| *t > 0.0)
        .unwrap_or(default_threshold)
}

pub(super) fn is_semantic_entry_expired(entry: &CacheEntry, ttl: Duration) -> bool {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let ttl_ms = ttl.as_millis() as i64;
    now_ms.saturating_sub(entry.created_at_epoch_ms) > ttl_ms
}

// ── Request introspection ─────────────────────────────────────────────────────

pub(super) fn request_has_image_input(request: &InternalRequest) -> bool {
    for message in &request.messages {
        if let MessageContent::Blocks(blocks) = &message.content
            && blocks
                .iter()
                .any(|b| matches!(b, ContentBlock::Image { .. }))
        {
            return true;
        }
    }
    false
}

pub(super) fn extract_semantic_embedding_input(
    request: &InternalRequest,
) -> Option<(String, String)> {
    let system_prompt = request
        .messages
        .iter()
        .filter(|m| matches!(m.role, Role::System))
        .map(|m| m.content.as_text())
        .filter(|t| !t.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    let last_user = request
        .messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, Role::User))
        .map(|m| m.content.as_text())
        .filter(|t| !t.trim().is_empty())?;

    let combined = if system_prompt.is_empty() {
        last_user.clone()
    } else {
        format!("{system_prompt}\n{last_user}")
    };
    Some((system_prompt, combined))
}
