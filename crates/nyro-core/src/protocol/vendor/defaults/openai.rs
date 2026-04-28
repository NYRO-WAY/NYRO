//! OpenAI-compatible family default. Mirrors the legacy
//! `OpenAICompatAdapter`.

use reqwest::header::{HeaderMap, HeaderValue};

use crate::protocol::ids::ProtocolFamily;
use crate::protocol::vendor::{VendorCtx, VendorExtension, VendorRegistration, VendorScope};

pub struct OpenAiDefault;

impl VendorExtension for OpenAiDefault {
    fn scope(&self) -> VendorScope {
        VendorScope::Family(ProtocolFamily::OpenAI)
    }

    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Ok(value) = HeaderValue::from_str(&format!("Bearer {}", ctx.api_key)) {
            h.insert("Authorization", value);
        }
        h
    }

    fn build_url(&self, _ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String {
        let base = base_url.trim_end_matches('/');
        let adjusted = if has_non_root_path(base) && path.starts_with("/v1/") {
            &path[3..]
        } else {
            path
        };
        format!("{base}{adjusted}")
    }
}

inventory::submit! {
    VendorRegistration { make: || Box::new(OpenAiDefault) }
}

/// Returns true if `base` parses as a URL with a non-empty path
/// component (e.g. `https://api.example.com/v1`).
pub(crate) fn has_non_root_path(base: &str) -> bool {
    reqwest::Url::parse(base)
        .ok()
        .map(|url| {
            let pathname = url.path().trim_end_matches('/');
            !pathname.is_empty() && pathname != "/"
        })
        .unwrap_or(false)
}
