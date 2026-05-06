//! Zhipu AI vendor (OpenAI-compatible).

use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::error::GatewayError;
use crate::protocol::ids::ProtocolId;
use crate::protocol::types::{InternalRequest, InternalResponse};
use crate::provider::adapter::{ProviderAdapter, ProviderCtx};
use crate::provider::common::openai::{
    openai_bearer_auth_headers, openai_build_url, openai_compat_build_request,
    openai_compat_parse_response, openai_compat_stream_parser, openai_map_error,
};
use crate::provider::inbound::InboundResponse;
use crate::provider::metadata::{AuthMode, ChannelDef, Label, ProtocolBaseUrl, VendorMetadata};
use crate::provider::outbound::OutboundRequest;
use crate::provider::registry::{ProviderAdapterRegistration, VendorRegistration, VendorScope};
use crate::provider::stream::ProviderStreamParser;
use crate::provider::vendor_ext::{VendorCtx, VendorExtension};

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
    fn scope(&self) -> VendorScope { VendorScope::Vendor { vendor_id: "zhipuai" } }
    fn metadata(&self) -> Option<&'static VendorMetadata> { Some(&METADATA) }
    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap { openai_bearer_auth_headers(ctx) }
    fn build_url(&self, _ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String { openai_build_url(base_url, path) }
}

#[async_trait]
impl ProviderAdapter for ZhipuaiVendor {
    fn vendor_id(&self) -> &'static str { "zhipuai" }
    fn supported_protocols(&self) -> &'static [ProtocolId] {
        use crate::protocol::ids::OPENAI_CHAT_V1;
        &[OPENAI_CHAT_V1]
    }
    async fn build_request(&self, req: &mut InternalRequest, ctx: &ProviderCtx<'_>) -> Result<OutboundRequest, GatewayError> {
        openai_compat_build_request(self, req, ctx).await
    }
    async fn parse_response(&self, resp: InboundResponse, ctx: &ProviderCtx<'_>) -> Result<InternalResponse, GatewayError> {
        openai_compat_parse_response(self, resp, ctx).await
    }
    fn stream_parser(&self, ctx: &ProviderCtx<'_>) -> Box<dyn ProviderStreamParser + Send> { openai_compat_stream_parser(ctx) }
    fn map_error(&self, status: u16, body: Value) -> GatewayError { openai_map_error("zhipuai", status, body) }
}

inventory::submit! { VendorRegistration { make: || Box::new(ZhipuaiVendor) } }
inventory::submit! { ProviderAdapterRegistration { make: || Box::new(ZhipuaiVendor) } }
