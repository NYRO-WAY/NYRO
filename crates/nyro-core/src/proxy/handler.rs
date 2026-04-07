use std::collections::{BTreeSet, HashSet};
use std::convert::Infallible;
use std::time::Instant;

use chrono::{NaiveDateTime, Utc};
use async_trait::async_trait;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::StreamExt;
use dashmap::mapref::entry::Entry as DashEntry;
use serde_json::Value;
use tokio::sync::broadcast;
use tokio::time::{Duration, timeout};
use tokio_stream::wrappers::ReceiverStream;

use crate::db::models::{Provider, Route, RouteTarget};
use crate::cache::entry::CacheEntry;
use crate::cache::key::build_cache_key;
use crate::cache::CacheMode;
use crate::logging::LogEntry;
use crate::protocol::gemini::decoder::GeminiDecoder;
use crate::protocol::types::*;
use crate::protocol::{Protocol, ProviderProtocols};
use crate::proxy::adapter::{self, ProviderAdapter};
use crate::proxy::client::ProxyClient;
use crate::router::TargetSelector;
use crate::storage::traits::{ApiKeyAccessRecord, UsageWindow};
use crate::Gateway;

// ── OpenAI ingress: POST /v1/chat/completions ──

pub async fn openai_proxy(
    State(gw): State<Gateway>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    universal_proxy(gw, headers, body, Protocol::OpenAI).await
}

// ── OpenAI Responses API ingress: POST /v1/responses ──

pub async fn responses_proxy(
    State(gw): State<Gateway>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    universal_proxy(gw, headers, body, Protocol::ResponsesAPI).await
}

// ── OpenAI embeddings ingress: POST /v1/embeddings ──
pub async fn embeddings_proxy(
    State(gw): State<Gateway>,
    headers: HeaderMap,
    Json(mut body): Json<Value>,
) -> Response {
    let request_model = body
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string);
    let Some(request_model) = request_model else {
        return error_response(400, "model is required");
    };

    let route = {
        let cache = gw.route_cache.read().await;
        cache.match_route(&request_model).cloned()
    };
    let Some(route) = route else {
        return error_response(404, &format!("no route for model: {request_model}"));
    };

    let access_store = GatewayProxyAccessStore::new(&gw);
    let auth_key = match authorize_route_access(&access_store, &route, &headers).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    let targets = load_route_targets(&gw, &route).await;
    if targets.is_empty() {
        return error_response(503, "no route targets configured");
    }
    let ordered_targets = TargetSelector::select_ordered(&route.strategy, &targets);
    let start = Instant::now();
    let mut last_error: Option<Response> = None;
    for target in ordered_targets {
        let provider = match get_provider(&access_store, &target.provider_id).await {
            Ok(p) => p,
            Err(_) => continue,
        };
        let actual_model = if target.model.is_empty() || target.model == "*" {
            request_model.clone()
        } else {
            target.model.clone()
        };
        if let Some(obj) = body.as_object_mut() {
            obj.insert("model".into(), Value::String(actual_model.clone()));
        }
        let adapter = adapter::get_adapter(&provider, Protocol::OpenAI);
        let client = match gw.http_client_for_provider(provider.use_proxy).await {
            Ok(http_client) => ProxyClient::new(http_client),
            Err(e) => {
                last_error = Some(error_response(502, &format!("provider transport error: {e}")));
                continue;
            }
        };
        let call = client
            .call_non_stream(
                adapter.as_ref(),
                &provider.base_url,
                "/v1/embeddings",
                &provider.api_key,
                body.clone(),
                reqwest::header::HeaderMap::new(),
            )
            .await;
        match call {
            Ok((payload, status)) if status < 400 => {
                emit_log(
                    &gw,
                    "openai",
                    "openai",
                    &request_model,
                    &actual_model,
                    auth_key.id.as_deref(),
                    &provider.name,
                    status as i32,
                    start.elapsed().as_millis() as f64,
                    TokenUsage::default(),
                    false,
                    false,
                    None,
                    None,
                );
                return (
                    StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
                    Json(payload),
                )
                    .into_response();
            }
            Ok((payload, status)) => {
                last_error = Some((
                    StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
                    Json(payload),
                ).into_response());
            }
            Err(e) => {
                last_error = Some(error_response(502, &format!("upstream error: {e}")));
            }
        }
    }
    last_error.unwrap_or_else(|| error_response(502, "all route targets failed"))
}

