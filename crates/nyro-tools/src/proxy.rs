//! `nyro-tools proxy` — transparent passthrough proxy for local debugging.
//!
//! Forwards every incoming request as-is to the configured upstream and
//! streams the response back. No body or header rewriting beyond the hop-by-hop
//! headers that the framework or `reqwest` would mishandle.
//!
//! This subcommand is **not** used by CI; record/replay is the canonical path.

use crate::protocol::{ProtocolKind, validate_upstream_endpoint};
use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    Router,
};
use clap::Args;
use futures::TryStreamExt;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Debug, Args)]
pub struct ProxyArgs {
    /// Listen port
    #[arg(short = 'P', long, default_value_t = 25208)]
    pub port: u16,

    /// Listen host
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    pub host: String,

    /// Upstream protocol (kebab-case short name) — currently informational, used in startup log
    #[arg(short = 'p', long, value_enum)]
    pub upstream_protocol: ProtocolKind,

    /// Upstream endpoint base URL (e.g. https://api.openai.com)
    #[arg(short = 'e', long)]
    pub upstream_endpoint: url::Url,
}

#[derive(Debug)]
struct ProxyState {
    upstream: url::Url,
    protocol: ProtocolKind,
    client: reqwest::Client,
}

pub async fn run(args: ProxyArgs) -> Result<()> {
    validate_upstream_endpoint(&args.upstream_endpoint)?;
    let state = Arc::new(ProxyState {
        upstream: args.upstream_endpoint.clone(),
        protocol: args.upstream_protocol,
        client: reqwest::Client::builder()
            .build()
            .context("failed to build reqwest client")?,
    });

    let host: std::net::IpAddr = args
        .host
        .parse()
        .with_context(|| format!("invalid host `{}`", args.host))?;
    let addr = SocketAddr::from((host, args.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;

    let app = Router::new().fallback(forward).with_state(state);
    info!(
        %addr,
        upstream = %args.upstream_endpoint,
        protocol = %args.upstream_protocol,
        "nyro-tools proxy listening"
    );
    axum::serve(listener, app).await?;
    Ok(())
}

async fn forward(State(state): State<Arc<ProxyState>>, req: Request) -> Response {
    match forward_inner(state, req).await {
        Ok(resp) => resp,
        Err(e) => {
            warn!(error = %e, "proxy forward failed");
            let body = serde_json::json!({"error": {"message": e.to_string()}});
            (StatusCode::BAD_GATEWAY, axum::Json(body)).into_response()
        }
    }
}

async fn forward_inner(state: Arc<ProxyState>, req: Request) -> Result<Response> {
    let (parts, body) = req.into_parts();

    let target = build_target_url(&state.upstream, state.protocol, &parts.uri)?;
    let method = parts.method.clone();
    let upstream_method = reqwest::Method::from_bytes(method.as_str().as_bytes())?;

    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .map_err(|e| anyhow::anyhow!("failed to buffer request body: {e}"))?;

    let mut request = state.client.request(upstream_method, target.clone());
    request = request.headers(forward_request_headers(&parts.headers));
    if !bytes.is_empty() {
        request = request.body(bytes.to_vec());
    }

    let upstream = request.send().await.with_context(|| {
        format!(
            "upstream request to {} {} failed",
            method,
            target
        )
    })?;

    let status = StatusCode::from_u16(upstream.status().as_u16())?;
    let mut response_headers = HeaderMap::new();
    for (name, value) in upstream.headers().iter() {
        if is_hop_by_hop(name.as_str()) {
            continue;
        }
        if let (Ok(n), Ok(v)) = (
            HeaderName::from_bytes(name.as_str().as_bytes()),
            HeaderValue::from_bytes(value.as_bytes()),
        ) {
            response_headers.insert(n, v);
        }
    }

    let body_stream = upstream
        .bytes_stream()
        .map_err(|e| std::io::Error::other(e.to_string()));
    let mut response = Response::new(Body::from_stream(body_stream));
    *response.status_mut() = status;
    *response.headers_mut() = response_headers;
    Ok(response)
}

/// Build the full upstream URL by:
/// 1. stripping the protocol's standard version prefix from the client-supplied path
///    (e.g. `/v1/chat/completions` → `/chat/completions`),
/// 2. appending the suffix to the user-configured `--upstream-endpoint` (which already
///    carries the API version segment, possibly non-standard like `/api/coding/paas/v4`).
fn build_target_url(
    upstream: &url::Url,
    protocol: ProtocolKind,
    incoming: &Uri,
) -> Result<url::Url> {
    let suffix = protocol
        .strip_client_version_prefix(incoming.path())
        .with_context(|| {
            format!(
                "proxy received an unexpected client path for protocol `{}`",
                protocol.as_short_name()
            )
        })?;

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

// silence unused imports when no debug tracing path uses Method
#[allow(dead_code)]
fn _force_method(_: Method) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_v1_openai_chat() {
        let upstream = url::Url::parse("https://api.openai.com/v1").unwrap();
        let uri: Uri = "/v1/chat/completions?stream=true".parse().unwrap();
        let merged = build_target_url(&upstream, ProtocolKind::OpenAiChat, &uri).unwrap();
        assert_eq!(
            merged.as_str(),
            "https://api.openai.com/v1/chat/completions?stream=true"
        );
    }

    #[test]
    fn zhipu_paas_v4_strips_client_v1() {
        let upstream = url::Url::parse("https://open.bigmodel.cn/api/coding/paas/v4").unwrap();
        let uri: Uri = "/v1/chat/completions".parse().unwrap();
        let merged = build_target_url(&upstream, ProtocolKind::OpenAiChat, &uri).unwrap();
        assert_eq!(
            merged.as_str(),
            "https://open.bigmodel.cn/api/coding/paas/v4/chat/completions"
        );
    }

    #[test]
    fn anthropic_under_sub_mount() {
        let upstream = url::Url::parse("https://open.bigmodel.cn/api/anthropic/v1").unwrap();
        let uri: Uri = "/v1/messages".parse().unwrap();
        let merged = build_target_url(&upstream, ProtocolKind::AnthropicMessages, &uri).unwrap();
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
        let merged = build_target_url(&upstream, ProtocolKind::GoogleContent, &uri).unwrap();
        assert_eq!(
            merged.as_str(),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn rejects_mismatched_client_path() {
        let upstream = url::Url::parse("https://api.openai.com/v1").unwrap();
        let uri: Uri = "/v2/chat/completions".parse().unwrap();
        let err = build_target_url(&upstream, ProtocolKind::OpenAiChat, &uri).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("openai-chat"));
    }
}
