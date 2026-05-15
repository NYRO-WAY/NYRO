//! Unified observability sink for the proxy layer.
//!
//! All structured log writes and target-health updates MUST go through this
//! module.  No handler code should call `gw.log_tx.try_send` directly.
//!
//! The `emit_log` function is a transitional API that mirrors the signature of
//! the old handler-level helper.  In P2-F it will be replaced with proper
//! OpenTelemetry spans emanating from `RequestContext::trace`.

use crate::Gateway;
use crate::logging::LogEntry;
use crate::protocol::ir::Usage;

// ── Log extras ────────────────────────────────────────────────────────────────

/// Optional HTTP-layer fields attached to every log entry.
#[derive(Default, Clone)]
pub struct LogExtras {
    pub method: Option<String>,
    pub path: Option<String>,
    pub request_headers: Option<String>,
    pub request_body: Option<String>,
    pub response_headers: Option<String>,
    pub response_body: Option<String>,
}

// ── emit_log ──────────────────────────────────────────────────────────────────

/// Enqueue a structured log entry.
///
/// This is a thin wrapper around `gw.log_tx.try_send`.  The intent is to be
/// the **single call site** for log writes — never call `log_tx` directly from
/// a handler.
#[allow(clippy::too_many_arguments)]
pub fn emit_log(
    gw: &Gateway,
    ingress: &str,
    egress: &str,
    request_model: &str,
    actual_model: &str,
    api_key_id: Option<&str>,
    provider_name: &str,
    status_code: i32,
    duration_ms: f64,
    usage: Usage,
    is_stream: bool,
    is_tool_call: bool,
    error_message: Option<String>,
    response_preview: Option<String>,
    extras: LogExtras,
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
        method: extras.method,
        path: extras.path,
        request_headers: extras.request_headers,
        request_body: extras.request_body,
        response_headers: extras.response_headers,
        response_body: extras.response_body,
    });
}

// ── headers_to_json ───────────────────────────────────────────────────────────

/// Serialize an axum `HeaderMap` to a flat JSON object string for logging.
pub fn headers_to_json(headers: &axum::http::HeaderMap) -> Option<String> {
    let mut map = serde_json::Map::with_capacity(headers.len());
    for (name, value) in headers.iter() {
        let val = value
            .to_str()
            .map(|s| serde_json::Value::String(s.to_string()))
            .unwrap_or_else(|_| {
                serde_json::Value::String(format!("0x{}", hex_encode(value.as_bytes())))
            });
        map.insert(name.as_str().to_ascii_lowercase(), val);
    }
    serde_json::to_string(&serde_json::Value::Object(map)).ok()
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
