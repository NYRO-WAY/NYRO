//! Moonshot AI vendor (OpenAI-compatible). Two channels: global
//! (`default`) and China site (`china`) — the only difference is the
//! base URLs.

use reqwest::header::HeaderMap;

use crate::protocol::vendor::defaults::OpenAiDefault;
use crate::protocol::vendor::types::{
    AuthMode, ChannelDef, Label, ProtocolBaseUrl, VendorMetadata,
};
use crate::protocol::vendor::{VendorCtx, VendorExtension, VendorRegistration, VendorScope};

const METADATA: VendorMetadata = VendorMetadata {
    id: "moonshotai",
    label: Label { zh: "Moonshot AI", en: "Moonshot AI" },
    icon: "kimi",
    default_protocol: "openai",
    channels: &[
        ChannelDef {
            id: "default",
            label: Label { zh: "默认", en: "Default" },
            base_urls: &[
                ProtocolBaseUrl {
                    protocol: "openai",
                    base_url: "https://api.moonshot.ai/v1",
                },
                ProtocolBaseUrl {
                    protocol: "anthropic",
                    base_url: "https://api.moonshot.ai/anthropic",
                },
            ],
            api_key: None,
            models_source: Some("https://api.moonshot.ai/v1/models"),
            capabilities_source: Some("ai://models.dev/moonshotai"),
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
                    base_url: "https://api.moonshot.cn/v1",
                },
                ProtocolBaseUrl {
                    protocol: "anthropic",
                    base_url: "https://api.moonshot.cn/anthropic",
                },
            ],
            api_key: None,
            models_source: Some("https://api.moonshot.cn/v1/models"),
            capabilities_source: Some("ai://models.dev/moonshotai"),
            static_models: &[],
            auth_mode: AuthMode::ApiKey,
            oauth: None,
            runtime: None,
        },
    ],
};

pub struct MoonshotaiVendor;

impl VendorExtension for MoonshotaiVendor {
    fn scope(&self) -> VendorScope {
        VendorScope::Vendor { vendor_id: "moonshotai" }
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
    VendorRegistration { make: || Box::new(MoonshotaiVendor) }
}
