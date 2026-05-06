//! OpenAI-compatible adapter primitives shared by every OpenAI-family vendor.
//!
//! # Usage
//!
//! Each OpenAI-compatible vendor delegates its `auth_headers` / `build_url`
//! implementations to the free functions below:
//!
//! ```rust,ignore
//! use crate::provider::common::openai::{openai_bearer_auth_headers, openai_build_url};
//!
//! impl VendorExtension for MyVendor {
//!     fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap {
//!         openai_bearer_auth_headers(ctx)
//!     }
//!     fn build_url(&self, _ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String {
//!         openai_build_url(base_url, path)
//!     }
//! }
//! ```

use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;

use crate::error::GatewayError;
use crate::protocol::types::InternalResponse;
use crate::provider::vendor_ext::VendorCtx;

// ── Free-function auth / URL primitives ──────────────────────────────────────

/// Produces a standard `Authorization: Bearer <key>` header map.
pub fn openai_bearer_auth_headers(ctx: &VendorCtx<'_>) -> HeaderMap {
    let mut h = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&format!("Bearer {}", ctx.api_key)) {
        h.insert("Authorization", value);
    }
    h
}

/// Builds an upstream URL.
///
/// If `base_url`'s path already ends with a version segment like `/v1` or
/// `/v4` (i.e. `/v` followed by digits), the leading `/v1/` prefix from
/// `path` is stripped to avoid double-versioning. Other non-root paths
/// (e.g. `/api/anthropic`) are left alone so that the encoder-emitted
/// `/v1/messages` is preserved.
pub fn openai_build_url(base_url: &str, path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    let adjusted = if base_ends_with_version_segment(base) && path.starts_with("/v1/") {
        &path[3..]
    } else {
        path
    };
    format!("{base}{adjusted}")
}

/// Returns `true` when the parsed URL path's last segment matches `^v\d+$`
/// (e.g. `/v1`, `/api/coding/paas/v4`). Returns `false` for `/`, empty path,
/// or non-version trailing segments like `/anthropic`.
fn base_ends_with_version_segment(base: &str) -> bool {
    reqwest::Url::parse(base)
        .ok()
        .map(|url| {
            let p = url.path().trim_end_matches('/');
            if p.is_empty() || p == "/" {
                return false;
            }
            let last = p.rsplit('/').next().unwrap_or("");
            is_version_segment(last)
        })
        .unwrap_or(false)
}

/// Recognizes version segments shaped like `v` + digits + optional alpha
/// suffix: `v1`, `v4`, `v1beta`, `v2alpha`, `v3stable`. Rejects `v`,
/// `vNext`, `vendor`, etc.
fn is_version_segment(s: &str) -> bool {
    let mut it = s.chars();
    if it.next() != Some('v') {
        return false;
    }
    let mut saw_digit = false;
    let mut digits_done = false;
    for c in it {
        if !digits_done && c.is_ascii_digit() {
            saw_digit = true;
        } else if saw_digit && c.is_ascii_alphabetic() {
            digits_done = true;
        } else {
            return false;
        }
    }
    saw_digit
}

/// Maps a non-2xx OpenAI-compatible HTTP response to a `GatewayError`.
pub fn openai_map_error(vendor_id: &str, status: u16, body: Value) -> GatewayError {
    let msg = body
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("upstream HTTP {status}"));
    GatewayError::upstream_status(vendor_id, status, Some(msg))
}

// ── GenericOpenAICompatibleAdapter ────────────────────────────────────────────

/// Zero-size adapter used by `custom/` and any vendor that needs pure
/// Bearer-auth + standard URL construction without custom overrides.
pub struct GenericOpenAICompatibleAdapter;

