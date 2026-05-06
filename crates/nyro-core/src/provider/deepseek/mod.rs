//! DeepSeek vendor (OpenAI-compatible).

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
    id: "deepseek",
    label: Label { zh: "DeepSeek", en: "DeepSeek" },
    icon: "deepseek",
    default_protocol: "openai",
    channels: &[ChannelDef {
        id: "default",
        label: Label { zh: "默认", en: "Default" },
        base_urls: &[
            ProtocolBaseUrl { protocol: "openai", base_url: "https://api.deepseek.com/v1" },
            ProtocolBaseUrl {
                protocol: "anthropic",
                base_url: "https://api.deepseek.com/anthropic",
            },
        ],
        api_key: None,
        models_source: Some("https://api.deepseek.com/v1/models"),
        capabilities_source: Some("ai://models.dev/deepseek"),
        static_models: &[],
        auth_mode: AuthMode::ApiKey,
        oauth: None,
        runtime: None,
    }],
};

pub struct DeepseekVendor;

impl VendorExtension for DeepseekVendor {
    fn scope(&self) -> VendorScope {
        VendorScope::Vendor { vendor_id: "deepseek" }
    }
    fn metadata(&self) -> Option<&'static VendorMetadata> {
        Some(&METADATA)
    }
    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap {
        openai_bearer_auth_headers(ctx)
    }
    fn build_url(&self, _ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String {
        openai_build_url(base_url, path)
    }
}

#[async_trait]
impl ProviderAdapter for DeepseekVendor {
    fn vendor_id(&self) -> &'static str { "deepseek" }
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
    fn stream_parser(&self, ctx: &ProviderCtx<'_>) -> Box<dyn ProviderStreamParser + Send> {
        openai_compat_stream_parser(ctx)
    }
    fn map_error(&self, status: u16, body: Value) -> GatewayError {
        openai_map_error("deepseek", status, body)
    }
}

inventory::submit! { VendorRegistration { make: || Box::new(DeepseekVendor) } }
inventory::submit! { ProviderAdapterRegistration { make: || Box::new(DeepseekVendor) } }
