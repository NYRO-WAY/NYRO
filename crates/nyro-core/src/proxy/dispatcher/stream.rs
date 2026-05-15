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
use crate::protocol::ir::compat::ai_stream_delta_to_old;
use crate::protocol::ir::{AiResponse, AiStreamDelta};
use crate::protocol::types::StreamDelta;
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
    // Shared log builder: identity + request-side extras pre-filled, stream=true.
    let log = LogBuilder::from_ctx(call_ctx)
        .stream()
        .with_req_extras(req_extras);

    let call_result = match client.call_stream(url, headers, body.clone()).await {
        Ok(r) => r,
        Err(e) => {
            log.status(502)
                .error(e.to_string())
                .resp_body(Some(
                    serde_json::json!({ "error": { "message": format!("upstream error: {e}") } })
                        .to_string(),
                ))
                .emit();
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
        log.status(status)
            .error(err_body.to_string())
            .resp_headers(upstream_hdrs_str.clone())
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
            if let Ok(ai_deltas) = log_parser.parse_chunk(&log_text) {
                let old: Vec<StreamDelta> = ai_deltas.iter().map(ai_stream_delta_to_old).collect();
                accumulator.apply_all(&old);
            }
            if let Ok(ai_deltas) = log_parser.finish() {
                let old: Vec<StreamDelta> = ai_deltas.iter().map(ai_stream_delta_to_old).collect();
                accumulator.apply_all(&old);
            }

            let mut internal = accumulator.into_internal_response();
            if internal.id.is_empty() {
                internal.id = format!("msg_{}", uuid::Uuid::new_v4().simple());
            }
            if internal.model.is_empty() {
                // actual_model is embedded inside log_pt
                internal.model = log_pt.actual_model.clone();
            }

            let aggregated_formatter = ingress.handler().make_response_formatter();
            let aggregated_output =
                aggregated_formatter.format_response(&AiResponse::from(internal.clone()));
            let aggregated_body_str = serde_json::to_string(&aggregated_output).ok();

            log_pt
                .status(200)
                .usage(internal.usage.clone())
                .maybe_tool(!internal.tool_calls.is_empty())
                .maybe_error(stream_error)
                .resp_headers(upstream_hdrs_pt)
                .resp_body(aggregated_body_str)
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
    let mut stream_parser = egress.handler().make_stream_parser();
    let mut stream_formatter = ingress.handler().make_stream_formatter();
    let mut byte_stream = resp.bytes_stream();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, Infallible>>(64);

    // Move the log builder into the spawn.  Extract the fields we need AFTER
    // emit() consumes the builder, before passing it to the spawn.
    let log_ir = log;
    let gw_ir = log_ir.gw.clone(); // needed for cache writes after emit()
    let provider_name_ir = log_ir.provider_name.clone();
    let act_model_ir = log_ir.actual_model.clone();
    let cache_key_owned = cache_key.map(ToString::to_string);
    let leader_key_owned = singleflight_key.map(ToString::to_string);
    let leader_tx_owned = singleflight_tx.clone();
    let exact_cache_ttl_owned = exact_cache_ttl;
    let semantic_write_ctx_owned = semantic_write_ctx.clone();
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
            let text = String::from_utf8_lossy(&bytes);
            if let Ok(ai_deltas) = stream_parser.parse_chunk(&text) {
                let old_deltas: Vec<StreamDelta> =
                    ai_deltas.iter().map(ai_stream_delta_to_old).collect();
                accumulator.apply_all(&old_deltas);
                let events = stream_formatter.format_deltas(&ai_deltas);
                for ev in events {
                    if tx.send(Ok(ev.to_sse_string())).await.is_err() {
                        return;
                    }
                }
            }
        }

        if let Ok(ai_deltas) = stream_parser.finish() {
            let old_deltas: Vec<StreamDelta> =
                ai_deltas.iter().map(ai_stream_delta_to_old).collect();
            accumulator.apply_all(&old_deltas);
            let events = stream_formatter.format_deltas(&ai_deltas);
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
            internal.model = act_model_ir.clone();
        }
        if internal.stop_reason.is_none() {
            internal.stop_reason = Some("stop".to_string());
        }

        let aggregated_formatter = ingress.handler().make_response_formatter();
        let aggregated_output =
            aggregated_formatter.format_response(&AiResponse::from(internal.clone()));
        let aggregated_body_str = serde_json::to_string(&aggregated_output).ok();
        log_ir
            .status(200)
            .usage(internal.usage.clone())
            .maybe_tool(!internal.tool_calls.is_empty())
            .resp_headers(upstream_hdrs_owned)
            .resp_body(aggregated_body_str)
            .emit();

        let mut singleflight_payload: Option<Vec<u8>> = None;
        if allow_exact_store && internal.tool_calls.is_empty() {
            let cache_backend = (**gw_ir.cache_backend.load()).clone();
            if let (Some(cache_backend), Some(cache_key)) =
                (cache_backend.as_ref(), cache_key_owned.as_deref())
            {
                let formatter = ingress.handler().make_response_formatter();
                let payload = formatter.format_response(&AiResponse::from(internal.clone()));
                let entry = CacheEntry {
                    payload,
                    status_code: 200,
                    provider_name: provider_name_ir.clone(),
                    actual_model: Some(act_model_ir.clone()),
                    usage: internal.usage.clone(),
                    created_at_epoch_ms: chrono::Utc::now().timestamp_millis(),
                    internal_response: Some(internal.clone()),
                };
                if let Ok(bytes) = serde_json::to_vec(&entry) {
                    let _ = cache_backend
                        .set(cache_key, &bytes, exact_cache_ttl_owned)
                        .await;
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
                            let _ = vector_store
                                .upsert(&ctx.partition, ctx.key.clone(), vector, bytes)
                                .await;
                        }
                    }
                }
            }
        } else if internal.tool_calls.is_empty() {
            let vector_store = (**gw_ir.vector_store.load()).clone();
            if let (Some(vector_store), Some(ctx)) =
                (vector_store.as_ref(), semantic_write_ctx_owned.as_ref())
            {
                let formatter = ingress.handler().make_response_formatter();
                let payload = formatter.format_response(&AiResponse::from(internal.clone()));
                let entry = CacheEntry {
                    payload,
                    status_code: 200,
                    provider_name: provider_name_ir.clone(),
                    actual_model: Some(act_model_ir.clone()),
                    usage: internal.usage.clone(),
                    created_at_epoch_ms: chrono::Utc::now().timestamp_millis(),
                    internal_response: Some(internal.clone()),
                };
                if let Ok(bytes) = serde_json::to_vec(&entry) {
                    let vector = if let Some(existing) = ctx.query_vector.clone() {
                        Some(existing)
                    } else {
                        compute_embedding(&gw_ir, &ctx.embedding_text).await.ok()
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