impl GenericOpenAICompatibleAdapter {
    pub fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap {
        openai_bearer_auth_headers(ctx)
    }
    pub fn build_url(&self, _ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String {
        openai_build_url(base_url, path)
    }
}

// ── Shared ProviderAdapter helpers ───────────────────────────────────────────

/// Shared `build_request` logic for any `VendorExtension` that is also a
/// `ProviderAdapter`. Calls the standard pipeline:
/// `pre_request → normalize_tool_results → pre_encode → codec_encode →
///  post_encode → auth_headers → build_url`.
pub async fn openai_compat_build_request<V>(
    vendor: &V,
    req: &mut crate::protocol::types::InternalRequest,
    ctx: &crate::provider::adapter::ProviderCtx<'_>,
) -> Result<crate::provider::outbound::OutboundRequest, GatewayError>
where
    V: crate::provider::vendor_ext::VendorExtension,
{
    // Set actual model before encoding so the codec uses the routed model.
    req.model = ctx.actual_model.to_string();

    let vendor_ctx = ctx.to_vendor_ctx();

    // 1. pre_request hook
    vendor
        .pre_request(&vendor_ctx, req, ctx.gw)
        .await
        .map_err(GatewayError::internal)?;

    // 2. normalize tool results
    crate::protocol::codec::tool_correlation::normalize_request_tool_results(req);

    // 3. pre_encode hook
    vendor
        .pre_encode(&vendor_ctx, req)
        .await
        .map_err(GatewayError::internal)?;

    // 4. codec encode
    let egress_handler = ctx.protocol.handler();
    let encoder = egress_handler.make_encoder();
    let (mut body, mut extra_headers) = encoder
        .encode_request(req)
        .map_err(GatewayError::internal)?;

    // 5. post_encode hook
    vendor
        .post_encode(&vendor_ctx, &mut body, &mut extra_headers)
        .await
        .map_err(GatewayError::internal)?;

    // 6. auth headers
    let mut headers = vendor.auth_headers(&vendor_ctx);
    // Anthropic-protocol upstreams require `x-api-key` instead of
    // `Authorization: Bearer`. Most OpenAI-compatible vendors blindly emit
    // Bearer; rewrite here so any vendor with a declared anthropic endpoint
    // works out of the box.
    if ctx.protocol.family == crate::protocol::ids::ProtocolFamily::Anthropic
        && !headers.contains_key("x-api-key")
    {
        headers.remove(reqwest::header::AUTHORIZATION);
        if let Ok(v) = reqwest::header::HeaderValue::from_str(ctx.api_key) {
            headers.insert("x-api-key", v);
        }
    }
    headers.extend(extra_headers);

    // 7. build URL
    let egress_path = encoder.egress_path(ctx.actual_model, req.stream);
    let url = vendor.build_url(&vendor_ctx, ctx.egress_base_url, &egress_path);

    Ok(crate::provider::outbound::OutboundRequest { url, headers, body })
}

/// Shared `parse_response` logic for any `VendorExtension` that is also a
/// `ProviderAdapter`.
pub async fn openai_compat_parse_response<V>(
    vendor: &V,
    resp: crate::provider::inbound::InboundResponse,
    ctx: &crate::provider::adapter::ProviderCtx<'_>,
) -> Result<crate::protocol::types::InternalResponse, GatewayError>
where
    V: crate::provider::vendor_ext::VendorExtension,
{
    let vendor_ctx = ctx.to_vendor_ctx();
    let mut body = resp.body;

    // 1. pre_parse hook
    vendor
        .pre_parse(&vendor_ctx, &mut body)
        .await
        .map_err(GatewayError::internal)?;

    // 2. codec parse
    let egress_handler = ctx.protocol.handler();
    let parser = egress_handler.make_response_parser();
    let mut internal_resp = parser
        .parse_response(body)
        .map_err(GatewayError::internal)?;

    // 3. reasoning normalization
    crate::protocol::codec::reasoning::normalize_response_reasoning(&mut internal_resp);

    // 4. post_parse hook
    vendor
        .post_parse(&vendor_ctx, &mut internal_resp)
        .await
        .map_err(GatewayError::internal)?;

    Ok(internal_resp)
}

/// Shared `stream_parser` factory for OpenAI-compatible vendors.
pub fn openai_compat_stream_parser(
    ctx: &crate::provider::adapter::ProviderCtx<'_>,
) -> Box<dyn crate::provider::stream::ProviderStreamParser + Send> {
    let egress_handler = ctx.protocol.handler();
    Box::new(crate::provider::stream::LegacyStreamParserAdapter(egress_handler.make_stream_parser()))
}

// ── ThinkTagExtractingParser ──────────────────────────────────────────────────

/// Strips `<think>…</think>` tags from `InternalResponse.content` and moves
/// the thinking text to `reasoning_content`.
pub struct ThinkTagExtractingParser;

impl ThinkTagExtractingParser {
    pub fn apply(resp: &mut InternalResponse) {
        crate::protocol::codec::reasoning::normalize_response_reasoning(resp);
    }

