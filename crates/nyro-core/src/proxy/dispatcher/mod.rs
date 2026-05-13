//! Dispatcher: single orchestration point that drives a request through the
//! full proxy pipeline.
//!
//! `dispatch_pipeline` is the canonical entry point. Each ingress thin-shell
//! decodes the incoming body into an `InternalRequest` and calls this function.
//!
//! Pipeline:
//!   1. Route lookup + type gate (embedding vs chat).
//!   2. `authorize_route_access` (API-key auth + quota).
//!   3. Exact-cache check → singleflight dedup.
//!   4. Semantic-cache check.
//!   5. Target iteration (health-aware): for each live target →
//!      a. Resolve `Provider` + `ProviderRuntime`.
//!      b. Resolve egress protocol + base URL via `negotiate()`.
//!      c. Look up `Vendor` from `VendorRegistry`.
//!      d. Build outbound: `ProtocolMode::Native` + no mutations → `passthrough_run`;
//!         else full 7-step `adapter.build_request`.
//!      e. Merge `runtime_binding` extra-headers.
//!      f. HTTP call → `handle_non_stream` / `handle_stream`.
//!      g. On success: record health, return; on retryable error: continue.
//!   6. Finalize singleflight; return last error or 502.

mod accumulator;
mod util;
use self::accumulator::*;
use self::util::*;

use std::convert::Infallible;
use std::time::Instant;

use bytes::Bytes;

use async_trait::async_trait;
use axum::Json;
use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use dashmap::mapref::entry::Entry as DashEntry;
use futures::StreamExt;
use reqwest::header::HeaderMap as ReqwestHeaderMap;
use serde_json::Value;
use tokio::sync::broadcast;
use tokio::time::{Duration, timeout};
use tokio_stream::wrappers::ReceiverStream;

use crate::Gateway;
use crate::cache::entry::CacheEntry;
use crate::cache::key::{build_cache_key, build_semantic_partition};
use crate::db::models::{Provider, Route};
use crate::error::{AuthFailure, GatewayError};
use crate::protocol::ProviderProtocols;
use crate::protocol::ids::{OPENAI_CHAT_COMPLETIONS_V1, OPENAI_EMBEDDINGS_V1, ProtocolId};
use crate::protocol::ir::{AiRequest, RawEnvelope};
use crate::protocol::types::{InternalRequest, InternalResponse, StreamDelta, TokenUsage};
use crate::provider::inbound::InboundResponse;
use crate::provider::vendor::ProviderCtx;
use crate::provider::{VendorCtx, VendorRegistry};
use crate::proxy::client::ProxyClient;
use crate::proxy::context::RequestContext;
use crate::proxy::observability::{LogExtras, emit_log, headers_to_json};
use crate::proxy::planner::{ProtocolMode, negotiate};
use crate::proxy::security::{extract_api_key, is_key_expired};
use crate::router::TargetSelector;
use crate::storage::traits::{ApiKeyAccessRecord, UsageWindow};

// ── Public entry points ───────────────────────────────────────────────────────

