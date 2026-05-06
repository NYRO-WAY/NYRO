//! Z.ai vendor (OpenAI-compatible).

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
    id: "zai",
    label: Label { zh: "Z.ai", en: "Z.ai" },
    icon: "zai",
    default_protocol: "openai",
    channels: &[
        ChannelDef {
            id: "default",
            label: Label { zh: "默认", en: "Default" },
            base_urls: &[
                ProtocolBaseUrl { protocol: "openai", base_url: "https://api.z.ai/api/paas/v4" },
                ProtocolBaseUrl { protocol: "anthropic", base_url: "https://api.z.ai/api/anthropic" },
            ],
            api_key: None,
            models_source: Some("https://api.z.ai/api/paas/v4/models"),
            capabilities_source: Some("ai://models.dev/zai"),
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
                    base_url: "https://api.z.ai/api/coding/paas/v4",
                },
                ProtocolBaseUrl { protocol: "anthropic", base_url: "https://api.z.ai/api/anthropic" },
            ],
            api_key: None,
            models_source: Some("https://api.z.ai/api/coding/paas/v4/models"),
            capabilities_source: Some("ai://models.dev/zai"),
            static_models: &[],
            auth_mode: AuthMode::ApiKey,
            oauth: None,
            runtime: None,
        },
    ],
};

pub struct ZaiVendor;

impl VendorExtension for ZaiVendor {
    fn scope(&self) -> VendorScope { VendorScope::Vendor { vendor_id: "zai" } }
    fn metadata(&self) -> Option<&'static VendorMetadata> { Some(&METADATA) }
    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap { openai_bearer_auth_headers(ctx) }
    fn build_url(&self, _ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String { openai_build_url(base_url, path) }
}

#[async_trait]
impl ProviderAdapter for ZaiVendor {
    fn vendor_id(&self) -> &'static str { "zai" }
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
    fn map_error(&self, status: u16, body: Value) -> GatewayError { openai_map_error("zai", status, body) }
}

inventory::submit! { VendorRegistration { make: || Box::new(ZaiVendor) } }
inventory::submit! { ProviderAdapterRegistration { make: || Box::new(ZaiVendor) } }
