//! Ollama vendor extension. Auth/build_url reuse the OpenAI-compat
//! defaults; the vendor-specific behaviour is the `pre_request` hook
//! that probes `/api/show` and strips tool definitions when the model
//! does not support tools.

mod capabilities;

use async_trait::async_trait;
use reqwest::header::HeaderMap;

use crate::Gateway;
use crate::protocol::types::InternalRequest;
use crate::protocol::vendor::defaults::OpenAiDefault;
use crate::protocol::vendor::types::{
    AuthMode, ChannelDef, Label, ProtocolBaseUrl, VendorMetadata,
};
use crate::protocol::vendor::{VendorCtx, VendorExtension, VendorRegistration, VendorScope};

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
    fn scope(&self) -> VendorScope {
        VendorScope::Vendor { vendor_id: "ollama" }
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

inventory::submit! {
    VendorRegistration { make: || Box::new(OllamaVendor) }
}
