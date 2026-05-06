//! Google vendor (Gemini direct API).

pub mod gemini_cli;

use async_trait::async_trait;
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
    id: "google",
    label: Label { zh: "Google", en: "Google" },
    icon: "google",
    default_protocol: "gemini",
    channels: &[ChannelDef {
        id: "default",
        label: Label { zh: "默认", en: "Default" },
        base_urls: &[
            ProtocolBaseUrl {
                protocol: "openai",
                base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
            },
            ProtocolBaseUrl {
                protocol: "gemini",
                base_url: "https://generativelanguage.googleapis.com",
            },
        ],
        api_key: None,
        models_source: Some(
            "https://generativelanguage.googleapis.com/v1beta/openai/models",
        ),
        capabilities_source: Some("ai://models.dev/google"),
        static_models: &[],
        auth_mode: AuthMode::ApiKey,
        oauth: None,
        runtime: None,
    }],
};

pub struct GoogleVendor;

impl VendorExtension for GoogleVendor {
    fn scope(&self) -> VendorScope {
        VendorScope::Vendor { vendor_id: "google" }
    }
    fn metadata(&self) -> Option<&'static VendorMetadata> {
        Some(&METADATA)
    }
    fn build_url(&self, ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String {
        let url = format!("{}{path}", base_url.trim_end_matches('/'));
        if url.contains('?') {
            format!("{url}&key={}", ctx.api_key)
        } else {
            format!("{url}?key={}", ctx.api_key)
        }
    }
}

#[async_trait]
impl ProviderAdapter for GoogleVendor {
    fn vendor_id(&self) -> &'static str {
        "google"
    }
    fn supported_protocols(&self) -> &'static [ProtocolId] {
        use crate::protocol::ids::GOOGLE_GENERATE_V1BETA;
        &[GOOGLE_GENERATE_V1BETA]
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
        GatewayError::upstream_status("google", status, Some(msg))
    }
}

inventory::submit! {
    VendorRegistration { make: || Box::new(GoogleVendor) }
}

inventory::submit! {
    ProviderAdapterRegistration { make: || Box::new(GoogleVendor) }
}

/// Family-level fallback for providers with blank/unknown vendor on Google-family protocols.
pub struct GoogleFamilyExt;

impl VendorExtension for GoogleFamilyExt {
    fn scope(&self) -> VendorScope {
        VendorScope::Family(ProtocolFamily::Google)
    }
    fn metadata(&self) -> Option<&'static VendorMetadata> {
        None
    }
    fn build_url(&self, ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String {
        let url = format!("{}{path}", base_url.trim_end_matches('/'));
        if url.contains('?') {
            format!("{url}&key={}", ctx.api_key)
        } else {
            format!("{url}?key={}", ctx.api_key)
        }
    }
}

inventory::submit! {
    VendorRegistration { make: || Box::new(GoogleFamilyExt) }
}
