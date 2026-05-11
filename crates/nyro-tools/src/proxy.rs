//! `nyro-tools proxy` — transparent passthrough proxy for local debugging.
//!
//! Forwards requests matching known LLM ingress paths to the configured upstream.
//! Unknown paths are logged and answered with 200 OK. All request and response
//! bodies (including SSE streams) are emitted as structured JSON log lines,
//! one line per request and one per response, correlated by a shared `id`.

use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
    Router,
};
use bytes::Bytes;
use clap::Args;
use futures::{SinkExt, StreamExt};
use std::io::{self, BufWriter, Write};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tracing::warn;
use uuid::Uuid;

const BOX_SEP: &str = "─────────────────────────────────────────────────────";

// ── Logger ─────────────────────────────────────────────────────────────────

struct ProxyLogger {
    writer: Mutex<Box<dyn Write + Send>>,
}

impl ProxyLogger {
    fn stdout() -> Self {
        Self {
            writer: Mutex::new(Box::new(io::stdout())),
        }
    }

    fn file(path: &std::path::Path) -> io::Result<Self> {
        let f = std::fs::File::create(path)?;
        Ok(Self {
            writer: Mutex::new(Box::new(BufWriter::new(f))),
        })
    }

    fn emit(&self, value: serde_json::Value) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = writeln!(w, "{value}");
            let _ = w.flush();
        }
    }
}

// ── Log mode ───────────────────────────────────────────────────────────────

/// Controls which events are written to the log output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum LogMode {
    /// Log both requests and responses (default)
    #[value(name = "all")]
    All,
    /// Log requests (and skipped paths) only
    #[value(name = "req")]
    Request,
    /// Log responses only
    #[value(name = "resp")]
    Response,
}

impl LogMode {
    fn log_request(self) -> bool {
        matches!(self, LogMode::All | LogMode::Request)
    }
    fn log_response(self) -> bool {
        matches!(self, LogMode::All | LogMode::Response)
    }
}

// ── CLI args ───────────────────────────────────────────────────────────────

#[derive(Debug, Args)]
pub struct ProxyArgs {
    /// Listen port
    #[arg(short = 'P', long, default_value_t = 25208)]
    pub port: u16,

    /// Listen host
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    pub host: String,

    /// Upstream base URL (e.g. https://api.openai.com/v1)
    #[arg(short = 'u', long)]
    pub url: url::Url,

    /// Log output file path; defaults to stdout when omitted
    #[arg(short = 'o', long)]
    pub output: Option<std::path::PathBuf>,

    /// What to log: all (default), req (requests + skipped), resp (responses)
    #[arg(short = 'l', long, default_value = "all")]
    pub log_mode: LogMode,
}

// ── State ──────────────────────────────────────────────────────────────────

struct ProxyState {
    upstream: url::Url,
    client: reqwest::Client,
    logger: Arc<ProxyLogger>,
    log_mode: LogMode,
}

// ── Entry point ────────────────────────────────────────────────────────────

