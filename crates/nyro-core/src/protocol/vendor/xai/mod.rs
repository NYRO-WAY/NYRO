//! xAI vendor (OpenAI-compatible).

use reqwest::header::HeaderMap;

use crate::protocol::vendor::defaults::OpenAiDefault;
use crate::protocol::vendor::types::{
    AuthMode, ChannelDef, Label, ProtocolBaseUrl, VendorMetadata,
};
use crate::protocol::vendor::{VendorCtx, VendorExtension, VendorRegistration, VendorScope};

const METADATA: VendorMetadata = VendorMetadata {
    id: "xai",
    label: Label { zh: "xAI", en: "xAI" },
    icon: "xai",
    default_protocol: "openai",
    channels: &[ChannelDef {
        id: "default",
        label: Label { zh: "默认", en: "Default" },
        base_urls: &[ProtocolBaseUrl {
            protocol: "openai",
            base_url: "https://api.x.ai/v1",
        }],
        api_key: None,
        models_source: Some("https://api.x.ai/v1/models"),
        capabilities_source: Some("ai://models.dev/xai"),
        static_models: &[],
        auth_mode: AuthMode::ApiKey,
        oauth: None,
        runtime: None,
    }],
};

pub struct XaiVendor;

impl VendorExtension for XaiVendor {
    fn scope(&self) -> VendorScope {
        VendorScope::Vendor { vendor_id: "xai" }
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
    VendorRegistration { make: || Box::new(XaiVendor) }
}
