//! Custom vendor preset — user-defined "Bring Your Own Endpoint".

use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::error::GatewayError;
use crate::protocol::ids::ProtocolId;
use crate::protocol::types::{InternalRequest, InternalResponse};
use crate::provider::adapter::{ProviderAdapter, ProviderCtx};
use crate::provider::common::openai::{
    openai_compat_build_request, openai_compat_parse_response, openai_compat_stream_parser,
    openai_map_error, GenericOpenAICompatibleAdapter,
};
use crate::provider::inbound::InboundResponse;
use crate::provider::metadata::{AuthMode, ChannelDef, Label, VendorMetadata};
use crate::provider::outbound::OutboundRequest;
use crate::provider::registry::{ProviderAdapterRegistration, VendorRegistration, VendorScope};
use crate::provider::stream::ProviderStreamParser;
use crate::provider::vendor_ext::{VendorCtx, VendorExtension};

const METADATA: VendorMetadata = VendorMetadata {
    id: "custom",
    label: Label { zh: "自定义", en: "Custom" },
    icon: "custom",
    default_protocol: "openai",
    channels: &[ChannelDef {
        id: "default",
        label: Label { zh: "默认", en: "Default" },
        base_urls: &[],
        api_key: None,
        models_source: None,
        capabilities_source: Some("ai://models.dev/"),
        static_models: &[],
        auth_mode: AuthMode::ApiKey,
        oauth: None,
        runtime: None,
    }],
};

pub struct CustomVendor;

impl VendorExtension for CustomVendor {
    fn scope(&self) -> VendorScope { VendorScope::Vendor { vendor_id: "custom" } }
    fn metadata(&self) -> Option<&'static VendorMetadata> { Some(&METADATA) }
    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap {
        GenericOpenAICompatibleAdapter.auth_headers(ctx)
    }
    fn build_url(&self, ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String {
        GenericOpenAICompatibleAdapter.build_url(ctx, base_url, path)
    }
}

#[async_trait]
impl ProviderAdapter for CustomVendor {
    fn vendor_id(&self) -> &'static str { "custom" }
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
    fn map_error(&self, status: u16, body: Value) -> GatewayError { openai_map_error("custom", status, body) }
}

inventory::submit! { VendorRegistration { make: || Box::new(CustomVendor) } }
inventory::submit! { ProviderAdapterRegistration { make: || Box::new(CustomVendor) } }
