//! Anthropic Claude Code OAuth channel.
//!
//! Auth-specific headers are injected by `ClaudeOAuthDriver` through
//! `RuntimeBinding.extra_headers`; this channel extension just gives the
//! resolver a concrete `(vendor=anthropic, channel=claude-code)` target
//! and intentionally returns no fallback auth headers so that flipping
//! `disable_default_auth` cannot leak an empty `x-api-key`.

use reqwest::header::HeaderMap;

use crate::provider::registry::{ExtensionRegistration, VendorScope};
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
    // `x-api-key`. The proxy pipeline resolves the vendor by `vendor_id`
    // and the gate lives in `provider::common::pipeline::build_request`
    // (`if ctx.disable_default_auth { HeaderMap::new() }`).
    fn auth_headers(&self, _ctx: &VendorCtx<'_>) -> HeaderMap {
        HeaderMap::new()
    }
}

inventory::submit! {
    ExtensionRegistration { make: || Box::new(AnthropicClaudeCodeChannel) }
}
