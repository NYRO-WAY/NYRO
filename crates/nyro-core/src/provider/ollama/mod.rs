//! Ollama vendor extension. The `pre_request` hook probes `/api/show` and
//! strips tool definitions when the model does not support tools.

mod capabilities;

use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::Gateway;
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
    id: "ollama",
    label: Label { zh: "Ollama", en: "Ollama" },
    icon: "ollama",
    default_protocol: "openai",
    channels: &[ChannelDef {
        id: "default",
        label: Label { zh: "默认", en: "Default" },
        base_urls: &[
            ProtocolBaseUrl { protocol: "openai", base_url: "http://127.0.0.1:11434/v1" },
            ProtocolBaseUrl { protocol: "anthropic", base_url: "http://127.0.0.1:11434" },
        ],
        api_key: Some("sk-ollama"),
        models_source: Some("http://127.0.0.1:11434/v1/models"),
        capabilities_source: Some("http://127.0.0.1:11434/api/show"),
        static_models: &[],
        auth_mode: AuthMode::ApiKey,
        oauth: None,
        runtime: None,
    }],
};

pub struct OllamaVendor;

#[async_trait]
impl VendorExtension for OllamaVendor {
    fn scope(&self) -> VendorScope { VendorScope::Vendor { vendor_id: "ollama" } }
    fn metadata(&self) -> Option<&'static VendorMetadata> { Some(&METADATA) }
    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap { openai_bearer_auth_headers(ctx) }
    fn build_url(&self, _ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String { openai_build_url(base_url, path) }

    async fn pre_request(
        &self,
        ctx: &VendorCtx<'_>,
        req: &mut InternalRequest,
        gw: &Gateway,
    ) -> anyhow::Result<()> {
        if req.tools.is_none() && req.tool_choice.is_none() {
            return Ok(());
        }
        let model = ctx.actual_model;
        let caps = match capabilities::get_ollama_capabilities(gw, ctx.provider, model).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    "failed to fetch capabilities for model {model}, skipping tools check: {e}"
                );
                return Ok(());
            }
        };
        let supports_tools = caps.iter().any(|c| c == "tools");
        if !supports_tools {
            tracing::warn!(
                "tools stripped for model {model} (tools not supported, capabilities: {caps:?})"
            );
            req.tools = None;
            req.tool_choice = None;
            req.extra.remove("tools");
            req.extra.remove("tool_choice");
        }
        Ok(())
    }
}

#[async_trait]
impl ProviderAdapter for OllamaVendor {
    fn vendor_id(&self) -> &'static str { "ollama" }
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
    fn map_error(&self, status: u16, body: Value) -> GatewayError { openai_map_error("ollama", status, body) }
}

inventory::submit! { VendorRegistration { make: || Box::new(OllamaVendor) } }
inventory::submit! { ProviderAdapterRegistration { make: || Box::new(OllamaVendor) } }