// ── Anthropic ingress: POST /v1/messages ──

pub async fn anthropic_proxy(
    State(gw): State<Gateway>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    universal_proxy(gw, headers, body, Protocol::Anthropic).await
}

// ── Gemini ingress: POST /v1beta/models/:model_action ──

pub async fn gemini_proxy(
    State(gw): State<Gateway>,
    headers: HeaderMap,
    Path(model_action): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let (model, action) = match model_action.rsplit_once(':') {
        Some((m, a)) => (m.to_string(), a.to_string()),
        None => (model_action.clone(), "generateContent".to_string()),
    };
    let is_stream = action == "streamGenerateContent";

    let decoder = GeminiDecoder;
    let internal = match decoder.decode_with_model(body, &model, is_stream) {
        Ok(r) => r,
        Err(e) => return error_response(400, &format!("invalid Gemini request: {e}")),
    };

    proxy_pipeline(gw, headers, internal, Protocol::Gemini).await
}

// ── OpenAI models list ingress: GET /v1/models ──
pub async fn models_list(State(gw): State<Gateway>, headers: HeaderMap) -> Response {
    let mut accessible_route_ids = HashSet::new();

    if let Some(raw_key) = extract_api_key(&headers) {
        if let Some(store) = gw.storage.auth() {
            if let Ok(Some(key_row)) = store.find_api_key(&raw_key).await {
                let key_active = key_row.status == "active"
                    && key_row
                        .expires_at
                        .as_ref()
                        .map(|expires| !is_key_expired(expires))
                        .unwrap_or(true);

                if key_active {
                    if let Ok(bound_route_ids) = store.list_bound_route_ids(&key_row.id).await {
                        accessible_route_ids.extend(bound_route_ids);
                    }
                }
            }
        }
    }

    let cache = gw.route_cache.read().await;
    let models = cache
        .routes
        .iter()
        .filter(|route| !route.access_control || accessible_route_ids.contains(&route.id))
        .map(|route| route.virtual_model.trim())
        .filter(|model| !model.is_empty())
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>();

    let data = models
        .into_iter()
        .map(|model| {
            serde_json::json!({
                "id": model,
                "object": "model",
                "created": 0,
                "owned_by": "Nyro"
            })
        })
        .collect::<Vec<_>>();

    Json(serde_json::json!({
        "object": "list",
        "data": data
    }))
    .into_response()
}

// ── Universal proxy pipeline ──

async fn universal_proxy(gw: Gateway, headers: HeaderMap, body: Value, ingress: Protocol) -> Response {
    let decoder = crate::protocol::get_decoder(ingress);
    let internal = match decoder.decode_request(body) {
        Ok(r) => r,
        Err(e) => return error_response(400, &format!("invalid request: {e}")),
    };

    proxy_pipeline(gw, headers, internal, ingress).await
}

