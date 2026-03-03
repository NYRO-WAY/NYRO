use std::convert::Infallible;
use std::time::Instant;

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::StreamExt;
use serde_json::Value;
use tokio_stream::wrappers::ReceiverStream;

use crate::db::models::Provider;
use crate::logging::LogEntry;
use crate::protocol::openai::decoder::OpenAIDecoder;
use crate::protocol::openai::encoder::{self, OpenAIEncoder};
use crate::protocol::openai::stream::OpenAITranscoder;
use crate::protocol::types::TokenUsage;
use crate::protocol::{EgressEncoder, IngressDecoder, Protocol, ResponseTranscoder};
use crate::proxy::client::ProxyClient;
use crate::Gateway;

pub async fn openai_proxy(
    State(gw): State<Gateway>,
    Json(body): Json<Value>,
) -> Response {
    let start = Instant::now();

    let internal = match OpenAIDecoder.decode_request(body) {
        Ok(r) => r,
        Err(e) => return error_response(400, &format!("invalid request: {e}")),
    };

    let request_model = internal.model.clone();
    let is_stream = internal.stream;

    let route = {
        let cache = gw.route_cache.read().await;
        cache.match_route(&request_model).cloned()
    };

    let route = match route {
        Some(r) => r,
        None => return error_response(404, &format!("no route for model: {request_model}")),
    };

    let provider = match get_provider(&gw, &route.target_provider).await {
        Ok(p) => p,
        Err(e) => return error_response(502, &format!("provider error: {e}")),
    };

    let target_protocol: Protocol = provider
        .protocol
        .parse()
        .unwrap_or(Protocol::OpenAI);

    let (egress_body, _extra_headers) = match resolve_encoder(target_protocol)
        .encode_request(&internal)
    {
        Ok(r) => r,
        Err(e) => return error_response(500, &format!("encode error: {e}")),
    };

    let egress_body = override_model(egress_body, &route.target_model);
    let egress_path = resolve_egress_path(target_protocol);

    let client = ProxyClient::new(gw.http_client.clone());

    if is_stream {
        handle_stream(
            gw, client, &provider, target_protocol, egress_path, egress_body,
            request_model, route.target_model.clone(), start,
        )
        .await
    } else {
        handle_non_stream(
            gw, client, &provider, target_protocol, egress_path, egress_body,
            request_model, route.target_model.clone(), start,
        )
        .await
    }
}

async fn handle_non_stream(
    gw: Gateway,
    client: ProxyClient,
    provider: &Provider,
    target_protocol: Protocol,
    path: &str,
    body: Value,
    request_model: String,
    actual_model: String,
    start: Instant,
) -> Response {
    let (resp, status) = match client
        .call_non_stream(&provider.base_url, path, &provider.api_key, target_protocol, body)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            emit_log(
                &gw, "openai", &target_protocol.to_string(),
                &request_model, &actual_model, &provider.name,
                502, start.elapsed().as_millis() as f64,
                TokenUsage::default(), false, false,
                Some(e.to_string()), None, None,
            );
            return error_response(502, &format!("upstream error: {e}"));
        }
    };

    let transcoder = resolve_transcoder(Protocol::OpenAI);
    let (output, usage) = match transcoder.transcode_response(resp) {
        Ok(r) => r,
        Err(e) => return error_response(500, &format!("transcode error: {e}")),
    };

    let response_preview = serde_json::to_string(&output)
        .ok()
        .map(|s| s.chars().take(500).collect());

    emit_log(
        &gw, "openai", &target_protocol.to_string(),
        &request_model, &actual_model, &provider.name,
        status as i32, start.elapsed().as_millis() as f64,
        usage, false, false, None, None, response_preview,
    );

    (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(output)).into_response()
}

