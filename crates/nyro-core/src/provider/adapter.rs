//! `ProviderAdapter` — the top-level orchestration trait for a vendor.
//!
//! Each vendor implementation:
//!   1. Holds a `VendorExtension` impl (for hooks) in the same struct.
//!   2. Delegates codec work to `ctx.protocol.handler()`.
//!   3. Maps upstream errors to `GatewayError`.
//!
//! The dispatcher (PR-16) will call `build_request → transport →
//! parse_response / stream_parser`, replacing the equivalent logic
//! currently spread across `proxy/handler.rs`.

use async_trait::async_trait;

use crate::Gateway;
use crate::auth::types::StoredCredential;
use crate::db::models::Provider;
use crate::error::GatewayError;
use crate::protocol::ids::ProtocolId;
use crate::protocol::types::{InternalRequest, InternalResponse};
use crate::provider::inbound::InboundResponse;
use crate::provider::outbound::OutboundRequest;
use crate::provider::stream::ProviderStreamParser;
use crate::provider::vendor_ext::VendorCtx;

/// Runtime context handed to every `ProviderAdapter` method.
pub struct ProviderCtx<'a> {
    pub provider: &'a Provider,
    /// Resolved egress protocol (from `ProviderProtocols::resolve_egress`).
    pub protocol: ProtocolId,
    /// Resolved egress base URL (from the matched `ProtocolEndpointEntry`).
    pub egress_base_url: &'a str,
    pub api_key: &'a str,
    pub actual_model: &'a str,
    pub credential: Option<&'a StoredCredential>,
    pub gw: &'a Gateway,
}

impl<'a> ProviderCtx<'a> {
    /// Build a `VendorCtx` from this `ProviderCtx` for passing to
    /// `VendorExtension` hooks.
    pub fn to_vendor_ctx(&self) -> VendorCtx<'a> {
        VendorCtx {
            provider: self.provider,
            protocol_id: self.protocol,
            api_key: self.api_key,
            actual_model: self.actual_model,
            credential: self.credential,
        }
    }
}

/// Per-vendor orchestration trait.
///
/// Implementations combine codec + `VendorExtension` hooks to produce a
/// ready-to-send `OutboundRequest` and parse the response back into an
/// `InternalResponse`.
#[async_trait]
pub trait ProviderAdapter: Send + Sync + 'static {
    /// Vendor identifier (matches `Provider.vendor` column).
    fn vendor_id(&self) -> &'static str;

    /// Protocols this adapter can handle as egress (used for registry lookup
    /// validation; not enforced at runtime in PR-15).
    fn supported_protocols(&self) -> &'static [ProtocolId];

    /// Build the outbound request:
    /// `pre_request → normalize_tool_results → pre_encode →
    ///  codec_encode → post_encode → auth_headers → build_url`.
    async fn build_request(
        &self,
        req: &mut InternalRequest,
        ctx: &ProviderCtx<'_>,
    ) -> Result<OutboundRequest, GatewayError>;

    /// Parse a non-streaming response:
    /// `pre_parse → codec_parse → normalize_reasoning → post_parse`.
    async fn parse_response(
        &self,
        resp: InboundResponse,
        ctx: &ProviderCtx<'_>,
    ) -> Result<InternalResponse, GatewayError>;

    /// Return a stream parser for streaming responses.
    fn stream_parser(&self, ctx: &ProviderCtx<'_>) -> Box<dyn ProviderStreamParser + Send>;

    /// Map a non-2xx HTTP response to a `GatewayError`.
    fn map_error(&self, status: u16, body: serde_json::Value) -> GatewayError;

    /// Validate that the provider row contains the required credentials /
    /// configuration before any request is attempted.
    fn validate_environment(&self, _provider: &Provider) -> Result<(), GatewayError> {
        Ok(())
    }
}
