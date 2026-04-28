//! Anthropic family default. Mirrors the legacy `AnthropicAdapter`.

use reqwest::header::{HeaderMap, HeaderValue};

use crate::protocol::ids::ProtocolFamily;
use crate::protocol::vendor::{VendorCtx, VendorExtension, VendorRegistration, VendorScope};

pub struct AnthropicDefault;

impl VendorExtension for AnthropicDefault {
    fn scope(&self) -> VendorScope {
        VendorScope::Family(ProtocolFamily::Anthropic)
    }

    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            "x-api-key",
            HeaderValue::from_str(ctx.api_key).unwrap_or_else(|_| HeaderValue::from_static("")),
        );
        h.insert(
            "anthropic-version",
            HeaderValue::from_static("2023-06-01"),
        );
        h
    }
}

inventory::submit! {
    VendorRegistration { make: || Box::new(AnthropicDefault) }
}