async fn handle_stream(
    gw: Gateway,
    client: ProxyClient,
    provider: &Provider,
    target_protocol: Protocol,
    path: &str,
    body: Value,
    request_model: String,
    actual_model: String,
    start: Instant,
) -> Response {
    let (resp, status) = match client
        .call_stream(&provider.base_url, path, &provider.api_key, target_protocol, body)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            emit_log(
                &gw, "openai", &target_protocol.to_string(),
                &request_model, &actual_model, &provider.name,
                502, start.elapsed().as_millis() as f64,
                TokenUsage::default(), true, false,
                Some(e.to_string()), None, None,
            );
            return error_response(502, &format!("upstream error: {e}"));
        }
    };

    if status >= 400 {
        let err_body: Value = resp.json().await.unwrap_or_else(|_| {
            serde_json::json!({"error": {"message": "upstream error"}})
        });
        emit_log(
            &gw, "openai", &target_protocol.to_string(),
            &request_model, &actual_model, &provider.name,
            status as i32, start.elapsed().as_millis() as f64,
            TokenUsage::default(), true, false,
            Some(err_body.to_string()), None, None,
        );
        return (StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY), Json(err_body))
            .into_response();
    }

    let transcoder = resolve_transcoder(Protocol::OpenAI);
    let mut stream_transcoder = transcoder.stream_transcoder();
    let mut byte_stream = resp.bytes_stream();

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, Infallible>>(64);

    let gw_log = gw.clone();
    let provider_name = provider.name.clone();
    let target_proto_str = target_protocol.to_string();

    tokio::spawn(async move {
        while let Some(chunk) = byte_stream.next().await {
            let bytes = match chunk {
                Ok(b) => b,
                Err(_) => break,
            };
            let text = String::from_utf8_lossy(&bytes);
            if let Ok(events) = stream_transcoder.process_chunk(&text) {
                for ev in events {
                    let sse = ev.to_sse_string();
                    if tx.send(Ok(sse)).await.is_err() {
                        return;
                    }
                }
            }
        }

        if let Ok(events) = stream_transcoder.finish() {
            for ev in events {
                let _ = tx.send(Ok(ev.to_sse_string())).await;
            }
        }

        let usage = stream_transcoder.usage();
        emit_log(
            &gw_log, "openai", &target_proto_str,
            &request_model, &actual_model, &provider_name,
            200, start.elapsed().as_millis() as f64,
            usage, true, false, None, None, None,
        );
    });

    let stream = ReceiverStream::new(rx);
    let body = Body::from_stream(stream);

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(body)
        .unwrap()
}

async fn get_provider(gw: &Gateway, id: &str) -> anyhow::Result<Provider> {
    let provider = sqlx::query_as::<_, Provider>(
        "SELECT id, name, protocol, base_url, api_key, is_active, priority, created_at, updated_at FROM providers WHERE id = ? AND is_active = 1",
    )
    .bind(id)
    .fetch_optional(&gw.db)
    .await?
    .ok_or_else(|| anyhow::anyhow!("provider not found or inactive: {id}"))?;
    Ok(provider)
}

fn resolve_encoder(protocol: Protocol) -> Box<dyn EgressEncoder> {
    match protocol {
        Protocol::OpenAI => Box::new(OpenAIEncoder),
        _ => Box::new(OpenAIEncoder),
    }
}

fn resolve_transcoder(source: Protocol) -> Box<dyn ResponseTranscoder> {
    match source {
        Protocol::OpenAI => Box::new(OpenAITranscoder),
        _ => Box::new(OpenAITranscoder),
    }
}

fn resolve_egress_path(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::OpenAI => encoder::egress_path(),
        _ => encoder::egress_path(),
    }
}

fn override_model(mut body: Value, model: &str) -> Value {
    if let Some(obj) = body.as_object_mut() {
        obj.insert("model".into(), Value::String(model.to_string()));
    }
    body
}

fn error_response(status: u16, message: &str) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        code,
        Json(serde_json::json!({
            "error": {
                "message": message,
                "type": "gateway_error",
                "code": status
            }
        })),
    )
        .into_response()
}

fn emit_log(
    gw: &Gateway,
    ingress: &str,
    egress: &str,
    request_model: &str,
    actual_model: &str,
    provider_name: &str,
    status_code: i32,
    duration_ms: f64,
    usage: TokenUsage,
    is_stream: bool,
    is_tool_call: bool,
    error_message: Option<String>,
    request_preview: Option<String>,
    response_preview: Option<String>,
) {
    let _ = gw.log_tx.try_send(LogEntry {
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
        request_preview,
        response_preview,
    });
}
