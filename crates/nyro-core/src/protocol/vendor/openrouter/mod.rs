//! OpenRouter vendor (OpenAI-compatible aggregator).

use reqwest::header::HeaderMap;

use crate::protocol::vendor::defaults::OpenAiDefault;
use crate::protocol::vendor::types::{
    AuthMode, ChannelDef, Label, ProtocolBaseUrl, VendorMetadata,
};
use crate::protocol::vendor::{VendorCtx, VendorExtension, VendorRegistration, VendorScope};

const METADATA: VendorMetadata = VendorMetadata {
    id: "openrouter",
    label: Label { zh: "OpenRouter", en: "OpenRouter" },
    icon: "openrouter",
    default_protocol: "openai",
    channels: &[ChannelDef {
        id: "default",
        label: Label { zh: "默认", en: "Default" },
        base_urls: &[
            ProtocolBaseUrl {
                protocol: "openai",
                base_url: "https://openrouter.ai/api/v1",
            },
            ProtocolBaseUrl {
                protocol: "anthropic",
                base_url: "https://openrouter.ai/api",
            },
        ],
        api_key: None,
        models_source: Some("https://openrouter.ai/api/v1/models"),
        capabilities_source: Some("https://openrouter.ai/api/v1/models"),
        static_models: &[],
        auth_mode: AuthMode::ApiKey,
        oauth: None,
        runtime: None,
    }],
};

pub struct OpenrouterVendor;

impl VendorExtension for OpenrouterVendor {
    fn scope(&self) -> VendorScope {
        VendorScope::Vendor { vendor_id: "openrouter" }
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
    VendorRegistration { make: || Box::new(OpenrouterVendor) }
}