pub async fn run(args: ProxyArgs) -> Result<()> {
    let logger = match &args.output {
        Some(path) => ProxyLogger::file(path)
            .with_context(|| format!("failed to open log file `{}`", path.display()))?,
        None => ProxyLogger::stdout(),
    };

    let state = Arc::new(ProxyState {
        upstream: args.url.clone(),
        client: reqwest::Client::builder()
            .build()
            .context("failed to build reqwest client")?,
        logger: Arc::new(logger),
        log_mode: args.log_mode,
    });

    let host: std::net::IpAddr = args
        .host
        .parse()
        .with_context(|| format!("invalid host `{}`", args.host))?;
    let addr = SocketAddr::from((host, args.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;

    let log_dest = args
        .output
        .as_deref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "stdout".to_string());

    let app = Router::new().fallback(forward).with_state(state);

    // Startup summary goes to tracing (always visible in terminal)
    let log_mode_str = match args.log_mode {
        LogMode::All => "all",
        LogMode::Request => "req",
        LogMode::Response => "resp",
    };
    tracing::info!(
        "\n┌─ nyro-tools proxy ──────────────────────────────────\n\
         │ ingress   http://{addr}\n\
         │ egress    {url}\n\
         │ routes    /v1/chat/completions\n\
         │           /v1/responses\n\
         │           /v1/messages\n\
         │           /v1beta/models/*:*\n\
         │ log       {log_dest}  [{log_mode_str}]\n\
         └{BOX_SEP}",
        url = args.url,
    );

    axum::serve(listener, app).await?;
    Ok(())
}

// ── Handlers ───────────────────────────────────────────────────────────────

async fn forward(State(state): State<Arc<ProxyState>>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    let path = parts.uri.path().to_string();

    let bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(b) => b,
        Err(e) => {
            warn!(error = %e, "failed to buffer request body");
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };

    let id = Uuid::new_v4().to_string();

    if !is_known_ingress(&path) {
        if state.log_mode.log_request() {
            state.logger.emit(serde_json::json!({
                "id": id,
                "type": "skipped",
                "method": parts.method.as_str(),
                "path": path,
                "headers": headers_to_json(&parts.headers),
                "body": body_to_json(&bytes),
            }));
        }
        return (
            StatusCode::OK,
            axum::Json(serde_json::json!({"status": "received", "path": path})),
        )
            .into_response();
    }

    let target_url = build_target_url(&state.upstream, &parts.uri)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| "?".to_string());

    if state.log_mode.log_request() {
        state.logger.emit(serde_json::json!({
            "id": id,
            "type": "request",
            "method": parts.method.as_str(),
            "path": path,
            "target": target_url,
            "headers": headers_to_json(&parts.headers),
            "body": body_to_json(&bytes),
        }));
    }

    match forward_inner(state, parts, bytes, id).await {
        Ok(resp) => resp,
        Err(e) => {
            warn!(error = %e, "proxy forward failed");
            (
                StatusCode::BAD_GATEWAY,
                axum::Json(serde_json::json!({"error": {"message": e.to_string()}})),
            )
                .into_response()
        }
    }
}

async fn forward_inner(
    state: Arc<ProxyState>,
    parts: axum::http::request::Parts,
    bytes: Bytes,
    id: String,
) -> Result<Response> {
    let target = build_target_url(&state.upstream, &parts.uri)?;
    let method = parts.method.clone();
    let upstream_method = reqwest::Method::from_bytes(method.as_str().as_bytes())?;

    let mut request = state.client.request(upstream_method, target.clone());
    request = request.headers(forward_request_headers(&parts.headers));
    if !bytes.is_empty() {
        request = request.body(bytes.to_vec());
    }

    let upstream = request
        .send()
        .await
        .with_context(|| format!("upstream request to {} {} failed", method, target))?;

    let status = StatusCode::from_u16(upstream.status().as_u16())?;
    let is_sse = upstream
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.contains("text/event-stream"))
        .unwrap_or(false);

    let mut response_headers = HeaderMap::new();
    let mut resp_headers_json = serde_json::Map::new();
    for (name, value) in upstream.headers().iter() {
        if is_hop_by_hop(name.as_str()) {
            continue;
        }
        resp_headers_json.insert(
            name.to_string(),
            serde_json::Value::String(value.to_str().unwrap_or("?").to_string()),
        );
        if let (Ok(n), Ok(v)) = (
            HeaderName::from_bytes(name.as_str().as_bytes()),
            HeaderValue::from_bytes(value.as_bytes()),
        ) {
            response_headers.insert(n, v);
        }
    }

    // Tee the upstream stream: forward each chunk to the client in real-time
    // while accumulating a copy for the response log emitted after stream ends.
    let logger = Arc::clone(&state.logger);
    let log_mode = state.log_mode;
    let status_u16 = status.as_u16();
    let req_path = parts.uri.path().to_string();
    let (mut tx, rx) = futures::channel::mpsc::channel::<Result<Bytes, io::Error>>(128);
    tokio::spawn(async move {
        let mut log_buf: Vec<u8> = Vec::new();
        let mut stream = Box::pin(upstream.bytes_stream());
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(b) => {
                    log_buf.extend_from_slice(&b);
                    if tx.send(Ok(b)).await.is_err() {
                        break; // client disconnected
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(io::Error::other(e.to_string()))).await;
                    break;
                }
            }
        }
        if log_mode.log_response() {
            let data = if is_sse {
                assemble_sse(&req_path, &log_buf)
            } else {
                body_to_json(&log_buf)
            };
            logger.emit(serde_json::json!({
                "id": id,
                "type": "response",
                "status": status_u16,
                "headers": serde_json::Value::Object(resp_headers_json),
                "data": data,
            }));
        }
    });

    let body = Body::from_stream(rx);
    let mut response = Response::new(body);
    *response.status_mut() = status;
    *response.headers_mut() = response_headers;
    Ok(response)
}

