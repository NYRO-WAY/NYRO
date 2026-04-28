//! Anthropic vendor (direct API). Reuses the Anthropic family default
//! for headers/URL.
//!
//! `claude-code` is a planned OAuth channel — see the placeholder
//! submodule (`claude_code`) which is intentionally not registered
//! until its OAuth driver lands.

pub mod claude_code;

use reqwest::header::HeaderMap;

use crate::protocol::vendor::defaults::AnthropicDefault;
use crate::protocol::vendor::types::{
    AuthMode, ChannelDef, Label, ProtocolBaseUrl, VendorMetadata,
};
use crate::protocol::vendor::{VendorCtx, VendorExtension, VendorRegistration, VendorScope};

const METADATA: VendorMetadata = VendorMetadata {
    id: "anthropic",
    label: Label {
        zh: "Anthropic",
        en: "Anthropic",
    },
    icon: "anthropic",
    default_protocol: "anthropic",
    channels: &[ChannelDef {
        id: "default",
        label: Label {
            zh: "默认",
            en: "Default",
        },
        base_urls: &[ProtocolBaseUrl {
            protocol: "anthropic",
            base_url: "https://api.anthropic.com",
        }],
        api_key: None,
        models_source: Some("https://api.anthropic.com/v1/models"),
        capabilities_source: Some("ai://models.dev/anthropic"),
        static_models: &[],
        auth_mode: AuthMode::ApiKey,
        oauth: None,
        runtime: None,
    }],
};

pub struct AnthropicVendor;

impl VendorExtension for AnthropicVendor {
    fn scope(&self) -> VendorScope {
        VendorScope::Vendor {
            vendor_id: "anthropic",
        }
    }

    fn metadata(&self) -> Option<&'static VendorMetadata> {
        Some(&METADATA)
    }

    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap {
        AnthropicDefault.auth_headers(ctx)
    }

    fn build_url(&self, ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String {
        AnthropicDefault.build_url(ctx, base_url, path)
    }
}

inventory::submit! {
    VendorRegistration { make: || Box::new(AnthropicVendor) }
}