/// Full pipeline entry point.
///
/// Each ingress shell captures the raw body in a `RawEnvelope` and decodes
/// the body into an `AiRequest`, then hands off here.
///
/// Pipeline:
///   a. Resolve egress protocol + base URL via `negotiate()`.
///   b. Auth + cache check.
///   c. Look up `Vendor` from `VendorRegistry`.
///   d. Build outbound: `ProtocolMode::Native` + no mutations → `passthrough_run`;
///      else full 7-step `adapter.build_request`.
///   e. HTTP call → `handle_non_stream` / `handle_stream`.
pub async fn dispatch_pipeline(
    gw: Gateway,
    headers: HeaderMap,
    envelope: RawEnvelope,
    request: AiRequest,
    ingress: ProtocolId,
) -> Response {
    // Derive logging strings from envelope; convert new IR → old IR for backward-compat.
    let method_owned = envelope.method.clone();
    let path_owned = envelope.path.clone();
    let request_body_str = envelope
        .body
        .as_ref()
        .and_then(|b| serde_json::to_string(b).ok());
    let request_headers_str = serde_json::to_string(&envelope.headers).ok();
    let internal: InternalRequest = request.into();
    let start = Instant::now();
    let request_model = internal.model.clone();
    let is_stream = internal.stream;
    let is_embedding_request = ingress.handler().capabilities().embeddings;
    let ingress_str = ingress.to_string();

    // ── Route lookup ─────────────────────────────────────────────────────────

    let route = {
        let cache = gw.route_cache.read().await;
        cache.match_route(&request_model).cloned()
    };
    let route = match route {
        Some(r) => r,
        None => {
            let msg = format!("no route for model: {request_model}");
            emit_log(
                &gw,
                &ingress_str,
                &ingress_str,
                &request_model,
                "",
                None,
                "",
                404,
                start.elapsed().as_millis() as f64,
                TokenUsage::default(),
                is_stream,
                false,
                Some(msg.clone()),
                None,
                LogExtras {
                    method: Some(method_owned.clone()),
                    path: Some(path_owned.clone()),
                    request_headers: request_headers_str.clone(),
                    request_body: request_body_str.clone(),
                    response_headers: None,
                    response_body: Some(
                        serde_json::json!({ "error": { "message": msg.clone() } }).to_string(),
                    ),
                },
            );
            return error_response(404, &msg);
        }
    };

    // Gate: embedding routes may only be accessed via the embeddings endpoint
    // and chat routes may not be accessed via embeddings.
    if is_embedding_request && !route.is_embedding_route() {
        let msg = format!(
            "route '{}' is type='{}', embeddings endpoint requires type='embedding'",
            route.virtual_model,
            route.normalized_route_type()
        );
        emit_log(
            &gw,
            &ingress_str,
            &ingress_str,
            &request_model,
            "",
            None,
            "",
            400,
            start.elapsed().as_millis() as f64,
            TokenUsage::default(),
            false,
            false,
            Some(msg.clone()),
            None,
            LogExtras {
                method: Some(method_owned.clone()),
                path: Some(path_owned.clone()),
                request_headers: request_headers_str.clone(),
                request_body: request_body_str.clone(),
                response_headers: None,
                response_body: Some(
                    serde_json::json!({ "error": { "message": msg.clone() } }).to_string(),
                ),
            },
        );
        return error_response(400, &msg);
    }
    if !is_embedding_request && route.is_embedding_route() {
        let msg = format!(
            "route '{}' is type='embedding', use /v1/embeddings",
            route.virtual_model
        );
        emit_log(
            &gw,
            &ingress_str,
            &ingress_str,
            &request_model,
            "",
            None,
            "",
            400,
            start.elapsed().as_millis() as f64,
            TokenUsage::default(),
            is_stream,
            false,
            Some(msg.clone()),
            None,
            LogExtras {
                method: Some(method_owned.clone()),
                path: Some(path_owned.clone()),
                request_headers: request_headers_str.clone(),
                request_body: request_body_str.clone(),
                response_headers: None,
                response_body: Some(
                    serde_json::json!({ "error": { "message": msg.clone() } }).to_string(),
                ),
            },
        );
        return error_response(400, &msg);
    }

    // ── Auth ─────────────────────────────────────────────────────────────────

    let access_store = GatewayProxyAccessStore::new(&gw);
    let auth_key = match authorize_route_access(&access_store, &route, &headers).await {
        Ok(v) => v,
        Err(resp) => {
            let status = resp.status().as_u16() as i32;
            emit_log(
                &gw,
                &ingress_str,
                &ingress_str,
                &request_model,
                "",
                None,
                "",
                status,
                start.elapsed().as_millis() as f64,
                TokenUsage::default(),
                is_stream,
                false,
                Some(format!("authorization failed: {status}")),
                None,
                LogExtras {
                    method: Some(method_owned.clone()),
                    path: Some(path_owned.clone()),
                    request_headers: request_headers_str.clone(),
                    request_body: request_body_str.clone(),
                    response_headers: None,
                    response_body: None,
                },
            );
            return resp;
        }
    };

    // ── Cache setup ───────────────────────────────────────────────────────────

    let cache_config = gw.effective_cache_config();
    let cache_backend = (**gw.cache_backend.load()).clone();
    let vector_store = (**gw.vector_store.load()).clone();
    let route_cache = resolve_route_cache(&route);
    let request_has_image = request_has_image_input(&internal);
    let exact_enabled_for_route = cache_config.exact.enabled
        && cache_backend.is_some()
        && route_cache.exact.is_some()
        && !request_has_image;
    let semantic_enabled_for_route = cache_config.semantic.enabled
        && vector_store.is_some()
        && route_cache.semantic.is_some()
        && !request_has_image;
    let semantic_write_temp_allowed = internal.temperature.unwrap_or(0.0) <= 0.0;
    let request_cache_key = if exact_enabled_for_route || semantic_enabled_for_route {
        Some(build_cache_key(&internal, ingress))
    } else {
        None
    };

    let exact_ttl = route_exact_ttl(&route_cache, cache_config.exact.default_ttl);
    let semantic_ttl = route_semantic_ttl(&route_cache, cache_config.semantic.default_ttl);
    let semantic_threshold =
        route_semantic_threshold(&route_cache, cache_config.semantic.similarity_threshold);
    let semantic_entry_key = request_cache_key
        .clone()
        .unwrap_or_else(|| build_cache_key(&internal, ingress));
    let semantic_embedding = extract_semantic_embedding_input(&internal);
    let semantic_partition = semantic_embedding
        .as_ref()
        .map(|(system_prompt, _)| build_semantic_partition(&internal.model, system_prompt));

    // ── Exact cache read ──────────────────────────────────────────────────────

    if let (Some(cache_backend), Some(key)) = (cache_backend.as_ref(), request_cache_key.as_deref())
        && exact_enabled_for_route
        && let Ok(Some(bytes)) = cache_backend.get(key).await
        && let Ok(cached_entry) = serde_json::from_slice::<CacheEntry>(&bytes)
    {
        let response = cached_entry_to_response(
            ingress,
            &cached_entry,
            is_stream,
            Some(key),
            "EXACT",
            None,
            cache_config.exact.stream_replay_tps,
            cache_config.exact.expose_headers,
        );
        let cached_usage = cached_entry.usage.clone();
        emit_log(
            &gw,
            &ingress_str,
            &ingress_str,
            &request_model,
            cached_entry
                .actual_model
                .as_deref()
                .unwrap_or(&request_model),
            auth_key.id.as_deref(),
            &cached_entry.provider_name,
            cached_entry.status_code as i32,
            start.elapsed().as_millis() as f64,
            cached_usage,
            is_stream,
            false,
            None,
            None,
            LogExtras {
                method: Some(method_owned.clone()),
                path: Some(path_owned.clone()),
                request_headers: request_headers_str.clone(),
                request_body: request_body_str.clone(),
                response_headers: None,
                response_body: serde_json::to_string(&cached_entry.payload).ok(),
            },
        );
        return response;
    }

    // ── Singleflight ─────────────────────────────────────────────────────────

    let mut singleflight_leader: Option<(String, broadcast::Sender<Vec<u8>>)> = None;
    if exact_enabled_for_route && let Some(key) = request_cache_key.as_ref() {
        match gw.cache_in_flight.entry(key.clone()) {
            DashEntry::Occupied(entry) => {
                let mut rx = entry.get().subscribe();
                drop(entry);
                if let Ok(Ok(bytes)) = timeout(Duration::from_secs(120), rx.recv()).await
                    && !bytes.is_empty()
                    && let Ok(cached_entry) = serde_json::from_slice::<CacheEntry>(&bytes)
                {
                    let response = cached_entry_to_response(
                        ingress,
                        &cached_entry,
                        is_stream,
                        Some(key),
                        "EXACT",
                        None,
                        cache_config.exact.stream_replay_tps,
                        cache_config.exact.expose_headers,
                    );
                    let cached_usage = cached_entry.usage.clone();
                    emit_log(
                        &gw,
                        &ingress_str,
                        &ingress_str,
                        &request_model,
                        cached_entry
                            .actual_model
                            .as_deref()
                            .unwrap_or(&request_model),
                        auth_key.id.as_deref(),
                        &cached_entry.provider_name,
                        cached_entry.status_code as i32,
                        start.elapsed().as_millis() as f64,
                        cached_usage,
                        is_stream,
                        false,
                        None,
                        None,
                        LogExtras {
                            method: Some(method_owned.clone()),
                            path: Some(path_owned.clone()),
                            request_headers: request_headers_str.clone(),
                            request_body: request_body_str.clone(),
                            response_headers: None,
                            response_body: serde_json::to_string(&cached_entry.payload).ok(),
                        },
                    );
                    return response;
                }
            }
            DashEntry::Vacant(entry) => {
                let (tx, _) = broadcast::channel(16);
                entry.insert(tx.clone());
                singleflight_leader = Some((key.clone(), tx));
            }
        }
    }

    // ── Semantic cache read ───────────────────────────────────────────────────

    let mut semantic_query_vector: Option<Vec<f32>> = None;
    if semantic_enabled_for_route
        && let (Some(vector_store), Some(partition), Some((_, semantic_text))) = (
            vector_store.as_ref(),
            semantic_partition.as_deref(),
            semantic_embedding.as_ref(),
        )
        && let Ok(vector) = compute_embedding(&gw, semantic_text).await
    {
        semantic_query_vector = Some(vector.clone());
        if let Ok(Some(hit)) = vector_store
            .search(partition, &vector, semantic_threshold)
            .await
            && let Ok(cached_entry) = serde_json::from_slice::<CacheEntry>(&hit.data)
            && !is_semantic_entry_expired(&cached_entry, semantic_ttl)
        {
            if exact_enabled_for_route
                && let (Some(cache_backend), Some(key)) =
                    (cache_backend.as_ref(), request_cache_key.as_deref())
            {
                let _ = cache_backend.set(key, &hit.data, Some(exact_ttl)).await;
            }
            let response = cached_entry_to_response(
                ingress,
                &cached_entry,
                is_stream,
                Some(&hit.key),
                "SEMANTIC",
                Some(hit.score),
                cache_config.semantic.stream_replay_tps,
                cache_config.semantic.expose_headers,
            );
            let cached_usage = cached_entry.usage.clone();
            emit_log(
                &gw,
                &ingress_str,
                &ingress_str,
                &request_model,
                cached_entry
                    .actual_model
                    .as_deref()
                    .unwrap_or(&request_model),
                auth_key.id.as_deref(),
                &cached_entry.provider_name,
                cached_entry.status_code as i32,
                start.elapsed().as_millis() as f64,
                cached_usage,
                is_stream,
                false,
                None,
                None,
                LogExtras {
                    method: Some(method_owned.clone()),
                    path: Some(path_owned.clone()),
                    request_headers: request_headers_str.clone(),
                    request_body: request_body_str.clone(),
                    response_headers: None,
                    response_body: serde_json::to_string(&cached_entry.payload).ok(),
                },
            );
            return response;
        }
    }

    let semantic_write_ctx = if semantic_enabled_for_route && semantic_write_temp_allowed {
        if let (Some(partition), Some((_, semantic_text))) =
            (semantic_partition.clone(), semantic_embedding.clone())
        {
            Some(SemanticWriteContext {
                partition,
                embedding_text: semantic_text,
                key: semantic_entry_key,
                query_vector: semantic_query_vector.clone(),
            })
        } else {
            None
        }
    } else {
        None
    };

    // ── Target iteration ──────────────────────────────────────────────────────

    let targets = load_route_targets(&gw, &route).await;
    if targets.is_empty() {
        emit_log(
            &gw,
            &ingress_str,
            &ingress_str,
            &request_model,
            "",
            auth_key.id.as_deref(),
            "",
            503,
            start.elapsed().as_millis() as f64,
            TokenUsage::default(),
            is_stream,
            false,
            Some("no route targets configured".to_string()),
            None,
            LogExtras {
                method: Some(method_owned.clone()),
                path: Some(path_owned.clone()),
                request_headers: request_headers_str.clone(),
                request_body: request_body_str.clone(),
                response_headers: None,
                response_body: None,
            },
        );
        return error_response(503, "no route targets configured");
    }
    let ordered_targets = TargetSelector::select_ordered(&route.strategy, &targets);
    if ordered_targets.is_empty() {
        emit_log(
            &gw,
            &ingress_str,
            &ingress_str,
            &request_model,
            "",
            auth_key.id.as_deref(),
            "",
            503,
            start.elapsed().as_millis() as f64,
            TokenUsage::default(),
            is_stream,
            false,
            Some("no route targets configured".to_string()),
            None,
            LogExtras {
                method: Some(method_owned.clone()),
                path: Some(path_owned.clone()),
                request_headers: request_headers_str.clone(),
                request_body: request_body_str.clone(),
                response_headers: None,
                response_body: None,
            },
        );
        return error_response(503, "no route targets configured");
    }

    let miss_expose_headers =
        cache_config.exact.expose_headers || cache_config.semantic.expose_headers;

    let mut last_response: Option<Response> = None;
    for target in ordered_targets {
        let target_key = format!("{}:{}", target.provider_id, target.model);
        if !gw.health_registry.is_healthy(&target_key) {
            continue;
        }
        let provider = match get_provider(&access_store, &target.provider_id).await {
            Ok(p) => p,
            Err(_) => continue,
        };
        let actual_model = if target.model.is_empty() || target.model == "*" {
            request_model.clone()
        } else {
            target.model.clone()
        };

        let mut internal_for_target = internal.clone();

        let provider_runtime = match gw.admin().resolve_provider_runtime(&provider).await {
            Ok(runtime) => runtime,
            Err(e) => {
                last_response = Some(error_response(
                    502,
                    &format!("provider credential error: {e}"),
                ));
                continue;
            }
        };

        // Resolve egress protocol + base URL via negotiate().
        let provider_protocols = ProviderProtocols::from_provider(&provider);
        let mut req_ctx = RequestContext::new(ingress, std::time::Duration::from_secs(30));
        let plan = match negotiate(ingress, None, Some(&provider_protocols), &mut req_ctx) {
            Ok(p) => p,
            Err(e) => {
                last_response = Some(e.render(None));
                continue;
            }
        };
        let egress = plan.egress;
        let egress_base_url = if let Some(base_url_override) = provider_runtime
            .binding
            .base_url_override
            .clone()
            .filter(|v| !v.trim().is_empty())
        {
            base_url_override
        } else if plan.base_url.is_empty() {
            provider.base_url.clone()
        } else {
            plan.base_url.clone()
        };

        // Look up Vendor for this vendor_id.
        let vendor_id = provider
            .vendor
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("custom");
        let adapter = match VendorRegistry::global().get_vendor(vendor_id) {
            Some(a) => a.clone(),
            None => {
                last_response = Some(error_response(
                    503,
                    &format!("no vendor registered for '{vendor_id}'"),
                ));
                continue;
            }
        };

        let credential = provider_runtime.access_token.clone();
        let ctx = ProviderCtx {
            provider: &provider,
            protocol: egress,
            egress_base_url: &egress_base_url,
            api_key: &credential,
            actual_model: &actual_model,
            credential: None,
            gw: &gw,
            disable_default_auth: provider_runtime.binding.disable_default_auth,
        };

        // Build outbound request — PassThrough (Native + no mutations) or full 7-step pipeline.
        let passthrough_req =
            plan.mode == ProtocolMode::Native && !adapter.declared_request_mutations();
        let passthrough_resp =
            plan.mode == ProtocolMode::Native && !adapter.declared_response_mutations();
        let mut outbound = if passthrough_req {
            let raw = envelope.body.clone().unwrap_or_default();
            match crate::provider::common::pipeline::passthrough_run(adapter.as_ref(), raw, &ctx)
                .await
            {
                Ok(o) => o,
                Err(e) => {
                    last_response = Some(e.render(None));
                    continue;
                }
            }
        } else {
            match adapter.build_request(&mut internal_for_target, &ctx).await {
                Ok(o) => o,
                Err(e) => {
                    last_response = Some(e.render(None));
                    continue;
                }
            }
        };

        // Merge runtime-binding extra headers (runtime binding < adapter, adapter wins).
        match runtime_binding_headers(&provider_runtime.binding) {
            Ok(binding_hdrs) => {
                let mut merged = binding_hdrs;
                merged.extend(outbound.headers);
                outbound.headers = merged;
            }
            Err(e) => {
                last_response = Some(error_response(
                    502,
                    &format!("provider runtime binding error: {e}"),
                ));
                continue;
            }
        }

        let client = match gw.http_client_for_provider(provider.use_proxy).await {
            Ok(http_client) => ProxyClient::new(http_client),
            Err(e) => {
                let msg = format!("provider transport error: {e}");
                last_response = Some(error_response(502, &msg));
                continue;
            }
        };

        let egress_str = egress.to_string();
        let egress_caps = egress.handler().capabilities();
        let upstream_forces_stream = egress_caps.force_upstream_stream;

        let response = if is_stream {
            handle_stream(
                gw.clone(),
                client,
                &outbound.url,
                outbound.headers,
                &provider,
                egress,
                ingress,
                outbound.body,
                &ingress_str,
                &egress_str,
                &request_model,
                &actual_model,
                auth_key.id.as_deref(),
                start,
                request_cache_key.as_deref(),
                exact_enabled_for_route,
                Some(exact_ttl),
                semantic_write_ctx.clone(),
                singleflight_leader.as_ref().map(|(k, _)| k.as_str()),
                singleflight_leader.as_ref().map(|(_, tx)| tx.clone()),
                miss_expose_headers,
                &method_owned,
                &path_owned,
                request_headers_str.clone(),
                request_body_str.clone(),
                passthrough_resp,
            )
            .await
        } else if upstream_forces_stream {
            handle_non_stream_via_upstream_stream(
                gw.clone(),
                client,
                &outbound.url,
                outbound.headers,
                &provider,
                egress,
                ingress,
                outbound.body,
                &ingress_str,
                &egress_str,
                &request_model,
                &actual_model,
                auth_key.id.as_deref(),
                start,
                request_cache_key.as_deref(),
                exact_enabled_for_route,
                Some(exact_ttl),
                semantic_write_ctx.clone(),
                miss_expose_headers,
            )
            .await
        } else {
            handle_non_stream(
                gw.clone(),
                client,
                &outbound.url,
                outbound.headers,
                &provider,
                adapter.as_ref(),
                &ctx,
                egress,
                ingress,
                outbound.body,
                &ingress_str,
                &egress_str,
                &request_model,
                &actual_model,
                auth_key.id.as_deref(),
                start,
                request_cache_key.as_deref(),
                exact_enabled_for_route,
                Some(exact_ttl),
                semantic_write_ctx.clone(),
                miss_expose_headers,
                &method_owned,
                &path_owned,
                request_headers_str.clone(),
                request_body_str.clone(),
                passthrough_resp,
            )
            .await
        };

        let status = response.status().as_u16();
        if status < 400 {
            if !is_stream {
                finalize_singleflight(&gw, singleflight_leader.as_ref(), true).await;
            }
            gw.health_registry.record_success(&target_key);
            return response;
        }
        gw.health_registry.record_failure(&target_key);
        if is_retryable(status) {
            last_response = Some(response);
            continue;
        }
        finalize_singleflight(&gw, singleflight_leader.as_ref(), false).await;
        return response;
    }

    finalize_singleflight(&gw, singleflight_leader.as_ref(), false).await;
    last_response.unwrap_or_else(|| {
        emit_log(
            &gw,
            &ingress_str,
            &ingress_str,
            &request_model,
            "",
            auth_key.id.as_deref(),
            "",
            502,
            start.elapsed().as_millis() as f64,
            TokenUsage::default(),
            is_stream,
            false,
            Some("all route targets failed".to_string()),
            None,
            LogExtras {
                method: Some(method_owned.clone()),
                path: Some(path_owned.clone()),
                request_headers: request_headers_str.clone(),
                request_body: request_body_str.clone(),
                response_headers: None,
                response_body: None,
            },
        );
        error_response(502, "all route targets failed")
    })
}