// ── URL building ───────────────────────────────────────────────────────────

/// Build the upstream target URL from the configured egress base and the
/// incoming request URI.
///
/// If the egress base ends with a version segment (`v1`, `v1beta`, `v4`, …),
/// strip the leading version segment from the incoming path and append only
/// the remainder. Otherwise append the full incoming path as-is.
fn build_target_url(upstream: &url::Url, incoming: &Uri) -> Result<url::Url> {
    let suffix = if upstream_last_segment_is_version(upstream) {
        strip_version_prefix(incoming.path())
            .unwrap_or(incoming.path())
            .to_string()
    } else {
        incoming.path().to_string()
    };

    let mut url = upstream.clone();
    let base_path = url.path().trim_end_matches('/').to_owned();
    url.set_path(&format!("{base_path}{suffix}"));

    if let Some(q) = incoming.query() {
        url.set_query(Some(q));
    } else {
        url.set_query(None);
    }
    Ok(url)
}

/// Returns `true` if the last path segment of `upstream` looks like a version
/// token: `v` followed by at least one digit, optionally followed by more
/// alphanumeric characters (e.g. `v1`, `v1beta`, `v4`, `v2preview`).
fn upstream_last_segment_is_version(upstream: &url::Url) -> bool {
    upstream
        .path_segments()
        .and_then(|mut s| s.next_back())
        .map(|seg| {
            let mut chars = seg.chars();
            chars.next() == Some('v')
                && chars.next().map_or(false, |c| c.is_ascii_digit())
                && chars.all(|c| c.is_ascii_alphanumeric())
        })
        .unwrap_or(false)
}

/// Strip the leading `/v{digits+alphanum}` segment from `path` and return the
/// remainder. Returns `None` if the path does not start with a version segment.
///
/// Examples:
/// - `/v1/chat/completions`  → `/chat/completions`
/// - `/v1beta/models/x:act`  → `/models/x:act`
/// - `/v1`                   → `/`
/// - `/api/v1/foo`           → `None`
fn strip_version_prefix(path: &str) -> Option<&str> {
    let without_slash = path.strip_prefix('/')?;
    let (seg, _) = without_slash
        .split_once('/')
        .unwrap_or((without_slash, ""));
    let mut chars = seg.chars();
    if chars.next() != Some('v') {
        return None;
    }
    if !chars.next()?.is_ascii_digit() {
        return None;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let suffix = &path[1 + seg.len()..];
    if suffix.is_empty() { Some("/") } else { Some(suffix) }
}

/// Returns `true` for the four known LLM ingress paths this proxy handles.
fn is_known_ingress(path: &str) -> bool {
    path == "/v1/chat/completions"
        || path == "/v1/responses"
        || path == "/v1/messages"
        || (path.starts_with("/v1beta/models/") && path.contains(':'))
}

// ── Header forwarding ──────────────────────────────────────────────────────

fn forward_request_headers(incoming: &HeaderMap) -> reqwest::header::HeaderMap {
    let mut out = reqwest::header::HeaderMap::new();
    for (name, value) in incoming.iter() {
        if is_hop_by_hop(name.as_str()) {
            continue;
        }
        if name.as_str().eq_ignore_ascii_case("host") {
            continue;
        }
        if let (Ok(n), Ok(v)) = (
            reqwest::header::HeaderName::from_bytes(name.as_str().as_bytes()),
            reqwest::header::HeaderValue::from_bytes(value.as_bytes()),
        ) {
            out.insert(n, v);
        }
    }
    out
}

fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailers"
            | "transfer-encoding"
            | "upgrade"
            | "content-length"
    )
}

