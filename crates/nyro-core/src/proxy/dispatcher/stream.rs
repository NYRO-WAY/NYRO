//! Streaming response handler.
//!
//! Two internal paths:
//! - PassThrough: ingress == egress protocol, no vendor mutations → forward raw
//!   SSE bytes; side-channel parser accumulates stats for logging.
//! - IR round-trip: parse → accumulate → format → re-emit as target-protocol SSE.

use std::convert::Infallible;

use axum::Json;
use axum::body::Body;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures::StreamExt;
use reqwest::header::HeaderMap as ReqwestHeaderMap;
use serde_json::Value;
use tokio::sync::broadcast;
use tokio_stream::wrappers::ReceiverStream;

use crate::cache::entry::CacheEntry;
use crate::protocol::ir::AiStreamDelta;
use crate::proxy::client::ProxyClient;
use crate::proxy::observability::headers_to_json;

use super::{
    CacheWriteCtx, CallCtx, LogBuilder, RequestExtras, StreamResponseAccumulator,
    compute_embedding, error_response, set_cache_headers,
};

// ── Streaming response handler ────────────────────────────────────────────────

pub(super) async fn handle_stream(
    client: ProxyClient,
    url: &str,
    headers: ReqwestHeaderMap,
    body: Value,
    call_ctx: &CallCtx<'_>,
    cache_ctx: &CacheWriteCtx<'_>,
    req_extras: &RequestExtras,
    singleflight_key: Option<&str>,
    singleflight_tx: Option<broadcast::Sender<Vec<u8>>>,
    passthrough_resp: bool,
) -> Response {
    let egress = call_ctx.egress;
    let ingress = call_ctx.ingress;
    let cache_key = cache_ctx.cache_key;
    let allow_exact_store = cache_ctx.allow_exact_store;
    let exact_cache_ttl = cache_ctx.exact_cache_ttl;
    let semantic_write_ctx = cache_ctx.semantic.clone();
    let expose_headers = cache_ctx.expose_headers;
    // Shared log builder: identity + request-side extras pre-filled.
    let log = LogBuilder::from_ctx(call_ctx).with_req_extras(req_extras);

    let upstream_start = std::time::Instant::now();
    let call_result = match client.call_stream(url, headers.clone(), body.clone()).await {
        Ok(r) => r,
        Err(e) => {
            log.status(502)
                .resp_body(Some(
                    serde_json::json!({ "error": { "message": format!("upstream error: {e}") } })
                        .to_string(),
                ))
                .emit();
            return error_response(502, &format!("upstream error: {e}"));
        }
    };
    let upstream_req_hdrs_str = crate::proxy::observability::reqwest_headers_to_json(&headers);
    let upstream_req_body_str = serde_json::to_string(&body).ok();

    let (resp, status) = call_result;
    let upstream_hdrs_str = headers_to_json(resp.headers());

    if status >= 400 {
        let err_body: Value = resp
            .json()
            .await
            .unwrap_or_else(|_| serde_json::json!({"error": {"message": "upstream error"}}));
        let err_body_str = serde_json::to_string(&err_body).ok();
        log.status(status)
            .upstream_status(status as i32)
            .with_upstream_request(upstream_req_hdrs_str, upstream_req_body_str)
            .upstream_resp_headers(upstream_hdrs_str.clone())
            .upstream_resp_body(err_body_str.clone())
            .resp_body(err_body_str)
            .emit();
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

        // Clone the log builder into the spawn: all identity + request-side
        // fields are already owned inside the builder, so no individual variable
        // cloning is needed.
        let log_pt = log.clone();
        // gw is needed after emit() consumes log_pt; clone it up-front.
        let gw_pt = log_pt.gw.clone();
        let leader_key_pt = singleflight_key.map(ToString::to_string);
        let leader_tx_pt = singleflight_tx.clone();
        let upstream_hdrs_pt = upstream_hdrs_str.clone();
        let upstream_req_hdrs_pt = upstream_req_hdrs_str.clone();
        let upstream_req_body_pt = upstream_req_body_str.clone();
        let upstream_start_pt = upstream_start;

        tokio::spawn(async move {
            let mut log_buf: Vec<u8> = Vec::new();
            let mut byte_stream = resp.bytes_stream();
            let mut stream_error: Option<String> = None;
            let mut chunks_count: i32 = 0;
            let mut first_chunk_ms: Option<i64> = None;

            while let Some(result) = byte_stream.next().await {
                match result {
                    Ok(b) => {
                        if first_chunk_ms.is_none() {
                            first_chunk_ms = Some(upstream_start_pt.elapsed().as_millis() as i64);
                        }
                        chunks_count += 1;
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

            let upstream_latency_ms = upstream_start_pt.elapsed().as_millis() as i64;
            let raw_sse = String::from_utf8_lossy(&log_buf).into_owned();

            // Parse accumulated buffer for usage stats (best-effort).
            let mut log_parser = egress.handler().make_stream_response_decoder();
            let mut accumulator = StreamResponseAccumulator::default();
            if let Ok(ai_deltas) = log_parser.parse_chunk(&raw_sse) {
                accumulator.apply_all(&ai_deltas);
            }
            if let Ok(ai_deltas) = log_parser.finish() {
                accumulator.apply_all(&ai_deltas);
            }

            let mut ai_resp = accumulator.into_ai_response();
            if ai_resp.id.is_empty() {
                ai_resp.id = format!("msg_{}", uuid::Uuid::new_v4().simple());
            }
            if ai_resp.model.is_empty() {
                ai_resp.model = log_pt.upstream_model.clone();
            }

            log_pt
                .status(200)
                .upstream_status(200)
                .usage(ai_resp.usage.clone())
                .maybe_error(stream_error)
                .with_upstream_request(upstream_req_hdrs_pt, upstream_req_body_pt)
                .with_upstream_response(
                    200,
                    upstream_hdrs_pt,
                    Some(raw_sse.clone()),
                    Some(upstream_latency_ms),
                )
                .with_client_response(None, Some(raw_sse))
                .stream_metrics(chunks_count, first_chunk_ms)
                .emit();

            // log_pt is consumed by emit(); use the pre-cloned gw_pt.
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
    let mut stream_parser = egress.handler().make_stream_response_decoder();
    let mut stream_formatter = ingress.handler().make_stream_response_encoder();
    let mut byte_stream = resp.bytes_stream();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, Infallible>>(64);

    // Move the log builder into the spawn.  Extract the fields we need AFTER
    // emit() consumes the builder, before passing it to the spawn.
    let log_ir = log;
    let gw_ir = log_ir.gw.clone(); // needed for cache writes after emit()
    let provider_name_ir = log_ir.provider_id.clone();
    let act_model_ir = log_ir.upstream_model.clone();
    let cache_key_owned = cache_key.map(ToString::to_string);
    let leader_key_owned = singleflight_key.map(ToString::to_string);
    let leader_tx_owned = singleflight_tx.clone();
    let exact_cache_ttl_owned = exact_cache_ttl;
    let semantic_write_ctx_owned = semantic_write_ctx.clone();
    let upstream_hdrs_owned = upstream_hdrs_str;

    tokio::spawn(async move {
        let mut accumulator = StreamResponseAccumulator::default();
        let mut upstream_raw_buf: Vec<u8> = Vec::new();
        let mut client_sse_parts: Vec<String> = Vec::new();
        let mut chunks_count: i32 = 0;
        let mut first_chunk_ms: Option<i64> = None;

        while let Some(chunk) = byte_stream.next().await {
            let bytes = match chunk {
                Ok(b) => b,
                Err(e) => {
                    // P1: emit an explicit terminal event instead of silently breaking,
                    // so the client receives a defined stop_reason and does not hang.
                    tracing::warn!(error = %e, "upstream stream error; emitting terminal event");
                    let error_deltas = [AiStreamDelta::Done {
                        stop_reason: "error".to_string(),
                    }];
                    let events = stream_formatter.format_deltas(&error_deltas);
                    for ev in events {
                        let _ = tx.send(Ok(ev.to_sse_string())).await;
                    }
                    break;
                }
            };
            if first_chunk_ms.is_none() {
                first_chunk_ms = Some(upstream_start.elapsed().as_millis() as i64);
            }
            chunks_count += 1;
            upstream_raw_buf.extend_from_slice(&bytes);
            let text = String::from_utf8_lossy(&bytes);
            if let Ok(ai_deltas) = stream_parser.parse_chunk(&text) {
                accumulator.apply_all(&ai_deltas);
                let events = stream_formatter.format_deltas(&ai_deltas);
                for ev in events {
                    let sse = ev.to_sse_string();
                    client_sse_parts.push(sse.clone());
                    if tx.send(Ok(sse)).await.is_err() {
                        return;
                    }
                }
            }
        }

        if let Ok(ai_deltas) = stream_parser.finish() {
            accumulator.apply_all(&ai_deltas);
            let events = stream_formatter.format_deltas(&ai_deltas);
            for ev in events {
                let sse = ev.to_sse_string();
                client_sse_parts.push(sse.clone());
                let _ = tx.send(Ok(sse)).await;
            }
        }

        let done_events = stream_formatter.format_done();
        for ev in done_events {
            let sse = ev.to_sse_string();
            client_sse_parts.push(sse.clone());
            let _ = tx.send(Ok(sse)).await;
        }

        let upstream_latency_ms = upstream_start.elapsed().as_millis() as i64;
        let upstream_raw_str = String::from_utf8_lossy(&upstream_raw_buf).into_owned();
        let client_sse_str = client_sse_parts.join("");

        let usage = stream_formatter.usage();
        let mut ai_resp = accumulator.into_ai_response();
        if ai_resp.usage.prompt_tokens == 0 && ai_resp.usage.completion_tokens == 0 {
            ai_resp.usage = usage.clone();
        }
        if ai_resp.id.is_empty() {
            ai_resp.id = format!("chatcmpl-{}", uuid::Uuid::new_v4().simple());
        }
        if ai_resp.model.is_empty() {
            ai_resp.model = act_model_ir.clone();
        }
        if ai_resp.stop_reason.is_none() {
            ai_resp.stop_reason = Some("stop".to_string());
        }

        log_ir
            .status(200)
            .upstream_status(200)
            .usage(ai_resp.usage.clone())
            .with_upstream_request(upstream_req_hdrs_str, upstream_req_body_str)
            .with_upstream_response(
                200,
                upstream_hdrs_owned,
                Some(upstream_raw_str),
                Some(upstream_latency_ms),
            )
            .with_client_response(None, Some(client_sse_str))
            .stream_metrics(chunks_count, first_chunk_ms)
            .emit();

        let mut singleflight_payload: Option<Vec<u8>> = None;
        if allow_exact_store && ai_resp.tool_calls.is_empty() {
            let cache_backend = (**gw_ir.cache_backend.load()).clone();
            if let (Some(cache_backend), Some(cache_key)) =
                (cache_backend.as_ref(), cache_key_owned.as_deref())
            {
                let formatter = ingress.handler().make_response_encoder();
                let payload = formatter.format_response(&ai_resp);
                let entry = CacheEntry {
                    payload,
                    status_code: 200,
                    provider_name: provider_name_ir.clone(),
                    actual_model: Some(act_model_ir.clone()),
                    usage: ai_resp.usage.clone(),
                    created_at_epoch_ms: chrono::Utc::now().timestamp_millis(),
                    internal_response: Some(ai_resp.clone()),
                };
                if let Ok(bytes) = serde_json::to_vec(&entry) {
                    cache_backend
                        .set(cache_key, &bytes, exact_cache_ttl_owned)
                        .await
                        .ok();
                    singleflight_payload = Some(bytes.clone());
                    let vector_store = (**gw_ir.vector_store.load()).clone();
                    if let (Some(vector_store), Some(ctx)) =
                        (vector_store.as_ref(), semantic_write_ctx_owned.as_ref())
                    {
                        let vector = if let Some(existing) = ctx.query_vector.clone() {
                            Some(existing)
                        } else {
                            compute_embedding(&gw_ir, &ctx.embedding_text).await.ok()
                        };
                        if let Some(vector) = vector {
                            vector_store
                                .upsert(&ctx.partition, ctx.key.clone(), vector, bytes)
                                .await
                                .ok();
                        }
                    }
                }
            }
        } else if ai_resp.tool_calls.is_empty() {
            let vector_store = (**gw_ir.vector_store.load()).clone();
            if let (Some(vector_store), Some(ctx)) =
                (vector_store.as_ref(), semantic_write_ctx_owned.as_ref())
            {
                let formatter = ingress.handler().make_response_encoder();
                let payload = formatter.format_response(&ai_resp);
                let entry = CacheEntry {
                    payload,
                    status_code: 200,
                    provider_name: provider_name_ir.clone(),
                    actual_model: Some(act_model_ir.clone()),
                    usage: ai_resp.usage.clone(),
                    created_at_epoch_ms: chrono::Utc::now().timestamp_millis(),
                    internal_response: Some(ai_resp.clone()),
                };
                if let Ok(bytes) = serde_json::to_vec(&entry) {
                    let vector = if let Some(existing) = ctx.query_vector.clone() {
                        Some(existing)
                    } else {
                        compute_embedding(&gw_ir, &ctx.embedding_text).await.ok()
                    };
                    if let Some(vector) = vector {
                        vector_store
                            .upsert(&ctx.partition, ctx.key.clone(), vector, bytes)
                            .await
                            .ok();
                    }
                }
            }
        }

        if let (Some(key), Some(tx)) = (leader_key_owned.as_deref(), leader_tx_owned.as_ref()) {
            let _ = tx.send(singleflight_payload.unwrap_or_default());
            gw_ir.cache_in_flight.remove(key);
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