/// Legacy entry point: takes a raw `Value` body, wraps it in a `RawEnvelope`,
/// decodes it, and calls `dispatch_pipeline`.
pub async fn dispatch(
    gw: Gateway,
    headers: HeaderMap,
    body: Value,
    ingress: ProtocolId,
    method: &'static str,
    path: &'static str,
    _ctx: &mut RequestContext,
) -> Response {
    let request_headers_str = headers_to_json(&headers);
    let request_body_str = serde_json::to_string(&body).ok();

    let flat_headers: std::collections::HashMap<String, String> = headers
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|vs| (k.as_str().to_lowercase(), vs.to_string()))
        })
        .collect();
    let envelope = RawEnvelope::new(Some(body.clone()), flat_headers, method, path);

    let decoder = ingress.handler().make_decoder();
    let internal = match decoder.decode_request(body) {
        Ok(r) => r,
        Err(e) => {
            let ingress_str = ingress.to_string();
            let msg = format!("invalid request: {e}");
            emit_log(
                &gw,
                &ingress_str,
                &ingress_str,
                "",
                "",
                None,
                "",
                400,
                0.0,
                TokenUsage::default(),
                false,
                false,
                Some(msg.clone()),
                None,
                LogExtras {
                    method: Some(method.into()),
                    path: Some(path.into()),
                    request_headers: request_headers_str.clone(),
                    request_body: request_body_str.clone(),
                    response_headers: None,
                    response_body: Some(
                        serde_json::json!({ "error": { "message": msg.clone() } }).to_string(),
                    ),
                },
            );
            return error_response(400, &msg);
        }
    };

    let request: AiRequest = internal.into();
    dispatch_pipeline(gw, headers, envelope, request, ingress).await
}

