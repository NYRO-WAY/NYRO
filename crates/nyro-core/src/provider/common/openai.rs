//! OpenAI-compatible adapter primitives shared by every OpenAI-family vendor.
//!
//! This module provides auth / URL helpers and a zero-size generic adapter.
//! The 7-step request/response pipeline lives in [`super::pipeline`].
//!
//! # Usage
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! Tests cover URL building (`openai_build_url` / `base_ends_with_version_segment`)
    //! — versioned vs non-versioned bases.
    //!
    //! Pipeline auth-gate tests live in `provider::common::pipeline::tests`.
    use super::*;

    #[test]
    fn version_segment_recognition() {
        assert!(base_ends_with_version_segment("https://api.openai.com/v1"));
        assert!(base_ends_with_version_segment(
            "https://open.bigmodel.cn/api/coding/paas/v4"
        ));
        assert!(base_ends_with_version_segment(
            "https://api.deepseek.com/v1/"
        ));
        assert!(base_ends_with_version_segment("https://example.com/v123"));
        assert!(base_ends_with_version_segment(
            "https://generativelanguage.googleapis.com/v1beta"
        ));
        assert!(base_ends_with_version_segment(
            "https://example.com/v2alpha"
        ));
        assert!(base_ends_with_version_segment(
            "https://example.com/v3stable"
        ));

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
        assert!(!base_ends_with_version_segment(
            "https://example.com/vendor"
        ));
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
}
