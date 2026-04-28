//! MiniMax vendor (OpenAI-compatible). Two channels: global
//! (`default`) and China site (`china`).

use reqwest::header::HeaderMap;

use crate::protocol::vendor::defaults::OpenAiDefault;
use crate::protocol::vendor::types::{
    AuthMode, ChannelDef, Label, ProtocolBaseUrl, VendorMetadata,
};
use crate::protocol::vendor::{VendorCtx, VendorExtension, VendorRegistration, VendorScope};

const METADATA: VendorMetadata = VendorMetadata {
    id: "minimax",
    label: Label { zh: "MiniMax", en: "MiniMax" },
    icon: "minimax",
    default_protocol: "openai",
    channels: &[
        ChannelDef {
            id: "default",
            label: Label { zh: "默认", en: "Default" },
            base_urls: &[
                ProtocolBaseUrl {
                    protocol: "openai",
                    base_url: "https://api.minimax.io/v1",
                },
                ProtocolBaseUrl {
                    protocol: "anthropic",
                    base_url: "https://api.minimax.io/anthropic",
                },
            ],
            api_key: None,
            models_source: Some("ai://models.dev/minimax"),
            capabilities_source: Some("ai://models.dev/minimax"),
            static_models: &[],
            auth_mode: AuthMode::ApiKey,
            oauth: None,
            runtime: None,
        },
        ChannelDef {
            id: "china",
            label: Label { zh: "中国站", en: "China" },
            base_urls: &[
                ProtocolBaseUrl {
                    protocol: "openai",
                    base_url: "https://api.minimaxi.com/v1",
                },
                ProtocolBaseUrl {
                    protocol: "anthropic",
                    base_url: "https://api.minimaxi.com/anthropic",
                },
            ],
            api_key: None,
            models_source: Some("ai://models.dev/minimax"),
            capabilities_source: Some("ai://models.dev/minimax"),
            static_models: &[],
            auth_mode: AuthMode::ApiKey,
            oauth: None,
            runtime: None,
        },
    ],
};

pub struct MinimaxVendor;

impl VendorExtension for MinimaxVendor {
    fn scope(&self) -> VendorScope {
        VendorScope::Vendor { vendor_id: "minimax" }
    }
    fn metadata(&self) -> Option<&'static VendorMetadata> {
        Some(&METADATA)
    }
    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap {
        OpenAiDefault.auth_headers(ctx)
    }
    fn build_url(&self, ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String {
        OpenAiDefault.build_url(ctx, base_url, path)
    }
}

inventory::submit! {
    VendorRegistration { make: || Box::new(MinimaxVendor) }
}