// ── Non-streaming response handler ───────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn handle_non_stream(
    gw: Gateway,
    client: ProxyClient,
    url: &str,
    headers: ReqwestHeaderMap,
    provider: &Provider,
    adapter: &dyn crate::provider::vendor::Vendor,
    ctx: &ProviderCtx<'_>,
    egress: ProtocolId,
    ingress: ProtocolId,
    body: Value,
    ingress_str: &str,
    egress_str: &str,
    request_model: &str,
    actual_model: &str,
    api_key_id: Option<&str>,
    start: Instant,
    cache_key: Option<&str>,
    allow_exact_store: bool,
    exact_cache_ttl: Option<Duration>,
    semantic_write_ctx: Option<SemanticWriteContext>,
    expose_headers: bool,
    ingress_method: &str,
    ingress_path: &str,
    request_headers_str: Option<String>,
    request_body_str: Option<String>,
    // When true: Native protocol + no response mutations → skip IR round-trip.
    passthrough_resp: bool,
) -> Response {
    let make_extras = |response_body: Option<String>, resp_headers: Option<String>| LogExtras {
        method: Some(ingress_method.to_string()),
        path: Some(ingress_path.to_string()),
        request_headers: request_headers_str.clone(),
        request_body: request_body_str.clone(),
        response_headers: resp_headers,
        response_body,
    };

    let call_result = match client.call_non_stream(url, headers, body.clone()).await {
        Ok(r) => r,
        Err(e) => {
            emit_log(
                &gw, ingress_str, egress_str, request_model, actual_model,
                api_key_id, &provider.name, 502, start.elapsed().as_millis() as f64,
                TokenUsage::default(), false, false, Some(e.to_string()), None,
                make_extras(Some(
                    serde_json::json!({ "error": { "message": format!("upstream error: {e}") } }).to_string(),
                ), None),
            );
            return error_response(502, &format!("upstream error: {e}"));
        }
    };

    let (resp, status, upstream_headers) = call_result;
    let upstream_hdrs_str = headers_to_json(&upstream_headers);

    if status >= 400 {
        let body_str = serde_json::to_string(&resp).ok();
        let preview = body_str.as_ref().map(|s| s.chars().take(500).collect());
        emit_log(
            &gw,
            ingress_str,
            egress_str,
            request_model,
            actual_model,
            api_key_id,
            &provider.name,
            status as i32,
            start.elapsed().as_millis() as f64,
            TokenUsage::default(),
            false,
            false,
            preview,
            None,
            make_extras(body_str, upstream_hdrs_str.clone()),
        );
        return (
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(resp),
        )
            .into_response();
    }

    // Embeddings: passthrough response (parse_response is not implemented for codec).
    if egress.handler().capabilities().embeddings {
        let usage = crate::protocol::codec::openai_compatible::embeddings::parse_usage(&resp);
        let resp_str = serde_json::to_string(&resp).ok();
        let preview = resp_str.as_ref().map(|s| s.chars().take(500).collect());
        emit_log(
            &gw,
            ingress_str,
            egress_str,
            request_model,
            actual_model,
            api_key_id,
            &provider.name,
            status as i32,
            start.elapsed().as_millis() as f64,
            usage,
            false,
            false,
            None,
            preview,
            make_extras(resp_str, upstream_hdrs_str.clone()),
        );
        return (
            StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
            Json(resp),
        )
            .into_response();
    }

    // PassThrough: Native protocol + no response mutations → forward upstream JSON verbatim,
    // skipping the IR round-trip (parse_response → InternalResponse → format_response).
    if passthrough_resp {
        tracing::debug!(
            mode = "passthrough",
            egress = egress_str,
            "bypassing IR round-trip"
        );
        let resp_str = serde_json::to_string(&resp).ok();
        let preview = resp_str.as_ref().map(|s| s.chars().take(500).collect());
        emit_log(
            &gw,
            ingress_str,
            egress_str,
            request_model,
            actual_model,
            api_key_id,
            &provider.name,
            status as i32,
            start.elapsed().as_millis() as f64,
            TokenUsage::default(),
            false,
            false,
            None,
            preview,
            make_extras(resp_str, upstream_hdrs_str.clone()),
        );
        return (
            StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
            Json(resp),
        )
            .into_response();
    }

    // Parse response via ProviderAdapter.
    let inbound = InboundResponse { status, body: resp };
    let mut internal_resp = match adapter.parse_response(inbound, ctx).await {
        Ok(r) => r,
        Err(e) => {
            emit_log(
                &gw,
                ingress_str,
                egress_str,
                request_model,
                actual_model,
                api_key_id,
                &provider.name,
                500,
                start.elapsed().as_millis() as f64,
                TokenUsage::default(),
                false,
                false,
                Some(format!("parse error: {e}")),
                None,
                make_extras(
                    Some(
                        serde_json::json!({ "error": { "message": format!("parse error: {e}") } })
                            .to_string(),
                    ),
                    upstream_hdrs_str.clone(),
                ),
            );
            return error_response(500, &format!("parse error: {e}"));
        }
    };

    // Ensure actual_model is set in the response.
    if internal_resp.model.is_empty() {
        internal_resp.model = actual_model.to_string();
    }

    let is_tool = !internal_resp.tool_calls.is_empty();
    let usage = internal_resp.usage.clone();
    let formatter = ingress.handler().make_response_formatter();
    let output = formatter.format_response(&internal_resp);

    let response_body_full = serde_json::to_string(&output).ok();
    let response_preview = response_body_full
        .as_ref()
        .map(|s| s.chars().take(500).collect());
    emit_log(
        &gw,
        ingress_str,
        egress_str,
        request_model,
        actual_model,
        api_key_id,
        &provider.name,
        status as i32,
        start.elapsed().as_millis() as f64,
        usage.clone(),
        false,
        is_tool,
        None,
        response_preview,
        make_extras(response_body_full, upstream_hdrs_str),
    );

    let mut response = (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(output.clone()),
    )
        .into_response();
    set_cache_headers(&mut response, "MISS", cache_key, None, expose_headers);

    if status < 400 && !is_tool {
        let entry = CacheEntry {
            payload: output,
            status_code: status,
            provider_name: provider.name.clone(),
            actual_model: Some(actual_model.to_string()),
            usage,
            created_at_epoch_ms: chrono::Utc::now().timestamp_millis(),
            internal_response: Some(internal_resp),
        };
        if let Ok(bytes) = serde_json::to_vec(&entry) {
            if allow_exact_store {
                let cache_backend = (**gw.cache_backend.load()).clone();
                if let (Some(key), Some(cache_backend)) = (cache_key, cache_backend.as_ref()) {
                    let _ = cache_backend.set(key, &bytes, exact_cache_ttl).await;
                }
            }
            let vector_store = (**gw.vector_store.load()).clone();
            if let (Some(vector_store), Some(ctx)) =
                (vector_store.as_ref(), semantic_write_ctx.as_ref())
            {
                let vector = if let Some(existing) = ctx.query_vector.clone() {
                    Some(existing)
                } else {
                    compute_embedding(&gw, &ctx.embedding_text).await.ok()
                };
                if let Some(vector) = vector {
                    let _ = vector_store
                        .upsert(&ctx.partition, ctx.key.clone(), vector, bytes)
                        .await;
                }
            }
        }
    }
    response
}

// ── Force-stream non-stream handler ──────────────────────────────────────────

