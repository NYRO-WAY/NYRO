//! Zhipu AI vendor (OpenAI-compatible). Two channels: regular
//! (`default`) and Coding Plan (`coding`).
//!
//! Renamed from `zhipu` in PR2B to match the upstream brand and
//! `models.dev` namespace; legacy `vendor='zhipu'` rows are rewritten
//! to `vendor='zhipuai'` by the storage migrations.

use reqwest::header::HeaderMap;

use crate::protocol::vendor::defaults::OpenAiDefault;
use crate::protocol::vendor::types::{
    AuthMode, ChannelDef, Label, ProtocolBaseUrl, VendorMetadata,
};
use crate::protocol::vendor::{VendorCtx, VendorExtension, VendorRegistration, VendorScope};

const METADATA: VendorMetadata = VendorMetadata {
    id: "zhipuai",
    label: Label { zh: "Zhipu AI", en: "Zhipu AI" },
    icon: "zhipu",
    default_protocol: "openai",
    channels: &[
        ChannelDef {
            id: "default",
            label: Label { zh: "默认", en: "Default" },
            base_urls: &[
                ProtocolBaseUrl {
                    protocol: "openai",
                    base_url: "https://open.bigmodel.cn/api/paas/v4",
                },
                ProtocolBaseUrl {
                    protocol: "anthropic",
                    base_url: "https://open.bigmodel.cn/api/anthropic",
                },
            ],
            api_key: None,
            models_source: Some("https://open.bigmodel.cn/api/paas/v4/models"),
            capabilities_source: Some("ai://models.dev/zhipuai"),
            static_models: &[],
            auth_mode: AuthMode::ApiKey,
            oauth: None,
            runtime: None,
        },
        ChannelDef {
            id: "coding",
            label: Label { zh: "Coding Plan", en: "Coding Plan" },
            base_urls: &[
                ProtocolBaseUrl {
                    protocol: "openai",
                    base_url: "https://open.bigmodel.cn/api/coding/paas/v4",
                },
                ProtocolBaseUrl {
                    protocol: "anthropic",
                    base_url: "https://open.bigmodel.cn/api/anthropic",
                },
            ],
            api_key: None,
            models_source: Some("https://open.bigmodel.cn/api/coding/paas/v4/models"),
            capabilities_source: Some("ai://models.dev/zhipuai"),
            static_models: &[],
            auth_mode: AuthMode::ApiKey,
            oauth: None,
            runtime: None,
        },
    ],
};

pub struct ZhipuaiVendor;

impl VendorExtension for ZhipuaiVendor {
    fn scope(&self) -> VendorScope {
        VendorScope::Vendor { vendor_id: "zhipuai" }
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
    VendorRegistration { make: || Box::new(ZhipuaiVendor) }
}