    pub fn split(content: &str) -> (Option<String>, String) {
        crate::protocol::codec::reasoning::split_think_tags(content)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_segment_recognition() {
        assert!(base_ends_with_version_segment("https://api.openai.com/v1"));
        assert!(base_ends_with_version_segment(
            "https://open.bigmodel.cn/api/coding/paas/v4"
        ));
        assert!(base_ends_with_version_segment("https://api.deepseek.com/v1/"));
        assert!(base_ends_with_version_segment("https://example.com/v123"));
        assert!(base_ends_with_version_segment(
            "https://generativelanguage.googleapis.com/v1beta"
        ));
        assert!(base_ends_with_version_segment("https://example.com/v2alpha"));
        assert!(base_ends_with_version_segment("https://example.com/v3stable"));

        assert!(!base_ends_with_version_segment(
            "https://open.bigmodel.cn/api/anthropic"
        ));
        assert!(!base_ends_with_version_segment(
            "https://api.deepseek.com/anthropic"
        ));
        assert!(!base_ends_with_version_segment("https://api.deepseek.com"));
        assert!(!base_ends_with_version_segment("https://api.deepseek.com/"));
        assert!(!base_ends_with_version_segment("https://example.com/vNext"));
        assert!(!base_ends_with_version_segment("https://example.com/v"));
        assert!(!base_ends_with_version_segment("https://example.com/vendor"));
        assert!(!base_ends_with_version_segment("https://example.com/v1b2"));
    }

    #[test]
    fn build_url_strips_v1_for_versioned_base() {
        assert_eq!(
            openai_build_url("https://api.openai.com/v1", "/v1/chat/completions"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            openai_build_url(
                "https://open.bigmodel.cn/api/coding/paas/v4",
                "/v1/chat/completions"
            ),
            "https://open.bigmodel.cn/api/coding/paas/v4/chat/completions"
        );
        assert_eq!(
            openai_build_url("https://api.deepseek.com/v1/", "/v1/chat/completions"),
            "https://api.deepseek.com/v1/chat/completions"
        );
    }

    #[test]
    fn build_url_preserves_v1_for_anthropic_base() {
        assert_eq!(
            openai_build_url(
                "https://open.bigmodel.cn/api/anthropic",
                "/v1/messages"
            ),
            "https://open.bigmodel.cn/api/anthropic/v1/messages"
        );
        assert_eq!(
            openai_build_url("https://api.deepseek.com/anthropic", "/v1/messages"),
            "https://api.deepseek.com/anthropic/v1/messages"
        );
    }

    #[test]
    fn build_url_passthrough_when_no_version_prefix() {
        assert_eq!(
            openai_build_url("https://api.example.com", "/v1/chat/completions"),
            "https://api.example.com/v1/chat/completions"
        );
        assert_eq!(
            openai_build_url("https://api.example.com/", "/v1/chat/completions"),
            "https://api.example.com/v1/chat/completions"
        );
    }
}