/// Consume a streaming upstream response and return a non-streaming client
/// response. Used when the egress protocol forces `stream: true` upstream
/// (e.g. Responses API) but the ingress client requested non-stream.
#[allow(clippy::too_many_arguments)]
async fn handle_non_stream_via_upstream_stream(
    gw: Gateway,
    client: ProxyClient,
    url: &str,
    headers: ReqwestHeaderMap,
    provider: &Provider,
    egress: ProtocolId,
    ingress: ProtocolId,
    body: Value,
    ingress_str: &str,
    egress_str: &str,
    request_model: &str,
    actual_model: &str,
    api_key_id: Option<&str>,
    start: Instant,
    cache_key: Option<&str>,
    allow_exact_store: bool,
    exact_cache_ttl: Option<Duration>,
    semantic_write_ctx: Option<SemanticWriteContext>,
    expose_headers: bool,
) -> Response {
    let call_result = match client.call_stream(url, headers, body.clone()).await {
        Ok(r) => r,
        Err(e) => {
            emit_log(
                &gw,
                ingress_str,
                egress_str,
                request_model,
                actual_model,
                api_key_id,
                &provider.name,
                502,
                start.elapsed().as_millis() as f64,
                TokenUsage::default(),
                false,
                false,
                Some(e.to_string()),
                None,
                LogExtras::default(),
            );
            return error_response(502, &format!("upstream error: {e}"));
        }
    };

    let (resp, status) = call_result;
    let upstream_hdrs_str = headers_to_json(resp.headers());

    if status >= 400 {
        let err_body: Value = resp
            .json()
            .await
            .unwrap_or_else(|_| serde_json::json!({"error": {"message": "upstream error"}}));
        emit_log(
            &gw,
            ingress_str,
            egress_str,
            request_model,
            actual_model,
            api_key_id,
            &provider.name,
            status as i32,
            start.elapsed().as_millis() as f64,
            TokenUsage::default(),
            false,
            false,
            Some(err_body.to_string()),
            None,
            LogExtras {
                response_headers: upstream_hdrs_str,
                response_body: serde_json::to_string(&err_body).ok(),
                ..LogExtras::default()
            },
        );
        return (
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(err_body),
        )
            .into_response();
    }

    let mut stream_parser = egress.handler().make_stream_parser();
    let mut byte_stream = resp.bytes_stream();
    let mut accumulator = StreamResponseAccumulator::default();

    while let Some(chunk) = byte_stream.next().await {
        let bytes = match chunk {
            Ok(b) => b,
            Err(e) => {
                emit_log(
                    &gw,
                    ingress_str,
                    egress_str,
                    request_model,
                    actual_model,
                    api_key_id,
                    &provider.name,
                    502,
                    start.elapsed().as_millis() as f64,
                    TokenUsage::default(),
                    false,
                    false,
                    Some(format!("stream read error: {e}")),
                    None,
                    LogExtras::default(),
                );
                return error_response(502, &format!("upstream stream error: {e}"));
            }
        };
        let text = String::from_utf8_lossy(&bytes);
        if let Ok(deltas) = stream_parser.parse_chunk(&text) {
            accumulator.apply_all(&deltas);
        }
    }

    if let Ok(deltas) = stream_parser.finish() {
        accumulator.apply_all(&deltas);
    }

    let mut internal_resp = accumulator.into_internal_response();
    if internal_resp.id.is_empty() {
        internal_resp.id = format!("chatcmpl-{}", uuid::Uuid::new_v4().simple());
    }
    if internal_resp.model.is_empty() {
        internal_resp.model = actual_model.to_string();
    }
    if internal_resp.stop_reason.is_none() {
        internal_resp.stop_reason = Some("stop".to_string());
    }
    crate::protocol::codec::reasoning::normalize_response_reasoning(&mut internal_resp);

    let is_tool = !internal_resp.tool_calls.is_empty();
    let usage = internal_resp.usage.clone();
    let formatter = ingress.handler().make_response_formatter();
    let output = formatter.format_response(&internal_resp);

    let response_preview = serde_json::to_string(&output)
        .ok()
        .map(|s| s.chars().take(500).collect());
    emit_log(
        &gw,
        ingress_str,
        egress_str,
        request_model,
        actual_model,
        api_key_id,
        &provider.name,
        status as i32,
        start.elapsed().as_millis() as f64,
        usage.clone(),
        false,
        is_tool,
        None,
        response_preview,
        LogExtras {
            response_headers: upstream_hdrs_str,
            response_body: serde_json::to_string(&output).ok(),
            ..LogExtras::default()
        },
    );

    let mut response = (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(output.clone()),
    )
        .into_response();
    set_cache_headers(&mut response, "MISS", cache_key, None, expose_headers);

    if status < 400 && !is_tool {
        let entry = CacheEntry {
            payload: output,
            status_code: status,
            provider_name: provider.name.clone(),
            actual_model: Some(actual_model.to_string()),
            usage,
            created_at_epoch_ms: chrono::Utc::now().timestamp_millis(),
            internal_response: Some(internal_resp),
        };
        if let Ok(bytes) = serde_json::to_vec(&entry) {
            if allow_exact_store {
                let cache_backend = (**gw.cache_backend.load()).clone();
                if let (Some(key), Some(cache_backend)) = (cache_key, cache_backend.as_ref()) {
                    let _ = cache_backend.set(key, &bytes, exact_cache_ttl).await;
                }
            }
            let vector_store = (**gw.vector_store.load()).clone();
            if let (Some(vector_store), Some(ctx)) =
                (vector_store.as_ref(), semantic_write_ctx.as_ref())
            {
                let vector = if let Some(existing) = ctx.query_vector.clone() {
                    Some(existing)
                } else {
                    compute_embedding(&gw, &ctx.embedding_text).await.ok()
                };
                if let Some(vector) = vector {
                    let _ = vector_store
                        .upsert(&ctx.partition, ctx.key.clone(), vector, bytes)
                        .await;
                }
            }
        }
    }
    response
}