// ── JSON helpers ───────────────────────────────────────────────────────────

/// Convert a `HeaderMap` to a flat JSON object (last value wins on duplicate keys).
fn headers_to_json(headers: &HeaderMap) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (k, v) in headers.iter() {
        map.insert(
            k.to_string(),
            serde_json::Value::String(v.to_str().unwrap_or("?").to_string()),
        );
    }
    serde_json::Value::Object(map)
}

/// Body bytes → compact JSON value. Parses as JSON if possible; falls back to
/// a plain string. Empty body becomes `null`.
fn body_to_json(bytes: &[u8]) -> serde_json::Value {
    if bytes.is_empty() {
        return serde_json::Value::Null;
    }
    let text = String::from_utf8_lossy(bytes);
    serde_json::from_str(&text).unwrap_or_else(|_| serde_json::Value::String(text.to_string()))
}

/// Parse all `data:` lines from an SSE buffer into JSON values, skipping `[DONE]`.
fn parse_sse_data(buf: &[u8]) -> Vec<serde_json::Value> {
    let text = String::from_utf8_lossy(buf);
    text.split("\n\n")
        .flat_map(|block| block.lines())
        .filter(|line| line.starts_with("data: "))
        .filter_map(|line| {
            let data = line["data: ".len()..].trim();
            if data == "[DONE]" {
                return None;
            }
            serde_json::from_str(data).ok()
        })
        .collect()
}

/// Dispatch to the correct protocol assembler based on the ingress path, and
/// produce a single assembled JSON value suitable for log output.
fn assemble_sse(path: &str, buf: &[u8]) -> serde_json::Value {
    let chunks = parse_sse_data(buf);
    if path == "/v1/messages" {
        assemble_anthropic(&chunks)
    } else if path == "/v1/chat/completions" {
        assemble_openai_chat(&chunks)
    } else if path == "/v1/responses" {
        assemble_openai_responses(&chunks)
    } else if path.starts_with("/v1beta/models/") {
        assemble_google(&chunks)
    } else {
        serde_json::Value::Array(chunks)
    }
}

