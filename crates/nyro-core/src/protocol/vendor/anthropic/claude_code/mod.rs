//! Anthropic Claude Code channel.
//!
//! Auth-specific headers are injected by `ClaudeOAuthDriver` through
//! `RuntimeBinding.extra_headers`; this channel extension just gives the
//! resolver a concrete `(vendor=anthropic, channel=claude-code)` target.

use reqwest::header::HeaderMap;

use crate::protocol::vendor::defaults::AnthropicDefault;
use crate::protocol::vendor::{VendorCtx, VendorExtension, VendorRegistration, VendorScope};

pub struct AnthropicClaudeCodeChannel;

impl VendorExtension for AnthropicClaudeCodeChannel {
    fn scope(&self) -> VendorScope {
        VendorScope::Channel {
            vendor_id: "anthropic",
            channel_id: "claude-code",
        }
    }

    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap {
        AnthropicDefault.auth_headers(ctx)
    }

    fn build_url(&self, ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String {
        AnthropicDefault.build_url(ctx, base_url, path)
    }
}

inventory::submit! {
    VendorRegistration { make: || Box::new(AnthropicClaudeCodeChannel) }
}