// ── Streaming response handler ────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn handle_stream(
    gw: Gateway,
    client: ProxyClient,
    url: &str,
    headers: ReqwestHeaderMap,
    provider: &Provider,
    egress: ProtocolId,
    ingress: ProtocolId,
    body: Value,
    ingress_str: &str,
    egress_str: &str,
    request_model: &str,
    actual_model: &str,
    api_key_id: Option<&str>,
    start: Instant,
    cache_key: Option<&str>,
    allow_exact_store: bool,
    exact_cache_ttl: Option<Duration>,
    semantic_write_ctx: Option<SemanticWriteContext>,
    singleflight_key: Option<&str>,
    singleflight_tx: Option<broadcast::Sender<Vec<u8>>>,
    expose_headers: bool,
    ingress_method: &str,
    ingress_path: &str,
    request_headers_str: Option<String>,
    request_body_str: Option<String>,
    passthrough_resp: bool,
) -> Response {
    let make_extras_owned = {
        let method = ingress_method.to_string();
        let path_s = ingress_path.to_string();
        let rh = request_headers_str.clone();
        let rb = request_body_str.clone();
        move |response_body: Option<String>, resp_headers: Option<String>| LogExtras {
            method: Some(method.clone()),
            path: Some(path_s.clone()),
            request_headers: rh.clone(),
            request_body: rb.clone(),
            response_headers: resp_headers,
            response_body,
        }
    };
    let call_result = match client.call_stream(url, headers, body.clone()).await {
        Ok(r) => r,
        Err(e) => {
            emit_log(
                &gw, ingress_str, egress_str, request_model, actual_model,
                api_key_id, &provider.name, 502, start.elapsed().as_millis() as f64,
                TokenUsage::default(), true, false, Some(e.to_string()), None,
                make_extras_owned(Some(
                    serde_json::json!({ "error": { "message": format!("upstream error: {e}") } }).to_string(),
                ), None),
            );
            return error_response(502, &format!("upstream error: {e}"));
        }
    };

    let (resp, status) = call_result;
    let upstream_hdrs_str = headers_to_json(resp.headers());

    if status >= 400 {
        let err_body: Value = resp
            .json()
            .await
            .unwrap_or_else(|_| serde_json::json!({"error": {"message": "upstream error"}}));
        let err_body_str = serde_json::to_string(&err_body).ok();
        emit_log(
            &gw,
            ingress_str,
            egress_str,
            request_model,
            actual_model,
            api_key_id,
            &provider.name,
            status as i32,
            start.elapsed().as_millis() as f64,
            TokenUsage::default(),
            true,
            false,
            Some(err_body.to_string()),
            None,
            make_extras_owned(err_body_str, upstream_hdrs_str.clone()),
        );
        return (
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(err_body),
        )
            .into_response();
    }

    // ── Byte-level SSE passthrough ────────────────────────────────────────────
    // Used when ingress == egress protocol and the vendor declares no response
    // mutations (passthrough_resp=true). Upstream bytes are forwarded verbatim;
    // a side-channel parser accumulates usage stats for logging only.
    if passthrough_resp {
        let (pt_tx, pt_rx) = tokio::sync::mpsc::channel::<Result<Bytes, Infallible>>(64);

        let gw_pt = gw.clone();
        let provider_name_pt = provider.name.clone();
        let ingress_s_pt = ingress_str.to_string();
        let egress_s_pt = egress_str.to_string();
        let req_model_pt = request_model.to_string();
        let act_model_pt = actual_model.to_string();
        let key_id_pt = api_key_id.map(ToString::to_string);
        let leader_key_pt = singleflight_key.map(ToString::to_string);
        let leader_tx_pt = singleflight_tx.clone();
        let ingress_method_pt = ingress_method.to_string();
        let ingress_path_pt = ingress_path.to_string();
        let request_headers_pt = request_headers_str.clone();
        let request_body_pt = request_body_str.clone();
        let upstream_hdrs_pt = upstream_hdrs_str.clone();

        tokio::spawn(async move {
            let mut log_buf: Vec<u8> = Vec::new();
            let mut byte_stream = resp.bytes_stream();
            let mut stream_error: Option<String> = None;

            while let Some(result) = byte_stream.next().await {
                match result {
                    Ok(b) => {
                        log_buf.extend_from_slice(&b);
                        if pt_tx.send(Ok(b)).await.is_err() {
                            break; // client disconnected
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "upstream stream error during passthrough");
                        stream_error = Some(e.to_string());
                        // Emit an Anthropic-protocol error event so the client
                        // gets an explicit signal instead of a truncated stream.
                        let msg = e.to_string().replace('"', "\\\"");
                        let err_sse = format!(
                            "event: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"stream_error\",\"message\":\"{msg}\"}}}}\n\n"
                        );
                        let _ = pt_tx.send(Ok(Bytes::from(err_sse))).await;
                        break;
                    }
                }
            }

            // Parse accumulated buffer for usage stats and log entry (best-effort).
            let log_text = String::from_utf8_lossy(&log_buf);
            let mut log_parser = egress.handler().make_stream_parser();
            let mut accumulator = StreamResponseAccumulator::default();
            if let Ok(deltas) = log_parser.parse_chunk(&log_text) {
                accumulator.apply_all(&deltas);
            }
            if let Ok(deltas) = log_parser.finish() {
                accumulator.apply_all(&deltas);
            }

            let mut internal = accumulator.into_internal_response();
            if internal.id.is_empty() {
                internal.id = format!("msg_{}", uuid::Uuid::new_v4().simple());
            }
            if internal.model.is_empty() {
                internal.model = act_model_pt.clone();
            }

            let aggregated_formatter = ingress.handler().make_response_formatter();
            let aggregated_output = aggregated_formatter.format_response(&internal);
            let aggregated_body_str = serde_json::to_string(&aggregated_output).ok();

            emit_log(
                &gw_pt,
                &ingress_s_pt,
                &egress_s_pt,
                &req_model_pt,
                &act_model_pt,
                key_id_pt.as_deref(),
                &provider_name_pt,
                200,
                start.elapsed().as_millis() as f64,
                internal.usage.clone(),
                true,
                !internal.tool_calls.is_empty(),
                stream_error,
                None,
                LogExtras {
                    method: Some(ingress_method_pt.clone()),
                    path: Some(ingress_path_pt.clone()),
                    request_headers: request_headers_pt.clone(),
                    request_body: request_body_pt.clone(),
                    response_headers: upstream_hdrs_pt,
                    response_body: aggregated_body_str,
                },
            );

            if let (Some(key), Some(tx)) = (leader_key_pt.as_deref(), leader_tx_pt.as_ref()) {
                let _ = tx.send(vec![]);
                gw_pt.cache_in_flight.remove(key);
            }
        });

        let stream = ReceiverStream::new(pt_rx);
        let body = Body::from_stream(stream);
        let mut response = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .header(header::CONNECTION, "keep-alive")
            .body(body)
            .unwrap();
        set_cache_headers(&mut response, "MISS", cache_key, None, expose_headers);
        return response;
    }

    // ── IR round-trip path ────────────────────────────────────────────────────
    let mut stream_parser = egress.handler().make_stream_parser();
    let mut stream_formatter = ingress.handler().make_stream_formatter();
    let mut byte_stream = resp.bytes_stream();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, Infallible>>(64);

    let gw_log = gw.clone();
    let provider_name = provider.name.clone();
    let ingress_s = ingress_str.to_string();
    let egress_s = egress_str.to_string();
    let req_model = request_model.to_string();
    let act_model = actual_model.to_string();
    let key_id = api_key_id.map(ToString::to_string);
    let cache_key_owned = cache_key.map(ToString::to_string);
    let leader_key_owned = singleflight_key.map(ToString::to_string);
    let leader_tx_owned = singleflight_tx.clone();
    let exact_cache_ttl_owned = exact_cache_ttl;
    let semantic_write_ctx_owned = semantic_write_ctx.clone();
    let ingress_method_owned = ingress_method.to_string();
    let ingress_path_owned = ingress_path.to_string();
    let request_headers_owned = request_headers_str.clone();
    let request_body_owned = request_body_str.clone();
    let upstream_hdrs_owned = upstream_hdrs_str;

    tokio::spawn(async move {
        let mut accumulator = StreamResponseAccumulator::default();
        while let Some(chunk) = byte_stream.next().await {
            let bytes = match chunk {
                Ok(b) => b,
                Err(e) => {
                    // P1: emit an explicit terminal event instead of silently breaking,
                    // so the client receives a defined stop_reason and does not hang.
                    tracing::warn!(error = %e, "upstream stream error; emitting terminal event");
                    let error_deltas = [StreamDelta::Done {
                        stop_reason: "error".to_string(),
                    }];
                    let events = stream_formatter.format_deltas(&error_deltas);
                    for ev in events {
                        let _ = tx.send(Ok(ev.to_sse_string())).await;
                    }
                    break;
                }
            };
            let text = String::from_utf8_lossy(&bytes);
            if let Ok(deltas) = stream_parser.parse_chunk(&text) {
                accumulator.apply_all(&deltas);
                let events = stream_formatter.format_deltas(&deltas);
                for ev in events {
                    if tx.send(Ok(ev.to_sse_string())).await.is_err() {
                        return;
                    }
                }
            }
        }

        if let Ok(deltas) = stream_parser.finish() {
            accumulator.apply_all(&deltas);
            let events = stream_formatter.format_deltas(&deltas);
            for ev in events {
                let _ = tx.send(Ok(ev.to_sse_string())).await;
            }
        }

        let done_events = stream_formatter.format_done();
        for ev in done_events {
            let _ = tx.send(Ok(ev.to_sse_string())).await;
        }

        let usage = stream_formatter.usage();
        let mut internal = accumulator.into_internal_response();
        if internal.usage.input_tokens == 0 && internal.usage.output_tokens == 0 {
            internal.usage = usage.clone();
        }
        if internal.id.is_empty() {
            internal.id = format!("chatcmpl-{}", uuid::Uuid::new_v4().simple());
        }
        if internal.model.is_empty() {
            internal.model = act_model.clone();
        }
        if internal.stop_reason.is_none() {
            internal.stop_reason = Some("stop".to_string());
        }

        let aggregated_formatter = ingress.handler().make_response_formatter();
        let aggregated_output = aggregated_formatter.format_response(&internal);
        let aggregated_body_str = serde_json::to_string(&aggregated_output).ok();
        emit_log(
            &gw_log,
            &ingress_s,
            &egress_s,
            &req_model,
            &act_model,
            key_id.as_deref(),
            &provider_name,
            200,
            start.elapsed().as_millis() as f64,
            internal.usage.clone(),
            true,
            !internal.tool_calls.is_empty(),
            None,
            None,
            LogExtras {
                method: Some(ingress_method_owned.clone()),
                path: Some(ingress_path_owned.clone()),
                request_headers: request_headers_owned.clone(),
                request_body: request_body_owned.clone(),
                response_headers: upstream_hdrs_owned,
                response_body: aggregated_body_str,
            },
        );

        let mut singleflight_payload: Option<Vec<u8>> = None;
        if allow_exact_store && internal.tool_calls.is_empty() {
            let cache_backend = (**gw_log.cache_backend.load()).clone();
            if let (Some(cache_backend), Some(cache_key)) =
                (cache_backend.as_ref(), cache_key_owned.as_deref())
            {
                let formatter = ingress.handler().make_response_formatter();
                let payload = formatter.format_response(&internal);
                let entry = CacheEntry {
                    payload,
                    status_code: 200,
                    provider_name: provider_name.clone(),
                    actual_model: Some(act_model.clone()),
                    usage: internal.usage.clone(),
                    created_at_epoch_ms: chrono::Utc::now().timestamp_millis(),
                    internal_response: Some(internal.clone()),
                };
                if let Ok(bytes) = serde_json::to_vec(&entry) {
                    let _ = cache_backend
                        .set(cache_key, &bytes, exact_cache_ttl_owned)
                        .await;
                    singleflight_payload = Some(bytes.clone());
                    let vector_store = (**gw_log.vector_store.load()).clone();
                    if let (Some(vector_store), Some(ctx)) =
                        (vector_store.as_ref(), semantic_write_ctx_owned.as_ref())
                    {
                        let vector = if let Some(existing) = ctx.query_vector.clone() {
                            Some(existing)
                        } else {
                            compute_embedding(&gw_log, &ctx.embedding_text).await.ok()
                        };
                        if let Some(vector) = vector {
                            let _ = vector_store
                                .upsert(&ctx.partition, ctx.key.clone(), vector, bytes)
                                .await;
                        }
                    }
                }
            }
        } else if internal.tool_calls.is_empty() {
            let vector_store = (**gw_log.vector_store.load()).clone();
            if let (Some(vector_store), Some(ctx)) =
                (vector_store.as_ref(), semantic_write_ctx_owned.as_ref())
            {
                let formatter = ingress.handler().make_response_formatter();
                let payload = formatter.format_response(&internal);
                let entry = CacheEntry {
                    payload,
                    status_code: 200,
                    provider_name: provider_name.clone(),
                    actual_model: Some(act_model.clone()),
                    usage: internal.usage.clone(),
                    created_at_epoch_ms: chrono::Utc::now().timestamp_millis(),
                    internal_response: Some(internal.clone()),
                };
                if let Ok(bytes) = serde_json::to_vec(&entry) {
                    let vector = if let Some(existing) = ctx.query_vector.clone() {
                        Some(existing)
                    } else {
                        compute_embedding(&gw_log, &ctx.embedding_text).await.ok()
                    };
                    if let Some(vector) = vector {
                        let _ = vector_store
                            .upsert(&ctx.partition, ctx.key.clone(), vector, bytes)
                            .await;
                    }
                }
            }
        }

        if let (Some(key), Some(tx)) = (leader_key_owned.as_deref(), leader_tx_owned.as_ref()) {
            let _ = tx.send(singleflight_payload.unwrap_or_default());
            gw_log.cache_in_flight.remove(key);
        }
    });

    let stream = ReceiverStream::new(rx);
    let body = Body::from_stream(stream);

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(body)
        .unwrap();
    set_cache_headers(&mut response, "MISS", cache_key, None, expose_headers);
    response
}