async fn proxy_pipeline(
    gw: Gateway,
    headers: HeaderMap,
    internal: InternalRequest,
    ingress: Protocol,
) -> Response {
    let start = Instant::now();
    let request_model = internal.model.clone();
    let is_stream = internal.stream;

    let ingress_str = ingress.to_string();
    let route = {
        let cache = gw.route_cache.read().await;
        cache.match_route(&request_model).cloned()
    };
    let route = match route {
        Some(r) => r,
        None => return error_response(404, &format!("no route for model: {request_model}")),
    };

    let access_store = GatewayProxyAccessStore::new(&gw);

    let auth_key = match authorize_route_access(&access_store, &route, &headers).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    let cache_control = parse_cache_control(&headers);
    let request_cacheable = is_cacheable_request(&internal);
    let cache_enabled_for_request = request_cache_enabled(&gw, &cache_control) && request_cacheable;
    let cache_key = if cache_enabled_for_request {
        Some(build_cache_key(
            gw.config.cache.namespace.as_deref(),
            &internal,
        ))
    } else {
        None
    };

    if let (Some(cache_backend), Some(key)) = (gw.cache_backend.as_ref(), cache_key.as_deref()) {
        if cache_control.allow_read {
            if let Ok(Some(bytes)) = cache_backend.get(key).await {
                if let Ok(cached_entry) = serde_json::from_slice::<CacheEntry>(&bytes) {
                    let response = cached_entry_to_response(
                        ingress,
                        &cached_entry,
                        is_stream,
                        Some(key),
                        cache_control.ttl_seconds,
                    );
                    emit_log(
                        &gw,
                        &ingress_str,
                        &ingress_str,
                        &request_model,
                        &request_model,
                        auth_key.id.as_deref(),
                        &cached_entry.provider_name,
                        cached_entry.status_code as i32,
                        start.elapsed().as_millis() as f64,
                        cached_entry.usage,
                        is_stream,
                        false,
                        None,
                        None,
                    );
                    return response;
                }
            }
        }
    }

    let mut singleflight_leader: Option<(String, broadcast::Sender<Vec<u8>>)> = None;
    if cache_enabled_for_request && cache_control.allow_write {
        if let Some(key) = cache_key.as_ref() {
            match gw.cache_in_flight.entry(key.clone()) {
                DashEntry::Occupied(entry) => {
                    let mut rx = entry.get().subscribe();
                    drop(entry);
                    if let Ok(Ok(bytes)) = timeout(Duration::from_secs(120), rx.recv()).await {
                        if !bytes.is_empty() {
                            if let Ok(cached_entry) = serde_json::from_slice::<CacheEntry>(&bytes) {
                                let response = cached_entry_to_response(
                                    ingress,
                                    &cached_entry,
                                    is_stream,
                                    Some(key),
                                    cache_control.ttl_seconds,
                                );
                                emit_log(
                                    &gw,
                                    &ingress_str,
                                    &ingress_str,
                                    &request_model,
                                    &request_model,
                                    auth_key.id.as_deref(),
                                    &cached_entry.provider_name,
                                    cached_entry.status_code as i32,
                                    start.elapsed().as_millis() as f64,
                                    cached_entry.usage,
                                    is_stream,
                                    false,
                                    None,
                                    None,
                                );
                                return response;
                            }
                        }
                    }
                }
                DashEntry::Vacant(entry) => {
                    let (tx, _) = broadcast::channel(16);
                    entry.insert(tx.clone());
                    singleflight_leader = Some((key.clone(), tx));
                }
            }
        }
    }

    let targets = load_route_targets(&gw, &route).await;
    if targets.is_empty() {
        return error_response(503, "no route targets configured");
    }
    let ordered_targets = TargetSelector::select_ordered(&route.strategy, &targets);
    if ordered_targets.is_empty() {
        return error_response(503, "no route targets configured");
    }

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
        let selected_model = if target.model.is_empty() || target.model == "*" {
            request_model.clone()
        } else {
            target.model.clone()
        };
        let actual_model = selected_model;

        let mut internal_for_target = internal.clone();
        crate::protocol::semantic::tool_correlation::normalize_request_tool_results(
            &mut internal_for_target,
        );

        let provider_protocols = ProviderProtocols::from_provider(&provider);
        let resolved = provider_protocols.resolve_egress(ingress);
        let egress = resolved.protocol;
        let egress_base_url = if resolved.base_url.is_empty() {
            provider.base_url.clone()
        } else {
            resolved.base_url
        };

        let adapter = adapter::get_adapter(&provider, egress);
        adapter
            .pre_request(&mut internal_for_target, &actual_model, &gw, &provider)
            .await;

        let encoder = crate::protocol::get_encoder(egress);
        let (egress_body, extra_headers) = match encoder.encode_request(&internal_for_target) {
            Ok(r) => r,
            Err(e) => {
                last_response = Some(error_response(500, &format!("encode error: {e}")));
                continue;
            }
        };
        
        let egress_body = override_model(egress_body, &actual_model, egress);
        let egress_path = encoder.egress_path(&actual_model, is_stream);
        let credential = match resolve_provider_credential(&gw, &provider).await {
            Ok(value) => value,
            Err(e) => {
                last_response = Some(error_response(502, &format!("provider credential error: {e}")));
                continue;
            }
        };
        let client = match gw.http_client_for_provider(provider.use_proxy).await {
            Ok(http_client) => ProxyClient::new(http_client),
            Err(e) => {
                let msg = format!("provider transport error: {e}");
                last_response = Some(error_response(502, &msg));
                continue;
            }
        };
        let mut forward_headers = adapter.auth_headers(&credential);
        forward_headers.extend(extra_headers.clone());
        let egress_str = egress.to_string();

        let response = if is_stream {
            handle_stream(
                gw.clone(),
                client,
                adapter.as_ref(),
                &provider,
                &egress_base_url,
                egress,
                ingress,
                &egress_path,
                &credential,
                egress_body,
                extra_headers,
                &ingress_str,
                &egress_str,
                &request_model,
                &actual_model,
                auth_key.id.as_deref(),
                start,
                cache_key.as_deref(),
                cache_enabled_for_request && cache_control.allow_write,
                cache_control.ttl_seconds,
                singleflight_leader.as_ref().map(|(k, _)| k.as_str()),
                singleflight_leader.as_ref().map(|(_, tx)| tx.clone()),
            )
            .await
        } else {
            handle_non_stream(
                gw.clone(),
                client,
                adapter.as_ref(),
                &provider,
                &egress_base_url,
                egress,
                ingress,
                &egress_path,
                &credential,
                egress_body,
                extra_headers,
                &ingress_str,
                &egress_str,
                &request_model,
                &actual_model,
                auth_key.id.as_deref(),
                start,
                cache_key.as_deref(),
                cache_enabled_for_request && cache_control.allow_write,
                cache_control.ttl_seconds,
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
    last_response.unwrap_or_else(|| error_response(502, "all route targets failed"))
}


#[allow(clippy::too_many_arguments)]
async fn handle_non_stream(
    gw: Gateway,
    client: ProxyClient,
    adapter: &dyn ProviderAdapter,
    provider: &Provider,
    egress_base_url: &str,
    egress: Protocol,
    ingress: Protocol,
    path: &str,
    credential: &str,
    body: Value,
    extra_headers: reqwest::header::HeaderMap,
    ingress_str: &str,
    egress_str: &str,
    request_model: &str,
    actual_model: &str,
    api_key_id: Option<&str>,
    start: Instant,
    cache_key: Option<&str>,
    allow_cache_store: bool,
    cache_ttl_seconds: Option<u64>,
) -> Response {
    let credential_to_use = credential.to_string();
    let call_result = match client
        .call_non_stream(
            adapter,
            egress_base_url,
            path,
            &credential_to_use,
            body.clone(),
            extra_headers.clone(),
        )
        .await
    {
        Ok(r) => r,
        Err(e) => {
            emit_log(
                &gw, ingress_str, egress_str, request_model, actual_model,
                api_key_id,
                &provider.name, 502, start.elapsed().as_millis() as f64,
                TokenUsage::default(), false, false,
                Some(e.to_string()), None,
            );
            return error_response(502, &format!("upstream error: {e}"));
        }
    };
    
    let (resp, status) = call_result;

    if status >= 400 {
        let preview = serde_json::to_string(&resp).ok().map(|s| s.chars().take(500).collect());
        emit_log(
            &gw, ingress_str, egress_str, request_model, actual_model,
            api_key_id,
            &provider.name, status as i32, start.elapsed().as_millis() as f64,
            TokenUsage::default(), false, false,
            preview.clone(), None,
        );
        return (
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(resp),
        )
            .into_response();
    }

    let parser = crate::protocol::get_response_parser(egress);
    let formatter = crate::protocol::get_response_formatter(ingress);

    let mut internal_resp = match parser.parse_response(resp) {
        Ok(r) => r,
        Err(e) => return error_response(500, &format!("parse error: {e}")),
    };
    crate::protocol::semantic::reasoning::normalize_response_reasoning(&mut internal_resp);
    crate::protocol::semantic::response_items::populate_response_items(&mut internal_resp);

    let is_tool = !internal_resp.tool_calls.is_empty();
    let usage = internal_resp.usage.clone();
    let output = formatter.format_response(&internal_resp);

    let response_preview = serde_json::to_string(&output)
        .ok()
        .map(|s| s.chars().take(500).collect());

    emit_log(
        &gw, ingress_str, egress_str, request_model, actual_model,
        api_key_id,
        &provider.name, status as i32, start.elapsed().as_millis() as f64,
        usage.clone(), false, is_tool, None, response_preview,
    );

    let mut response = (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(output.clone()),
    )
        .into_response();
    set_cache_headers(&mut response, false, cache_key, cache_ttl_seconds);

    if allow_cache_store && status < 400 && !is_tool {
        if let (Some(key), Some(cache_backend)) = (cache_key, gw.cache_backend.as_ref()) {
            let entry = CacheEntry {
                payload: output,
                status_code: status,
                provider_name: provider.name.clone(),
                usage,
                created_at_epoch_ms: chrono::Utc::now().timestamp_millis(),
                internal_response: Some(internal_resp),
            };
            if let Ok(bytes) = serde_json::to_vec(&entry) {
                let ttl = cache_ttl_seconds
                    .map(std::time::Duration::from_secs)
                    .or(Some(gw.config.cache.default_ttl));
                let _ = cache_backend.set(key, &bytes, ttl).await;
            }
        }
    }
    response
}

#[allow(clippy::too_many_arguments)]
async fn handle_stream(
    gw: Gateway,
    client: ProxyClient,
    adapter: &dyn ProviderAdapter,
    provider: &Provider,
    egress_base_url: &str,
    egress: Protocol,
    ingress: Protocol,
    path: &str,
    credential: &str,
    body: Value,
    extra_headers: reqwest::header::HeaderMap,
    ingress_str: &str,
    egress_str: &str,
    request_model: &str,
    actual_model: &str,
    api_key_id: Option<&str>,
    start: Instant,
    cache_key: Option<&str>,
    allow_cache_store: bool,
    cache_ttl_seconds: Option<u64>,
    singleflight_key: Option<&str>,
    singleflight_tx: Option<broadcast::Sender<Vec<u8>>>,
) -> Response {
    let credential_to_use = credential.to_string();
    let call_result = match client
        .call_stream(
            adapter,
            egress_base_url,
            path,
            &credential_to_use,
            body.clone(),
            extra_headers.clone(),
        )
        .await
    {
        Ok(r) => r,
        Err(e) => {
            emit_log(
                &gw, ingress_str, egress_str, request_model, actual_model,
                api_key_id,
                &provider.name, 502, start.elapsed().as_millis() as f64,
                TokenUsage::default(), true, false,
                Some(e.to_string()), None,
            );
            return error_response(502, &format!("upstream error: {e}"));
        }
    };
    
    let (resp, status) = call_result;

    if status >= 400 {
        let err_body: Value = resp
            .json()
            .await
            .unwrap_or_else(|_| serde_json::json!({"error": {"message": "upstream error"}}));
        emit_log(
            &gw, ingress_str, egress_str, request_model, actual_model,
            api_key_id,
            &provider.name, status as i32, start.elapsed().as_millis() as f64,
            TokenUsage::default(), true, false,
            Some(err_body.to_string()), None,
        );
        return (
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(err_body),
        )
            .into_response();
    }

    let mut stream_parser = crate::protocol::get_stream_parser(egress);
    let mut stream_formatter = crate::protocol::get_stream_formatter(ingress);

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
    let default_cache_ttl = gw.config.cache.default_ttl;

    tokio::spawn(async move {
        let mut accumulator = StreamResponseAccumulator::default();
        while let Some(chunk) = byte_stream.next().await {
            let bytes = match chunk {
                Ok(b) => b,
                Err(_) => break,
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

        emit_log(
            &gw_log, &ingress_s, &egress_s, &req_model, &act_model,
            key_id.as_deref(),
            &provider_name, 200, start.elapsed().as_millis() as f64,
            internal.usage.clone(), true, !internal.tool_calls.is_empty(), None, None,
        );

        let mut singleflight_payload: Option<Vec<u8>> = None;
        if allow_cache_store && internal.tool_calls.is_empty() {
            if let (Some(cache_backend), Some(cache_key)) = (gw_log.cache_backend.as_ref(), cache_key_owned.as_deref()) {
                let formatter = crate::protocol::get_response_formatter(ingress);
                let payload = formatter.format_response(&internal);
                let entry = CacheEntry {
                    payload,
                    status_code: 200,
                    provider_name: provider_name.clone(),
                    usage: internal.usage.clone(),
                    created_at_epoch_ms: chrono::Utc::now().timestamp_millis(),
                    internal_response: Some(internal.clone()),
                };
                if let Ok(bytes) = serde_json::to_vec(&entry) {
                    let ttl = cache_ttl_seconds
                        .map(Duration::from_secs)
                        .or(Some(default_cache_ttl));
                    let _ = cache_backend.set(cache_key, &bytes, ttl).await;
                    singleflight_payload = Some(bytes);
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
    set_cache_headers(&mut response, false, cache_key, cache_ttl_seconds);
    response
}

// ── Helpers ──

struct AuthenticatedKey {
    id: Option<String>,
}

#[async_trait]
trait ProxyAccessStore {
    async fn get_active_provider(&self, id: &str) -> anyhow::Result<Option<Provider>>;
    async fn find_api_key(&self, raw_key: &str) -> anyhow::Result<Option<ApiKeyAccessRecord>>;
    async fn route_binding_exists(&self, api_key_id: &str, route_id: &str) -> anyhow::Result<bool>;
    async fn request_count_since(&self, api_key_id: &str, window: UsageWindow) -> anyhow::Result<i64>;
    async fn token_count_since(&self, api_key_id: &str, window: UsageWindow) -> anyhow::Result<i64>;
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
        Ok(provider.filter(|p| p.is_active))
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

    async fn request_count_since(&self, api_key_id: &str, window: UsageWindow) -> anyhow::Result<i64> {
        match self.gw.storage.auth() {
            Some(store) => store.request_count_since(api_key_id, window).await,
            None => Ok(0),
        }
    }

    async fn token_count_since(&self, api_key_id: &str, window: UsageWindow) -> anyhow::Result<i64> {
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

    if key_row.status != "active" {
        return Err(error_response(403, "api key revoked"));
    }

    if let Some(expires) = key_row.expires_at.as_ref() {
        if is_key_expired(expires) {
            return Err(error_response(403, "api key expired"));
        }
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

fn is_key_expired(expires_at: &str) -> bool {
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(expires_at) {
        return parsed.with_timezone(&Utc) <= Utc::now();
    }

    NaiveDateTime::parse_from_str(expires_at, "%Y-%m-%d %H:%M:%S")
        .map(|parsed| parsed.and_utc() <= Utc::now())
        .unwrap_or(false)
}

fn extract_api_key(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(token) = value.strip_prefix("Bearer ") {
            let token = token.trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }

    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
}

async fn get_provider<S: ProxyAccessStore + ?Sized>(access_store: &S, id: &str) -> anyhow::Result<Provider> {
    access_store
        .get_active_provider(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("provider not found or inactive: {id}"))
}

fn override_model(mut body: Value, model: &str, protocol: Protocol) -> Value {
    match protocol {
        Protocol::Gemini => body,
        _ => {
            if let Some(obj) = body.as_object_mut() {
                obj.insert("model".into(), Value::String(model.to_string()));
            }
            body
        }
    }
}

fn error_type_for_status(status: u16) -> &'static str {
    match status {
        400 => "NYRO_BAD_REQUEST",
        401 => "NYRO_AUTH_ERROR",
        403 => "NYRO_FORBIDDEN",
        404 => "NYRO_NOT_FOUND",
        429 => "NYRO_RATE_LIMIT",
        500 => "NYRO_INTERNAL_ERROR",
        502 => "NYRO_UPSTREAM_ERROR",
        503 => "NYRO_SERVICE_UNAVAILABLE",
        _ => "NYRO_GATEWAY_ERROR",
    }
}

fn error_response(status: u16, message: &str) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        code,
        Json(serde_json::json!({
            "error": {
                "message": message,
                "type": error_type_for_status(status),
                "code": status
            }
        })),
    )
        .into_response()
}

async fn load_route_targets(gw: &Gateway, route: &Route) -> Vec<RouteTarget> {
    if let Some(store) = gw.storage.route_targets() {
        if let Ok(targets) = store.list_targets_by_route(&route.id).await {
            if !targets.is_empty() {
                return targets;
            }
        }
    }
    if route.target_provider.trim().is_empty() {
        return vec![];
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

fn is_retryable(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 529)
}

#[derive(Debug, Clone, Copy)]
struct RequestCacheControl {
    allow_read: bool,
    allow_write: bool,
    explicit_enable: Option<bool>,
    ttl_seconds: Option<u64>,
}

fn parse_cache_control(headers: &HeaderMap) -> RequestCacheControl {
    let mut allow_read = true;
    let mut allow_write = true;
    if let Some(value) = headers.get(header::CACHE_CONTROL).and_then(|v| v.to_str().ok()) {
        let normalized = value.to_ascii_lowercase();
        if normalized.contains("no-cache") {
            allow_read = false;
        }
        if normalized.contains("no-store") {
            allow_write = false;
        }
    }
    let explicit_enable = headers
        .get("x-nyro-cache")
        .and_then(|v| v.to_str().ok())
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"));
    if explicit_enable == Some(false) {
        allow_read = false;
        allow_write = false;
    }
    let ttl_seconds = headers
        .get("x-nyro-cache-ttl")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|v| *v > 0);
    RequestCacheControl {
        allow_read,
        allow_write,
        explicit_enable,
        ttl_seconds,
    }
}

fn request_cache_enabled(gw: &Gateway, control: &RequestCacheControl) -> bool {
    if !gw.config.cache.enabled || gw.cache_backend.is_none() {
        return false;
    }
    match gw.config.cache.mode {
        CacheMode::DefaultOn => control.explicit_enable != Some(false),
        CacheMode::DefaultOff => control.explicit_enable == Some(true),
    }
}

fn is_cacheable_request(request: &InternalRequest) -> bool {
    if request.temperature.unwrap_or(0.0) > 0.0 {
        return false;
    }
    for message in &request.messages {
        if let MessageContent::Blocks(blocks) = &message.content {
            if blocks
                .iter()
                .any(|block| matches!(block, ContentBlock::Image { .. }))
            {
                return false;
            }
        }
    }
    true
}

fn set_cache_headers(response: &mut Response, hit: bool, key: Option<&str>, ttl_seconds: Option<u64>) {
    let headers = response.headers_mut();
    headers.insert(
        "x-nyro-cache-hit",
        if hit {
            HeaderValue::from_static("true")
        } else {
            HeaderValue::from_static("false")
        },
    );
    if let Some(key) = key {
        if let Ok(value) = HeaderValue::from_str(key) {
            headers.insert("x-nyro-cache-key", value);
        }
    }
    if let Some(ttl) = ttl_seconds {
        if let Ok(value) = HeaderValue::from_str(&ttl.to_string()) {
            headers.insert("x-nyro-cache-ttl", value);
        }
    }
}

fn cached_entry_to_response(
    ingress: Protocol,
    entry: &CacheEntry,
    is_stream: bool,
    cache_key: Option<&str>,
    cache_ttl_seconds: Option<u64>,
) -> Response {
    if is_stream {
        if let Some(internal) = entry.internal_response.as_ref() {
            return replay_cached_stream(ingress, internal, cache_key, cache_ttl_seconds, true);
        }
    }
    let mut response = (
        StatusCode::from_u16(entry.status_code).unwrap_or(StatusCode::OK),
        Json(entry.payload.clone()),
    )
        .into_response();
    set_cache_headers(&mut response, true, cache_key, cache_ttl_seconds);
    response
}

fn replay_cached_stream(
    ingress: Protocol,
    internal: &InternalResponse,
    cache_key: Option<&str>,
    cache_ttl_seconds: Option<u64>,
    hit: bool,
) -> Response {
    let mut formatter = crate::protocol::get_stream_formatter(ingress);
    let deltas = internal_response_to_deltas(internal);
    let mut payloads: Vec<String> = formatter
        .format_deltas(&deltas)
        .into_iter()
        .map(|event| event.to_sse_string())
        .collect();
    payloads.extend(
        formatter
            .format_done()
            .into_iter()
            .map(|event| event.to_sse_string()),
    );

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, Infallible>>(payloads.len().max(1));
    tokio::spawn(async move {
        for payload in payloads {
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
    set_cache_headers(&mut response, hit, cache_key, cache_ttl_seconds);
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
    if let Some(reasoning) = &internal.reasoning_content {
        if !reasoning.is_empty() {
            deltas.push(StreamDelta::ReasoningDelta(reasoning.clone()));
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

async fn finalize_singleflight(
    gw: &Gateway,
    leader: Option<&(String, broadcast::Sender<Vec<u8>>)>,
    success: bool,
) {
    let Some((key, tx)) = leader else {
        return;
    };
    let payload = if success {
        if let Some(cache_backend) = gw.cache_backend.as_ref() {
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

#[derive(Default)]
struct StreamResponseAccumulator {
    id: String,
    model: String,
    content: String,
    reasoning_content: String,
    tool_calls: Vec<Option<ToolCall>>,
    stop_reason: Option<String>,
    usage: TokenUsage,
}

impl StreamResponseAccumulator {
    fn apply_all(&mut self, deltas: &[StreamDelta]) {
        for delta in deltas {
            self.apply(delta);
        }
    }

    fn apply(&mut self, delta: &StreamDelta) {
        match delta {
            StreamDelta::MessageStart { id, model } => {
                if self.id.is_empty() {
                    self.id = id.clone();
                }
                if self.model.is_empty() {
                    self.model = model.clone();
                }
            }
            StreamDelta::ReasoningDelta(text) => self.reasoning_content.push_str(text),
            StreamDelta::TextDelta(text) => self.content.push_str(text),
            StreamDelta::ToolCallStart { index, id, name } => {
                ensure_tool_index(&mut self.tool_calls, *index);
                self.tool_calls[*index] = Some(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: String::new(),
                });
            }
            StreamDelta::ToolCallDelta { index, arguments } => {
                ensure_tool_index(&mut self.tool_calls, *index);
                if let Some(tool_call) = self.tool_calls[*index].as_mut() {
                    tool_call.arguments.push_str(arguments);
                } else {
                    self.tool_calls[*index] = Some(ToolCall {
                        id: format!("tool-{index}"),
                        name: String::new(),
                        arguments: arguments.clone(),
                    });
                }
            }
            StreamDelta::Usage(usage) => self.usage = usage.clone(),
            StreamDelta::Done { stop_reason } => self.stop_reason = Some(stop_reason.clone()),
        }
    }

    fn into_internal_response(self) -> InternalResponse {
        let tool_calls = self
            .tool_calls
            .into_iter()
            .flatten()
            .filter(|tool| !tool.name.is_empty())
            .collect::<Vec<_>>();
        InternalResponse {
            id: self.id,
            model: self.model,
            content: self.content,
            reasoning_content: if self.reasoning_content.is_empty() {
                None
            } else {
                Some(self.reasoning_content)
            },
            tool_calls,
            response_items: None,
            stop_reason: self.stop_reason,
            usage: self.usage,
        }
    }
}

fn ensure_tool_index(tool_calls: &mut Vec<Option<ToolCall>>, index: usize) {
    if tool_calls.len() <= index {
        tool_calls.resize_with(index + 1, || None);
    }
}

async fn resolve_provider_credential(gw: &Gateway, provider: &Provider) -> anyhow::Result<String> {
    let _ = gw;
    Ok(provider.api_key.clone())
}

fn emit_log(
    gw: &Gateway,
    ingress: &str,
    egress: &str,
    request_model: &str,
    actual_model: &str,
    api_key_id: Option<&str>,
    provider_name: &str,
    status_code: i32,
    duration_ms: f64,
    usage: TokenUsage,
    is_stream: bool,
    is_tool_call: bool,
    error_message: Option<String>,
    response_preview: Option<String>,
) {
    let _ = gw.log_tx.try_send(LogEntry {
        api_key_id: api_key_id.map(ToString::to_string),
        ingress_protocol: ingress.to_string(),
        egress_protocol: egress.to_string(),
        request_model: request_model.to_string(),
        actual_model: actual_model.to_string(),
        provider_name: provider_name.to_string(),
        status_code,
        duration_ms,
        usage,
        is_stream,
        is_tool_call,
        error_message,
        response_preview,
    });
}
