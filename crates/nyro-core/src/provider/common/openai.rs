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
    //
    // OAuth drivers (codex, claude-code) stash their Bearer + provider-
    // specific headers in `RuntimeBinding.extra_headers` and ask the
    // dispatcher to skip the vendor's default `auth_headers` via
    // `ctx.disable_default_auth`. Skipping unconditionally would break
    // every API-key path; gating here keeps the OAuth invariant
    // ("no leaked empty x-api-key") in a single seam shared by every
    // openai-compat adapter.
    let mut headers = if ctx.disable_default_auth {
        reqwest::header::HeaderMap::new()
    } else {
        vendor.auth_headers(&vendor_ctx)
    };
    // Anthropic-protocol upstreams require `x-api-key` instead of
    // `Authorization: Bearer`. Most OpenAI-compatible vendors blindly emit
    // Bearer; rewrite here so any vendor with a declared anthropic endpoint
    // works out of the box.
    //
    // Skipped under `disable_default_auth`: when an OAuth driver owns auth
    // (claude-code uses `Bearer <oauth_token>` + `anthropic-beta=
    // oauth-2025-04-20`), `ctx.api_key` is the OAuth Bearer token, NOT a
    // real Anthropic API key. Rewriting it here would forward the Bearer
    // as a fake `x-api-key` and break the OAuth handshake.
    if !ctx.disable_default_auth
        && ctx.protocol.family == crate::protocol::ids::ProtocolFamily::Anthropic
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
    //! Tests cover two seams in this module:
    //!
    //! 1. URL building (`openai_build_url` / `base_ends_with_version_segment`)
    //!    — versioned vs non-versioned bases.
    //!
    //! 2. The `disable_default_auth` gate inside
    //!    `openai_compat_build_request`. Every `ProviderAdapter`
    //!    (anthropic / openai / google / deepseek / zai / zhipuai /
    //!    minimax / moonshotai / nvidia / custom) routes through this
    //!    helper. When `ProviderCtx.disable_default_auth` is set, the
    //!    vendor's default `auth_headers` AND the Anthropic-egress
    //!    `Authorization → x-api-key` rewrite MUST be suppressed —
    //!    otherwise OAuth providers either leak an empty `x-api-key`
    //!    (the PR #86 bug) or forward an OAuth Bearer as a fake
    //!    `x-api-key` (the PR #105 interaction). Both directions are
    //!    pinned so a future refactor that flips a gate fails loudly.
    use super::*;
    use crate::Gateway;
    use crate::GatewayConfig;
    use crate::db::models::Provider;
    use crate::protocol::ids::{ANTHROPIC_MESSAGES_2023_06_01, OPENAI_CHAT_V1};
    use crate::protocol::types::{InternalMessage, InternalRequest, MessageContent, Role};
    use crate::provider::adapter::ProviderCtx;
    use crate::provider::registry::VendorScope;
    use crate::provider::vendor_ext::{VendorCtx, VendorExtension};
    use reqwest::header::HeaderMap as ExtHeaderMap;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use uuid::Uuid;

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
            openai_build_url("https://open.bigmodel.cn/api/anthropic", "/v1/messages"),
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

    /// Stand-in vendor: always tries to inject `x-api-key: <ctx.api_key>`,
    /// mirroring how `AnthropicVendor::auth_headers` and friends behave.
    /// The seam under test is "this fallback gets suppressed under
    /// `disable_default_auth`".
    struct FakeApiKeyVendor;

    impl VendorExtension for FakeApiKeyVendor {
        fn scope(&self) -> VendorScope {
            VendorScope::Vendor {
                vendor_id: "fake-test",
            }
        }
        fn auth_headers(&self, ctx: &VendorCtx<'_>) -> ExtHeaderMap {
            let mut h = ExtHeaderMap::new();
            if !ctx.api_key.is_empty() {
                h.insert(
                    "x-api-key",
                    reqwest::header::HeaderValue::from_str(ctx.api_key).unwrap(),
                );
            }
            h
        }
    }

    /// Mirrors most OpenAI-compat vendors (zhipuai/deepseek/custom...):
    /// emit `Authorization: Bearer <ctx.api_key>`. PR #105's rewrite block
    /// then converts this to `x-api-key` when egress family is Anthropic.
    /// We reuse it to test the OAuth interaction (rewrite must NOT fire
    /// when an OAuth driver owns auth).
    struct FakeBearerVendor;

    impl VendorExtension for FakeBearerVendor {
        fn scope(&self) -> VendorScope {
            VendorScope::Vendor {
                vendor_id: "fake-bearer",
            }
        }
        fn auth_headers(&self, ctx: &VendorCtx<'_>) -> ExtHeaderMap {
            let mut h = ExtHeaderMap::new();
            if !ctx.api_key.is_empty() {
                h.insert(
                    reqwest::header::AUTHORIZATION,
                    reqwest::header::HeaderValue::from_str(&format!("Bearer {}", ctx.api_key))
                        .unwrap(),
                );
            }
            h
        }
    }

    fn provider_with_api_key(api_key: &str) -> Provider {
        Provider {
            id: "p".into(),
            name: "p".into(),
            vendor: Some("fake-test".into()),
            protocol: "openai".into(),
            base_url: "https://upstream.local".into(),
            default_protocol: "openai".into(),
            protocol_endpoints: String::new(),
            preset_key: Some("fake-test".into()),
            channel: Some("default".into()),
            models_source: None,
            capabilities_source: None,
            static_models: None,
            api_key: api_key.into(),
            auth_mode: "apikey".into(),
            use_proxy: false,
            last_test_success: None,
            last_test_at: None,
            is_enabled: true,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn minimal_chat_request() -> InternalRequest {
        InternalRequest {
            messages: vec![InternalMessage {
                role: Role::User,
                content: MessageContent::Text("ping".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            model: "ignored-by-actual-model".into(),
            stream: false,
            temperature: None,
            max_tokens: None,
            top_p: None,
            tools: None,
            tool_choice: None,
            source_protocol: OPENAI_CHAT_V1,
            extra: HashMap::new(),
        }
    }

    async fn build_test_gateway() -> Gateway {
        let mut config = GatewayConfig::default();
        config.data_dir = PathBuf::from(std::env::temp_dir())
            .join(format!("nyro-disable-default-auth-test-{}", Uuid::new_v4()));
        let (gw, _log_rx) = Gateway::new(config).await.expect("gateway init");
        gw
    }

    #[tokio::test]
    async fn build_request_suppresses_default_auth_when_oauth_owns_it() {
        let gw = build_test_gateway().await;
        let provider = provider_with_api_key("would-leak-if-bypassed");
        let mut req = minimal_chat_request();
        let ctx = ProviderCtx {
            provider: &provider,
            protocol: OPENAI_CHAT_V1,
            egress_base_url: "https://upstream.local",
            api_key: &provider.api_key,
            actual_model: "gpt-test",
            credential: None,
            gw: &gw,
            disable_default_auth: true,
        };
        let out = openai_compat_build_request(&FakeApiKeyVendor, &mut req, &ctx)
            .await
            .expect("build_request succeeds");
        assert!(
            out.headers.get("x-api-key").is_none(),
            "OAuth provider must not emit fallback x-api-key, got: {:?}",
            out.headers.get("x-api-key"),
        );
    }

    #[tokio::test]
    async fn build_request_keeps_default_auth_when_no_oauth() {
        let gw = build_test_gateway().await;
        let provider = provider_with_api_key("apikey-abc");
        let mut req = minimal_chat_request();
        let ctx = ProviderCtx {
            provider: &provider,
            protocol: OPENAI_CHAT_V1,
            egress_base_url: "https://upstream.local",
            api_key: &provider.api_key,
            actual_model: "gpt-test",
            credential: None,
            gw: &gw,
            disable_default_auth: false,
        };
        let out = openai_compat_build_request(&FakeApiKeyVendor, &mut req, &ctx)
            .await
            .expect("build_request succeeds");
        assert_eq!(
            out.headers.get("x-api-key").and_then(|v| v.to_str().ok()),
            Some("apikey-abc"),
            "API-key path must still propagate x-api-key to upstream",
        );
    }

    /// Pins the rebase-time interaction with PR #105: when an OAuth driver
    /// owns auth (`disable_default_auth=true`) AND the egress family is
    /// Anthropic, the `Authorization → x-api-key` rewrite must NOT fire.
    /// `ctx.api_key` on the OAuth path is the OAuth Bearer token itself —
    /// rewriting it as `x-api-key` would forward the Bearer as a fake API
    /// key and break the OAuth handshake.
    #[tokio::test]
    async fn build_request_does_not_rewrite_oauth_bearer_to_xapikey_on_anthropic_egress() {
        let gw = build_test_gateway().await;
        let provider = provider_with_api_key("");
        let mut req = minimal_chat_request();
        let ctx = ProviderCtx {
            provider: &provider,
            protocol: ANTHROPIC_MESSAGES_2023_06_01,
            egress_base_url: "https://api.anthropic.com",
            api_key: "oauth_bearer_token_should_not_become_xapikey",
            actual_model: "claude-sonnet-4-6",
            credential: None,
            gw: &gw,
            disable_default_auth: true,
        };
        let out = openai_compat_build_request(&FakeBearerVendor, &mut req, &ctx)
            .await
            .expect("build_request succeeds");
        assert!(
            out.headers.get("x-api-key").is_none(),
            "OAuth Bearer must not be rewritten as x-api-key, got: {:?}",
            out.headers.get("x-api-key"),
        );
        assert!(
            out.headers.get(reqwest::header::AUTHORIZATION).is_none(),
            "default Authorization must be suppressed under disable_default_auth too, got: {:?}",
            out.headers.get(reqwest::header::AUTHORIZATION),
        );
    }

    /// Mirror of #105's main use case: API-key-mode OpenAI-compat vendor
    /// (e.g. zhipuai with anthropic endpoint) hitting Anthropic egress —
    /// the rewrite block MUST fire and turn `Authorization: Bearer` into
    /// `x-api-key`. Pinned alongside the OAuth-skip case so any future
    /// refactor of either gate fails loudly here.
    #[tokio::test]
    async fn build_request_rewrites_bearer_to_xapikey_on_anthropic_egress_for_apikey_path() {
        let gw = build_test_gateway().await;
        let provider = provider_with_api_key("real-anthropic-key");
        let mut req = minimal_chat_request();
        let ctx = ProviderCtx {
            provider: &provider,
            protocol: ANTHROPIC_MESSAGES_2023_06_01,
            egress_base_url: "https://api.anthropic.com",
            api_key: &provider.api_key,
            actual_model: "claude-sonnet-4-6",
            credential: None,
            gw: &gw,
            disable_default_auth: false,
        };
        let out = openai_compat_build_request(&FakeBearerVendor, &mut req, &ctx)
            .await
            .expect("build_request succeeds");
        assert_eq!(
            out.headers.get("x-api-key").and_then(|v| v.to_str().ok()),
            Some("real-anthropic-key"),
            "API-key path on Anthropic egress must produce x-api-key",
        );
        assert!(
            out.headers.get(reqwest::header::AUTHORIZATION).is_none(),
            "Authorization must be removed once x-api-key is set",
        );
    }
}