// ── Auth helpers ──────────────────────────────────────────────────────────────

struct AuthenticatedKey {
    id: Option<String>,
}

#[async_trait]
trait ProxyAccessStore {
    async fn get_active_provider(&self, id: &str) -> anyhow::Result<Option<Provider>>;
    async fn find_api_key(&self, raw_key: &str) -> anyhow::Result<Option<ApiKeyAccessRecord>>;
    async fn route_binding_exists(&self, api_key_id: &str, route_id: &str) -> anyhow::Result<bool>;
    async fn request_count_since(
        &self,
        api_key_id: &str,
        window: UsageWindow,
    ) -> anyhow::Result<i64>;
    async fn token_count_since(&self, api_key_id: &str, window: UsageWindow)
    -> anyhow::Result<i64>;
}

struct GatewayProxyAccessStore<'a> {
    gw: &'a Gateway,
}

impl<'a> GatewayProxyAccessStore<'a> {
    fn new(gw: &'a Gateway) -> Self {
        Self { gw }
    }
}

#[async_trait]
impl ProxyAccessStore for GatewayProxyAccessStore<'_> {
    async fn get_active_provider(&self, id: &str) -> anyhow::Result<Option<Provider>> {
        let provider = self.gw.storage.providers().get(id).await?;
        Ok(provider.filter(|p| p.is_enabled))
    }
    async fn find_api_key(&self, raw_key: &str) -> anyhow::Result<Option<ApiKeyAccessRecord>> {
        match self.gw.storage.auth() {
            Some(store) => store.find_api_key(raw_key).await,
            None => Ok(None),
        }
    }
    async fn route_binding_exists(&self, api_key_id: &str, route_id: &str) -> anyhow::Result<bool> {
        match self.gw.storage.auth() {
            Some(store) => store.route_binding_exists(api_key_id, route_id).await,
            None => Ok(false),
        }
    }
    async fn request_count_since(
        &self,
        api_key_id: &str,
        window: UsageWindow,
    ) -> anyhow::Result<i64> {
        match self.gw.storage.auth() {
            Some(store) => store.request_count_since(api_key_id, window).await,
            None => Ok(0),
        }
    }
    async fn token_count_since(
        &self,
        api_key_id: &str,
        window: UsageWindow,
    ) -> anyhow::Result<i64> {
        match self.gw.storage.auth() {
            Some(store) => store.token_count_since(api_key_id, window).await,
            None => Ok(0),
        }
    }
}

async fn authorize_route_access<S: ProxyAccessStore + ?Sized>(
    access_store: &S,
    route: &Route,
    headers: &HeaderMap,
) -> Result<AuthenticatedKey, Response> {
    if !route.access_control {
        return Ok(AuthenticatedKey { id: None });
    }

    let Some(raw_key) = extract_api_key(headers) else {
        return Err(error_response(401, "missing api key"));
    };

    let key_row = access_store
        .find_api_key(&raw_key)
        .await
        .map_err(|e| error_response(500, &format!("auth db error: {e}")))?;

    let Some(key_row) = key_row else {
        return Err(error_response(401, "invalid api key"));
    };

    if !key_row.is_enabled {
        return Err(error_response(403, "api key disabled"));
    }

    if let Some(expires) = key_row.expires_at.as_ref()
        && is_key_expired(expires)
    {
        return Err(error_response(403, "api key expired"));
    }

    let allowed = access_store
        .route_binding_exists(&key_row.id, &route.id)
        .await
        .map_err(|e| error_response(500, &format!("auth db error: {e}")))?;
    if !allowed {
        return Err(error_response(403, "api key not allowed for this route"));
    }

    if let Some(limit) = key_row.rpm.filter(|v| *v > 0) {
        let req_count = access_store
            .request_count_since(&key_row.id, UsageWindow::Minute)
            .await
            .map_err(|e| error_response(500, &format!("quota db error: {e}")))?;
        if req_count >= i64::from(limit) {
            return Err(error_response(429, "api key rpm quota exceeded"));
        }
    }

    if let Some(limit) = key_row.rpd.filter(|v| *v > 0) {
        let req_count = access_store
            .request_count_since(&key_row.id, UsageWindow::Day)
            .await
            .map_err(|e| error_response(500, &format!("quota db error: {e}")))?;
        if req_count >= i64::from(limit) {
            return Err(error_response(429, "api key rpd quota exceeded"));
        }
    }

    if let Some(limit) = key_row.tpm.filter(|v| *v > 0) {
        let token_count = access_store
            .token_count_since(&key_row.id, UsageWindow::Minute)
            .await
            .map_err(|e| error_response(500, &format!("quota db error: {e}")))?;
        if token_count >= i64::from(limit) {
            return Err(error_response(429, "api key tpm quota exceeded"));
        }
    }

    if let Some(limit) = key_row.tpd.filter(|v| *v > 0) {
        let token_count = access_store
            .token_count_since(&key_row.id, UsageWindow::Day)
            .await
            .map_err(|e| error_response(500, &format!("quota db error: {e}")))?;
        if token_count >= i64::from(limit) {
            return Err(error_response(429, "api key tpd quota exceeded"));
        }
    }

    Ok(AuthenticatedKey {
        id: Some(key_row.id),
    })
}

async fn get_provider<S: ProxyAccessStore + ?Sized>(
    access_store: &S,
    id: &str,
) -> anyhow::Result<Provider> {
    access_store
        .get_active_provider(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("provider not found or inactive: {id}"))
}

// Cache helpers (SemanticWriteContext, resolve_route_cache, route_*_ttl,
// is_semantic_entry_expired, request_has_image_input, extract_semantic_embedding_input,
// is_retryable, runtime_binding_headers, load_route_targets) are in util.rs.

fn set_cache_headers(
    response: &mut Response,
    cache_status: &str,
    key: Option<&str>,
    score: Option<f64>,
    expose_headers: bool,
) {
    if !expose_headers {
        return;
    }
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(cache_status) {
        headers.insert("X-NYRO-CACHE", value);
    }
    if let Some(key) = key
        && let Ok(value) = HeaderValue::from_str(key)
    {
        headers.insert("X-NYRO-CACHE-KEY", value);
    }
    if let Some(score) = score
        && let Ok(value) = HeaderValue::from_str(&format!("{score:.4}"))
    {
        headers.insert("X-NYRO-CACHE-SCORE", value);
    }
}

#[allow(clippy::too_many_arguments)]
fn cached_entry_to_response(
    ingress: ProtocolId,
    entry: &CacheEntry,
    is_stream: bool,
    cache_key: Option<&str>,
    cache_status: &str,
    score: Option<f64>,
    stream_replay_tps: u32,
    expose_headers: bool,
) -> Response {
    if is_stream && let Some(internal) = entry.internal_response.as_ref() {
        return replay_cached_stream(
            ingress,
            internal,
            cache_key,
            cache_status,
            score,
            stream_replay_tps,
            expose_headers,
        );
    }
    let mut response = (
        StatusCode::from_u16(entry.status_code).unwrap_or(StatusCode::OK),
        Json(entry.payload.clone()),
    )
        .into_response();
    set_cache_headers(
        &mut response,
        cache_status,
        cache_key,
        score,
        expose_headers,
    );
    response
}

fn replay_cached_stream(
    ingress: ProtocolId,
    internal: &InternalResponse,
    cache_key: Option<&str>,
    cache_status: &str,
    score: Option<f64>,
    stream_replay_tps: u32,
    expose_headers: bool,
) -> Response {
    let mut formatter = ingress.handler().make_stream_formatter();
    let deltas = internal_response_to_deltas(internal);
    let deltas = if stream_replay_tps > 0 {
        split_text_deltas(deltas, 4)
    } else {
        deltas
    };
    let mut payloads: Vec<String> = formatter
        .format_deltas(&deltas)
        .into_iter()
        .map(|ev| ev.to_sse_string())
        .collect();
    payloads.extend(
        formatter
            .format_done()
            .into_iter()
            .map(|ev| ev.to_sse_string()),
    );

    let interval = if stream_replay_tps > 0 {
        Some(std::time::Duration::from_micros(
            1_000_000 / stream_replay_tps as u64,
        ))
    } else {
        None
    };

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, Infallible>>(payloads.len().max(1));
    tokio::spawn(async move {
        for (i, payload) in payloads.into_iter().enumerate() {
            if i > 0
                && let Some(d) = interval
            {
                tokio::time::sleep(d).await;
            }
            if tx.send(Ok(payload)).await.is_err() {
                break;
            }
        }
    });

    let body = Body::from_stream(ReceiverStream::new(rx));
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(body)
        .unwrap();
    set_cache_headers(
        &mut response,
        cache_status,
        cache_key,
        score,
        expose_headers,
    );
    response
}

