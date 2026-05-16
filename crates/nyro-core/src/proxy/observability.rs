//! Unified observability sink for the proxy layer.
//!
//! All structured log writes and target-health updates MUST go through this
//! module.  No handler code should call `gw.log_tx.try_send` directly.

use crate::logging::LogEntry;

// ── Sensitive header redaction ─────────────────────────────────────────────────

/// Header names whose values are replaced with `"***"` before logging.
const REDACT_HEADER_KEYS: &[&str] = &[
    "authorization",
    "x-api-key",
    "x-goog-api-key",
    "openai-api-key",
    "anthropic-api-key",
    "cookie",
    "set-cookie",
    "proxy-authorization",
];

// ── Log extras ─────────────────────────────────────────────────────────────────

/// Optional HTTP-layer fields attached to every log entry. Used as an
/// intermediate carrier inside `LogBuilder`; maps 1-to-1 to `LogEntry` wire
/// fields.
#[derive(Default, Clone)]
pub struct LogExtras {
    pub method: Option<String>,
    pub path: Option<String>,

    pub client_request_headers: Option<String>,
    pub client_request_body: Option<String>,
    pub client_response_headers: Option<String>,
    pub client_response_body: Option<String>,

    pub upstream_request_headers: Option<String>,
    pub upstream_request_body: Option<String>,
    pub upstream_response_headers: Option<String>,
    pub upstream_response_body: Option<String>,

    pub upstream_url: Option<String>,
    pub upstream_status_code: Option<i32>,
    pub latency_upstream_ms: Option<i64>,

    pub stream_chunks_count: i32,
    pub stream_first_chunk_ms: Option<i64>,
}

// ── Direct log send ────────────────────────────────────────────────────────────

/// Enqueue a `LogEntry` directly. The canonical write path — no handler code
/// should call `gw.log_tx.try_send` outside of this function.
pub fn send_log(gw: &crate::Gateway, entry: LogEntry) {
    let _ = gw.log_tx.try_send(entry);
}

// ── headers_to_json ────────────────────────────────────────────────────────────

/// Serialize an axum `HeaderMap` to a flat JSON object string for logging.
/// Sensitive header values are replaced with `"***"`.
pub fn headers_to_json(headers: &axum::http::HeaderMap) -> Option<String> {
    let mut map = serde_json::Map::with_capacity(headers.len());
    for (name, value) in headers.iter() {
        let key = name.as_str().to_ascii_lowercase();
        let val = if REDACT_HEADER_KEYS.contains(&key.as_str()) {
            serde_json::Value::String("***".to_string())
        } else {
            value
                .to_str()
                .map(|s| serde_json::Value::String(s.to_string()))
                .unwrap_or_else(|_| {
                    serde_json::Value::String(format!("0x{}", hex_encode(value.as_bytes())))
                })
        };
        map.insert(key, val);
    }
    serde_json::to_string(&serde_json::Value::Object(map)).ok()
}

/// Serialize a reqwest `HeaderMap` to a flat JSON object string for logging.
/// Sensitive header values are replaced with `"***"`.
pub fn reqwest_headers_to_json(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let mut map = serde_json::Map::with_capacity(headers.len());
    for (name, value) in headers.iter() {
        let key = name.as_str().to_ascii_lowercase();
        let val = if REDACT_HEADER_KEYS.contains(&key.as_str()) {
            serde_json::Value::String("***".to_string())
        } else {
            value
                .to_str()
                .map(|s| serde_json::Value::String(s.to_string()))
                .unwrap_or_else(|_| {
                    serde_json::Value::String(format!("0x{}", hex_encode(value.as_bytes())))
                })
        };
        map.insert(key, val);
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
