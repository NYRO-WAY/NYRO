//! Anthropic vendor (direct API).

pub mod claude_code;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;

use crate::error::GatewayError;
use crate::protocol::ids::ProtocolId;
use crate::protocol::types::{InternalRequest, InternalResponse};
use crate::provider::adapter::{ProviderAdapter, ProviderCtx};
use crate::provider::common::openai::{
    openai_compat_build_request, openai_compat_parse_response, openai_compat_stream_parser,
};
use crate::provider::inbound::InboundResponse;
use crate::provider::metadata::{AuthMode, ChannelDef, Label, ProtocolBaseUrl, VendorMetadata};
use crate::provider::outbound::OutboundRequest;
use crate::protocol::ids::ProtocolFamily;
use crate::provider::registry::{ProviderAdapterRegistration, VendorRegistration, VendorScope};
use crate::provider::stream::ProviderStreamParser;
use crate::provider::vendor_ext::{VendorCtx, VendorExtension};

const METADATA: VendorMetadata = VendorMetadata {
    id: "anthropic",
    label: Label { zh: "Anthropic", en: "Anthropic" },
    icon: "anthropic",
    default_protocol: "anthropic",
    channels: &[ChannelDef {
        id: "default",
        label: Label { zh: "默认", en: "Default" },
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
        VendorScope::Vendor { vendor_id: "anthropic" }
    }
    fn metadata(&self) -> Option<&'static VendorMetadata> {
        Some(&METADATA)
    }
    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Ok(v) = HeaderValue::from_str(ctx.api_key) {
            h.insert("x-api-key", v);
        }
        h.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        h
    }
}

#[async_trait]
impl ProviderAdapter for AnthropicVendor {
    fn vendor_id(&self) -> &'static str {
        "anthropic"
    }
    fn supported_protocols(&self) -> &'static [ProtocolId] {
        use crate::protocol::ids::ANTHROPIC_MESSAGES_2023_06_01;
        &[ANTHROPIC_MESSAGES_2023_06_01]
    }
    async fn build_request(
        &self,
        req: &mut InternalRequest,
        ctx: &ProviderCtx<'_>,
    ) -> Result<OutboundRequest, GatewayError> {
        openai_compat_build_request(self, req, ctx).await
    }
    async fn parse_response(
        &self,
        resp: InboundResponse,
        ctx: &ProviderCtx<'_>,
    ) -> Result<InternalResponse, GatewayError> {
        openai_compat_parse_response(self, resp, ctx).await
    }
    fn stream_parser(&self, ctx: &ProviderCtx<'_>) -> Box<dyn ProviderStreamParser + Send> {
        openai_compat_stream_parser(ctx)
    }
    fn map_error(&self, status: u16, body: Value) -> GatewayError {
        let msg = body
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("upstream HTTP {status}"));
        GatewayError::upstream_status("anthropic", status, Some(msg))
    }
}

inventory::submit! {
    VendorRegistration { make: || Box::new(AnthropicVendor) }
}

inventory::submit! {
    ProviderAdapterRegistration { make: || Box::new(AnthropicVendor) }
}

/// Family-level fallback for providers with blank/unknown vendor on Anthropic-family protocols.
pub struct AnthropicFamilyExt;

impl VendorExtension for AnthropicFamilyExt {
    fn scope(&self) -> VendorScope {
        VendorScope::Family(ProtocolFamily::Anthropic)
    }
    fn metadata(&self) -> Option<&'static VendorMetadata> {
        None
    }
    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Ok(v) = HeaderValue::from_str(ctx.api_key) {
            h.insert("x-api-key", v);
        }
        h.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        h
    }
}

inventory::submit! {
    VendorRegistration { make: || Box::new(AnthropicFamilyExt) }
}