/// Anthropic Messages SSE → assembled message object.
///
/// Accumulates `text_delta` content, captures `message_start` metadata and
/// `message_delta` stop reason + usage counts.
///
/// Token priority: if `message_delta.usage.input_tokens` is non-zero it wins
/// over `message_start.usage.input_tokens` — ZhipuAI / MiniMax publish the
/// real value there instead of in `message_start`.
fn assemble_anthropic(chunks: &[serde_json::Value]) -> serde_json::Value {
    let mut id = String::new();
    let mut model = String::new();
    let mut text = String::new();
    let mut stop_reason = serde_json::Value::Null;
    let mut input_tokens: u64 = 0;
    let mut output_tokens: u64 = 0;
    let mut cache_read: Option<u64> = None;
    let mut cache_creation: Option<u64> = None;
    let mut web_search_requests: Option<u64> = None;
    let mut web_fetch_requests: Option<u64> = None;

    let read_cache_fields = |usage: &serde_json::Value,
                              cache_read: &mut Option<u64>,
                              cache_creation: &mut Option<u64>,
                              web_search: &mut Option<u64>,
                              web_fetch: &mut Option<u64>| {
        if let Some(v) = usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()) {
            *cache_read = Some(v);
        }
        if let Some(v) = usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64()) {
            *cache_creation = Some(v);
        }
        if let Some(stu) = usage.get("server_tool_use") {
            if let Some(v) = stu.get("web_search_requests").and_then(|v| v.as_u64()) {
                *web_search = Some(v);
            }
            if let Some(v) = stu.get("web_fetch_requests").and_then(|v| v.as_u64()) {
                *web_fetch = Some(v);
            }
        }
    };

    for chunk in chunks {
        match chunk.get("type").and_then(|t| t.as_str()) {
            Some("message_start") => {
                if let Some(msg) = chunk.get("message") {
                    id = msg.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    model = msg.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    if let Some(usage) = msg.get("usage") {
                        input_tokens =
                            usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        read_cache_fields(
                            usage,
                            &mut cache_read,
                            &mut cache_creation,
                            &mut web_search_requests,
                            &mut web_fetch_requests,
                        );
                    }
                }
            }
            Some("content_block_delta") => {
                if let Some(delta) = chunk.get("delta") {
                    match delta.get("type").and_then(|t| t.as_str()) {
                        Some("text_delta") => {
                            if let Some(t) = delta.get("text").and_then(|v| v.as_str()) {
                                text.push_str(t);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Some("message_delta") => {
                stop_reason = chunk
                    .pointer("/delta/stop_reason")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                if let Some(usage) = chunk.get("usage") {
                    // Override input_tokens when message_delta carries the real value
                    // (ZhipuAI / MiniMax pattern).
                    let delta_input =
                        usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                    if delta_input > 0 {
                        input_tokens = delta_input;
                    }
                    output_tokens =
                        usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                    read_cache_fields(
                        usage,
                        &mut cache_read,
                        &mut cache_creation,
                        &mut web_search_requests,
                        &mut web_fetch_requests,
                    );
                }
            }
            _ => {}
        }
    }

    let mut usage_obj = serde_json::json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
    });
    if let Some(v) = cache_read {
        usage_obj["cache_read_input_tokens"] = v.into();
    }
    if let Some(v) = cache_creation {
        usage_obj["cache_creation_input_tokens"] = v.into();
    }
    if web_search_requests.is_some() || web_fetch_requests.is_some() {
        usage_obj["server_tool_use"] = serde_json::json!({
            "web_search_requests": web_search_requests.unwrap_or(0),
            "web_fetch_requests": web_fetch_requests.unwrap_or(0),
        });
    }

    serde_json::json!({
        "id": id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [{"type": "text", "text": text}],
        "stop_reason": stop_reason,
        "usage": usage_obj
    })
}

/// OpenAI Chat Completions SSE → assembled chat completion object.
///
/// Accumulates `choices[0].delta.content`, captures id/model/finish_reason/usage.
fn assemble_openai_chat(chunks: &[serde_json::Value]) -> serde_json::Value {
    let mut id = String::new();
    let mut model = String::new();
    let mut content = String::new();
    let mut finish_reason = serde_json::Value::Null;
    let mut usage = serde_json::Value::Null;

    for chunk in chunks {
        if id.is_empty() {
            id = chunk.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            model = chunk.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();
        }
        if let Some(choices) = chunk.get("choices").and_then(|v| v.as_array()) {
            if let Some(choice) = choices.first() {
                if let Some(c) = choice.pointer("/delta/content").and_then(|v| v.as_str()) {
                    content.push_str(c);
                }
                if let Some(fr) = choice.get("finish_reason") {
                    if !fr.is_null() {
                        finish_reason = fr.clone();
                    }
                }
            }
        }
        if let Some(u) = chunk.get("usage") {
            if !u.is_null() {
                usage = u.clone();
            }
        }
    }

    serde_json::json!({
        "id": id,
        "model": model,
        "choices": [{"message": {"role": "assistant", "content": content}, "finish_reason": finish_reason}],
        "usage": usage
    })
}

/// OpenAI Responses API SSE → assembled response object.
///
/// Returns the `response` field from the `response.completed` event if present,
/// otherwise accumulates text from `response.output_text.delta` events.
fn assemble_openai_responses(chunks: &[serde_json::Value]) -> serde_json::Value {
    // Prefer the completed event which contains the full assembled response.
    for chunk in chunks {
        if chunk.get("type").and_then(|t| t.as_str()) == Some("response.completed") {
            if let Some(response) = chunk.get("response") {
                return response.clone();
            }
        }
    }
    // Fallback: accumulate text deltas.
    let mut text = String::new();
    for chunk in chunks {
        if chunk.get("type").and_then(|t| t.as_str()) == Some("response.output_text.delta") {
            if let Some(delta) = chunk.get("delta").and_then(|v| v.as_str()) {
                text.push_str(delta);
            }
        }
    }
    serde_json::json!({
        "output": [{"type": "message", "content": [{"type": "output_text", "text": text}]}]
    })
}

/// Google GenerateContent SSE → assembled response object.
///
/// Accumulates `candidates[0].content.parts[*].text`, captures finishReason
/// and usageMetadata from the last chunk that provides them.
fn assemble_google(chunks: &[serde_json::Value]) -> serde_json::Value {
    let mut text = String::new();
    let mut finish_reason = serde_json::Value::Null;
    let mut usage = serde_json::Value::Null;

    for chunk in chunks {
        if let Some(candidates) = chunk.get("candidates").and_then(|v| v.as_array()) {
            if let Some(candidate) = candidates.first() {
                if let Some(parts) = candidate.pointer("/content/parts").and_then(|v| v.as_array()) {
                    for part in parts {
                        if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                            text.push_str(t);
                        }
                    }
                }
                if let Some(fr) = candidate.get("finishReason") {
                    if !fr.is_null() {
                        finish_reason = fr.clone();
                    }
                }
            }
        }
        if let Some(u) = chunk.get("usageMetadata") {
            usage = u.clone();
        }
    }

    serde_json::json!({
        "candidates": [{"content": {"parts": [{"text": text}], "role": "model"}, "finishReason": finish_reason}],
        "usageMetadata": usage
    })
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // build_target_url

    #[test]
    fn standard_v1_openai_chat() {
        let upstream = url::Url::parse("https://api.openai.com/v1").unwrap();
        let uri: Uri = "/v1/chat/completions?stream=true".parse().unwrap();
        let merged = build_target_url(&upstream, &uri).unwrap();
        assert_eq!(
            merged.as_str(),
            "https://api.openai.com/v1/chat/completions?stream=true"
        );
    }

    #[test]
    fn zhipu_paas_v4_strips_client_v1() {
        let upstream = url::Url::parse("https://open.bigmodel.cn/api/coding/paas/v4").unwrap();
        let uri: Uri = "/v1/chat/completions".parse().unwrap();
        let merged = build_target_url(&upstream, &uri).unwrap();
        assert_eq!(
            merged.as_str(),
            "https://open.bigmodel.cn/api/coding/paas/v4/chat/completions"
        );
    }

    #[test]
    fn anthropic_under_sub_mount() {
        let upstream = url::Url::parse("https://open.bigmodel.cn/api/anthropic/v1").unwrap();
        let uri: Uri = "/v1/messages".parse().unwrap();
        let merged = build_target_url(&upstream, &uri).unwrap();
        assert_eq!(
            merged.as_str(),
            "https://open.bigmodel.cn/api/anthropic/v1/messages"
        );
    }

    #[test]
    fn google_content_keeps_action_segment() {
        let upstream =
            url::Url::parse("https://generativelanguage.googleapis.com/v1beta").unwrap();
        let uri: Uri = "/v1beta/models/gemini-2.0-flash:streamGenerateContent?alt=sse"
            .parse()
            .unwrap();
        let merged = build_target_url(&upstream, &uri).unwrap();
        assert_eq!(
            merged.as_str(),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn upstream_without_version_appends_full_uri() {
        let upstream = url::Url::parse("https://proxy.example.com/api").unwrap();
        let uri: Uri = "/v1/chat/completions".parse().unwrap();
        let merged = build_target_url(&upstream, &uri).unwrap();
        assert_eq!(
            merged.as_str(),
            "https://proxy.example.com/api/v1/chat/completions"
        );
    }

    #[test]
    fn bare_host_appends_full_uri() {
        let upstream = url::Url::parse("https://proxy.example.com").unwrap();
        let uri: Uri = "/v1/responses".parse().unwrap();
        let merged = build_target_url(&upstream, &uri).unwrap();
        assert_eq!(
            merged.as_str(),
            "https://proxy.example.com/v1/responses"
        );
    }

    // upstream_last_segment_is_version

    #[test]
    fn version_segment_detection() {
        let is_ver =
            |s: &str| upstream_last_segment_is_version(&url::Url::parse(s).unwrap());
        assert!(is_ver("https://api.openai.com/v1"));
        assert!(is_ver("https://api.openai.com/v1beta"));
        assert!(is_ver("https://api.openai.com/v3"));
        assert!(is_ver("https://api.example.com/api/paas/v4"));
        assert!(!is_ver("https://api.openai.com"));
        assert!(!is_ver("https://api.openai.com/api"));
        assert!(!is_ver("https://api.openai.com/1v"));
        assert!(!is_ver("https://api.openai.com/version1"));
    }

    // strip_version_prefix

    #[test]
    fn strip_version_prefix_cases() {
        assert_eq!(
            strip_version_prefix("/v1/chat/completions"),
            Some("/chat/completions")
        );
        assert_eq!(
            strip_version_prefix("/v1beta/models/x:act"),
            Some("/models/x:act")
        );
        assert_eq!(strip_version_prefix("/v1"), Some("/"));
        assert_eq!(strip_version_prefix("/api/v1/foo"), None);
        assert_eq!(strip_version_prefix("/health"), None);
        assert_eq!(strip_version_prefix("/v1beta"), Some("/"));
    }

    // is_known_ingress

    #[test]
    fn known_ingress_paths() {
        assert!(is_known_ingress("/v1/chat/completions"));
        assert!(is_known_ingress("/v1/responses"));
        assert!(is_known_ingress("/v1/messages"));
        assert!(is_known_ingress(
            "/v1beta/models/gemini-2.0-flash:streamGenerateContent"
        ));
        assert!(is_known_ingress("/v1beta/models/x:generateContent"));
    }

    #[test]
    fn unknown_ingress_paths() {
        assert!(!is_known_ingress("/health"));
        assert!(!is_known_ingress("/v1/embeddings"));
        assert!(!is_known_ingress("/v1beta/models/x")); // no colon
        assert!(!is_known_ingress("/"));
        assert!(!is_known_ingress(""));
    }

    // assemble_sse

    #[test]
    fn sse_anthropic_assembles_text() {
        let buf = concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-3\",\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" world\"}}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\n\n",
            "data: [DONE]\n\n",
        );
        let v = assemble_sse("/v1/messages", buf.as_bytes());
        assert_eq!(v["id"].as_str().unwrap(), "msg_1");
        assert_eq!(v["model"].as_str().unwrap(), "claude-3");
        assert_eq!(v["content"][0]["text"].as_str().unwrap(), "Hello world");
        assert_eq!(v["stop_reason"].as_str().unwrap(), "end_turn");
        assert_eq!(v["usage"]["input_tokens"].as_u64().unwrap(), 10);
        assert_eq!(v["usage"]["output_tokens"].as_u64().unwrap(), 5);
    }

    #[test]
    fn sse_openai_chat_assembles_content() {
        let buf = concat!(
            "data: {\"id\":\"c1\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"Hi\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"c1\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"content\":\"!\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\n",
            "data: [DONE]\n\n",
        );
        let v = assemble_sse("/v1/chat/completions", buf.as_bytes());
        assert_eq!(v["id"].as_str().unwrap(), "c1");
        assert_eq!(v["choices"][0]["message"]["content"].as_str().unwrap(), "Hi!");
        assert_eq!(v["choices"][0]["finish_reason"].as_str().unwrap(), "stop");
    }

    #[test]
    fn sse_done_is_skipped_in_parse() {
        let buf = b"data: {\"a\":1}\n\ndata: [DONE]\n\n";
        let chunks = parse_sse_data(buf);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0]["a"].as_u64().unwrap(), 1);
    }

    // body_to_json

    #[test]
    fn body_json_parsed() {
        let buf = br#"{"model":"gpt-4o"}"#;
        let v = body_to_json(buf);
        assert!(v.is_object());
        assert_eq!(v["model"].as_str().unwrap(), "gpt-4o");
    }

    #[test]
    fn body_non_json_fallback() {
        let buf = b"plain text";
        let v = body_to_json(buf);
        assert_eq!(v.as_str().unwrap(), "plain text");
    }

    #[test]
    fn body_empty_is_null() {
        assert!(body_to_json(b"").is_null());
    }

    // ── assemble_anthropic tests (Task 3) ──

    fn start_chunk(input_tokens: u64) -> serde_json::Value {
        serde_json::json!({
            "type": "message_start",
            "message": {
                "id": "msg_t",
                "model": "test-model",
                "usage": {"input_tokens": input_tokens, "output_tokens": 0}
            }
        })
    }

    fn delta_chunk(input_tokens: u64, output_tokens: u64) -> serde_json::Value {
        serde_json::json!({
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn"},
            "usage": {"input_tokens": input_tokens, "output_tokens": output_tokens}
        })
    }

    #[test]
    fn assemble_anthropic_delta_overrides_start_input_tokens() {
        // ZhipuAI pattern: message_start.input_tokens=0, real value in message_delta.
        let chunks = vec![start_chunk(0), delta_chunk(60, 43)];
        let result = assemble_anthropic(&chunks);
        let u = &result["usage"];
        assert_eq!(u["input_tokens"].as_u64(), Some(60), "message_delta wins when > 0");
        assert_eq!(u["output_tokens"].as_u64(), Some(43));
    }

    #[test]
    fn assemble_anthropic_keeps_start_input_when_delta_is_zero() {
        // Standard pattern: input_tokens in message_start, delta has no input field.
        let chunks = vec![
            start_chunk(100),
            serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": 50}
            }),
        ];
        let result = assemble_anthropic(&chunks);
        let u = &result["usage"];
        assert_eq!(u["input_tokens"].as_u64(), Some(100), "start value kept");
        assert_eq!(u["output_tokens"].as_u64(), Some(50));
    }

    #[test]
    fn assemble_anthropic_cache_and_tool_fields() {
        let chunks = vec![
            start_chunk(10),
            serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {
                    "output_tokens": 5,
                    "cache_read_input_tokens": 3,
                    "server_tool_use": {
                        "web_search_requests": 2,
                        "web_fetch_requests": 1
                    }
                }
            }),
        ];
        let result = assemble_anthropic(&chunks);
        let u = &result["usage"];
        assert_eq!(u["cache_read_input_tokens"].as_u64(), Some(3));
        // cache_creation_input_tokens was not present → must be absent from output
        assert!(
            u.get("cache_creation_input_tokens").is_none(),
            "absent field must not appear in assembled output",
        );
        assert_eq!(u["server_tool_use"]["web_search_requests"].as_u64(), Some(2));
        assert_eq!(u["server_tool_use"]["web_fetch_requests"].as_u64(), Some(1));
    }
}