fn internal_response_to_deltas(internal: &InternalResponse) -> Vec<StreamDelta> {
    let mut deltas = vec![StreamDelta::MessageStart {
        id: if internal.id.is_empty() {
            format!("chatcmpl-{}", uuid::Uuid::new_v4().simple())
        } else {
            internal.id.clone()
        },
        model: internal.model.clone(),
    }];
    if let Some(reasoning) = &internal.reasoning_content
        && !reasoning.is_empty()
    {
        deltas.push(StreamDelta::ReasoningDelta(reasoning.clone()));
        if let Some(sig) = internal
            .reasoning_signature
            .as_ref()
            .filter(|s| !s.is_empty())
        {
            deltas.push(StreamDelta::ReasoningSignature(sig.clone()));
        }
    }
    if !internal.content.is_empty() {
        deltas.push(StreamDelta::TextDelta(internal.content.clone()));
    }
    for (index, tool_call) in internal.tool_calls.iter().enumerate() {
        deltas.push(StreamDelta::ToolCallStart {
            index,
            id: tool_call.id.clone(),
            name: tool_call.name.clone(),
        });
        if !tool_call.arguments.is_empty() {
            deltas.push(StreamDelta::ToolCallDelta {
                index,
                arguments: tool_call.arguments.clone(),
            });
        }
    }
    deltas.push(StreamDelta::Usage(internal.usage.clone()));
    deltas.push(StreamDelta::Done {
        stop_reason: internal
            .stop_reason
            .clone()
            .unwrap_or_else(|| "stop".to_string()),
    });
    deltas
}

fn split_text_deltas(deltas: Vec<StreamDelta>, chunk_chars: usize) -> Vec<StreamDelta> {
    deltas
        .into_iter()
        .flat_map(|d| match d {
            StreamDelta::TextDelta(text) => {
                let chars: Vec<char> = text.chars().collect();
                if chars.len() <= chunk_chars {
                    return vec![StreamDelta::TextDelta(text)];
                }
                chars
                    .chunks(chunk_chars)
                    .map(|c| StreamDelta::TextDelta(c.iter().collect()))
                    .collect()
            }
            StreamDelta::ReasoningDelta(text) => {
                let chars: Vec<char> = text.chars().collect();
                if chars.len() <= chunk_chars {
                    return vec![StreamDelta::ReasoningDelta(text)];
                }
                chars
                    .chunks(chunk_chars)
                    .map(|c| StreamDelta::ReasoningDelta(c.iter().collect()))
                    .collect()
            }
            other => vec![other],
        })
        .collect()
}

async fn finalize_singleflight(
    gw: &Gateway,
    leader: Option<&(String, broadcast::Sender<Vec<u8>>)>,
    success: bool,
) {
    let Some((key, tx)) = leader else {
        return;
    };
    let payload = if success {
        let cache_backend = (**gw.cache_backend.load()).clone();
        if let Some(cache_backend) = cache_backend.as_ref() {
            cache_backend
                .get(key)
                .await
                .ok()
                .flatten()
                .unwrap_or_default()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    let _ = tx.send(payload);
    gw.cache_in_flight.remove(key);
}

pub(crate) fn error_response(status: u16, message: &str) -> Response {
    let err: GatewayError = match status {
        400 => GatewayError::bad_request("bad_request", message),
        401 => GatewayError::Unauthorized {
            reason: AuthFailure::Invalid,
        },
        403 => GatewayError::Forbidden {
            reason: crate::error::AccessDenial::Custom(message.to_string()),
        },
        404 => GatewayError::RouteNotFound {
            model: message.to_string(),
        },
        429 => GatewayError::QuotaExceeded {
            window: crate::error::QuotaWindow {
                window_type: "request".to_string(),
                reset_at_secs: None,
            },
        },
        503 => GatewayError::provider_unavailable("unknown", message),
        502 => GatewayError::upstream_status("unknown", 502, Some(message.to_string())),
        _ => GatewayError::Internal {
            source: anyhow::anyhow!("{}", message),
        },
    };
    err.render(None)
}

// StreamResponseAccumulator and ensure_tool_index are in accumulator.rs.

// ── Semantic embedding computation ────────────────────────────────────────────

/// Compute an embedding vector for the given text using the configured
/// semantic-cache embedding route.
///
/// Uses `VendorRegistry` directly because this is an internal embedding
/// call on the embeddings endpoint, outside the main chat proxy pipeline.
async fn compute_embedding(gw: &Gateway, text: &str) -> anyhow::Result<Vec<f32>> {
    let runtime_cache = gw.effective_cache_config();
    let embedding_route = runtime_cache.semantic.embedding_route.trim();
    if embedding_route.is_empty() {
        anyhow::bail!("semantic cache embedding_route is empty");
    }
    let route = {
        let cache = gw.route_cache.read().await;
        cache.match_route(embedding_route).cloned()
    }
    .ok_or_else(|| anyhow::anyhow!("embedding route not found: {embedding_route}"))?;
    if !route.is_embedding_route() {
        anyhow::bail!("embedding route must be type='embedding': {embedding_route}");
    }

    let targets = load_route_targets(gw, &route).await;
    if targets.is_empty() {
        anyhow::bail!("embedding route has no targets: {embedding_route}");
    }
    let ordered_targets = TargetSelector::select_ordered(&route.strategy, &targets);
    let access_store = GatewayProxyAccessStore::new(gw);
    let mut missing_openai_endpoint = false;

    for target in ordered_targets {
        let provider = match get_provider(&access_store, &target.provider_id).await {
            Ok(p) => p,
            Err(_) => continue,
        };
        let provider_runtime = match gw.admin().resolve_provider_runtime(&provider).await {
            Ok(r) => r,
            Err(_) => continue,
        };
        let openai_base_url = provider_runtime
            .binding
            .base_url_override
            .clone()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| resolve_openai_base_url(&provider));
        let Some(openai_base_url) = openai_base_url else {
            missing_openai_endpoint = true;
            continue;
        };
        let actual_model = if target.model.is_empty() || target.model == "*" {
            embedding_route.to_string()
        } else {
            target.model.clone()
        };
        let extension = match VendorRegistry::global().resolve(&provider, OPENAI_EMBEDDINGS_V1) {
            Some(ext) => ext.clone(),
            None => continue,
        };
        let credential = provider_runtime.access_token.clone();
        let upstream_url;
        let mut request_headers;
        {
            let ctx = VendorCtx {
                provider: &provider,
                protocol_id: OPENAI_EMBEDDINGS_V1,
                api_key: &credential,
                actual_model: &actual_model,
                credential: None,
            };
            upstream_url = extension.build_url(&ctx, &openai_base_url, "/v1/embeddings");
            request_headers = match runtime_binding_headers(&provider_runtime.binding) {
                Ok(h) => h,
                Err(_) => continue,
            };
            if !provider_runtime.binding.disable_default_auth {
                request_headers.extend(extension.auth_headers(&ctx));
            }
        }
        let client = match gw.http_client_for_provider(provider.use_proxy).await {
            Ok(c) => ProxyClient::new(c),
            Err(_) => continue,
        };
        let request_body = serde_json::json!({ "model": actual_model, "input": text });
        match client
            .call_non_stream(&upstream_url, request_headers, request_body)
            .await
        {
            Ok((payload, status, _)) if status < 400 => {
                if let Some(vector) = parse_embedding_vector(&payload) {
                    return Ok(vector);
                }
            }
            _ => {}
        }
    }

    if missing_openai_endpoint {
        anyhow::bail!("embedding route targets must expose protocol_endpoints.openai");
    }
    anyhow::bail!("failed to compute embedding from route: {embedding_route}")
}

fn parse_embedding_vector(payload: &Value) -> Option<Vec<f32>> {
    let embedding = payload
        .get("data")
        .and_then(Value::as_array)?
        .first()?
        .get("embedding")
        .and_then(Value::as_array)?;
    let mut out = Vec::with_capacity(embedding.len());
    for v in embedding {
        out.push(v.as_f64()? as f32);
    }
    if out.is_empty() { None } else { Some(out) }
}

fn resolve_openai_base_url(provider: &Provider) -> Option<String> {
    let protocols = ProviderProtocols::from_provider(provider);
    if !protocols.supports(OPENAI_CHAT_COMPLETIONS_V1) {
        return None;
    }
    let resolved = protocols.resolve_egress(OPENAI_CHAT_COMPLETIONS_V1);
    let trimmed = resolved.base_url.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
