//! Anthropic Claude Code OAuth channel.
//!
//! Auth-specific headers are injected by `ClaudeOAuthDriver` through
//! `RuntimeBinding.extra_headers`; this channel extension just gives the
//! resolver a concrete `(vendor=anthropic, channel=claude-code)` target
//! and intentionally returns no fallback auth headers so that flipping
//! `disable_default_auth` cannot leak an empty `x-api-key`.

use reqwest::header::HeaderMap;

use crate::provider::registry::{VendorRegistration, VendorScope};
use crate::provider::vendor_ext::{VendorCtx, VendorExtension};

pub struct AnthropicClaudeCodeChannel;

impl VendorExtension for AnthropicClaudeCodeChannel {
    fn scope(&self) -> VendorScope {
        VendorScope::Channel {
            vendor_id: "anthropic",
            channel_id: "claude-code",
        }
    }

    // OAuth credentials live in `RuntimeBinding.extra_headers`. Returning
    // an empty map here is defense-in-depth for the `VendorRegistry`
    // three-tier `Channel → Vendor → Family` resolution path (used by
    // admin-side flows), where this channel extension can be the seam
    // that would otherwise fall back to `AnthropicVendor.auth_headers`'s
    // `x-api-key`. The proxy `ProviderAdapter` path resolves the adapter
    // by `vendor_id` and never reaches this `auth_headers` impl — that
    // path's gate lives in `provider::common::openai::openai_compat_build_request`
    // (`if ctx.disable_default_auth { HeaderMap::new() }`).
    fn auth_headers(&self, _ctx: &VendorCtx<'_>) -> HeaderMap {
        HeaderMap::new()
    }
}

inventory::submit! {
    VendorRegistration { make: || Box::new(AnthropicClaudeCodeChannel) }
}
