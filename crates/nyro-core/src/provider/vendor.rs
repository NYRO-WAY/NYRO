//! Unified `Vendor` trait — merges `VendorExtension` hooks and
//! `ProviderAdapter` orchestration into a single abstraction.
//!
//! # Design
//!
//! Every vendor struct implements `Vendor` once.  The standard 7-step
//! pipeline lives in [`super::common::pipeline`]; vendor impls delegate
//! there via free-function calls:
//!
//! ```rust,ignore
//! use crate::provider::common::pipeline;
//!
//! async fn build_request(&self, req, ctx) -> Result<OutboundRequest> {
//!     pipeline::build_request(self, req, ctx).await
//! }
//! ```
//!
//! Channel-scoped extensions (e.g. `claude-code`, `codex`) keep
//! implementing `VendorExtension` and register via `ExtensionRegistration`.
//!
//! # Where to place field adjustments — layering rules
//!
//! ## Global codec  (`protocol/codec/{anthropic,openai,google}/`)
//!
//! Put logic here when the adjustment applies to **all** vendors that speak
//! the same wire protocol, regardless of who they are:
//! * Normalising enum spellings that differ across protocol versions
//!   (`tool_use` ↔ `tool_calls`, `stop_reason` ↔ `finish_reason`).
//! * Parsing / emitting standard event types defined in the spec
//!   (`content_block_start`, `message_delta`, `text_delta`, …).
//! * Forwarding unknown event types as [`StreamDelta::RawEvent`] so
//!   the downstream client receives them verbatim instead of losing them.
//!
//! ## Vendor hook  (`pre_encode` / `post_encode` / `pre_parse` / `post_parse`)
//!
//! Put logic here when the adjustment is **vendor-specific**, i.e. required
//! because a particular provider deviates from the spec or adds proprietary
//! fields:
//! * Adding / stripping provider-only top-level keys in the request body.
//! * Renaming a field that the provider misnamed relative to the spec.
//! * Injecting a vendor token or signing headers.
//!
//! If the same deviation appears in ≥ 2 unrelated providers, promote it to
//! the global codec instead.
//!
//! ## Decision flowchart
//!
//! ```text
//! Is the field/event defined in the protocol spec?
//!   YES → global codec.
//!   NO  → Is it specific to one vendor?
//!           YES → vendor hook (pre_/post_encode or pre_/post_parse).
//!           NO  → global codec with a feature-flag or a RawEvent fallback.
//! ```
//!
//! ## PassThrough shortcut
//!
//! When a vendor sets `declared_request_mutations() → false` **and**
//! `declared_response_mutations() → false`, and the route resolves to
//! [`ProtocolMode::Native`][crate::proxy::planner::ProtocolMode::Native]
//! (same ingress / egress protocol), the dispatcher skips the entire
//! encode → IR → decode round-trip and forwards bytes verbatim.  This is the
//! correct default for providers whose wire format matches the spec exactly —
//! it avoids any risk of silent data loss from unrecognised fields.

use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::Gateway;
use crate::auth::types::StoredCredential;
use crate::db::models::Provider;
use crate::error::GatewayError;
use crate::protocol::ids::ProtocolId;
use crate::protocol::ir::{AiRequest, AiResponse, AiStreamDelta};
use crate::provider::inbound::InboundResponse;
use crate::provider::metadata::VendorMetadata;
use crate::provider::outbound::OutboundRequest;
use crate::provider::registry::VendorScope;
use crate::provider::stream::ProviderStreamParser;
use crate::provider::vendor_ext::VendorCtx;

// ── ProviderCtx ──────────────────────────────────────────────────────────────

/// Runtime context handed to every [`Vendor`] orchestration method.
pub struct ProviderCtx<'a> {
    pub provider: &'a Provider,
    /// Resolved egress protocol (from `ProviderProtocols::resolve_egress`).
    pub protocol: ProtocolId,
    /// Resolved egress base URL.
    pub egress_base_url: &'a str,
    pub api_key: &'a str,
    pub actual_model: &'a str,
    pub credential: Option<&'a StoredCredential>,
    pub gw: &'a Gateway,
    /// When `true`, the vendor's default `auth_headers` and the Anthropic
    /// Bearer→x-api-key rewrite are suppressed.  Set by OAuth drivers that
    /// inject their own credentials via `RuntimeBinding.extra_headers`.
    pub disable_default_auth: bool,
}

impl<'a> ProviderCtx<'a> {
    /// Build a lightweight `VendorCtx` for passing to extension hooks.
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

// ── Vendor trait ─────────────────────────────────────────────────────────────

/// Unified vendor trait combining extension hooks with request orchestration.
///
/// Any type that implements `Vendor` automatically satisfies
/// [`VendorExtension`][super::vendor_ext::VendorExtension] via a blanket impl
/// in `vendor_ext.rs`, so it can be passed to `pipeline::build_request` and
/// friends without any extra boilerplate.
#[async_trait]
pub trait Vendor: Send + Sync + 'static {
    // ── Scope & metadata ──────────────────────────────────────────────────────

    /// Identifies which provider rows this vendor handles.
    fn scope(&self) -> VendorScope;

    /// Static metadata for the WebUI / preset list.
    fn metadata(&self) -> Option<&'static VendorMetadata> {
        None
    }

    // ── Extension hooks ───────────────────────────────────────────────────────

    fn auth_headers(&self, _ctx: &VendorCtx<'_>) -> HeaderMap {
        HeaderMap::new()
    }

    fn build_url(&self, _ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String {
        format!("{}{}", base_url.trim_end_matches('/'), path)
    }

    async fn pre_encode(&self, _ctx: &VendorCtx<'_>, _req: &mut AiRequest) -> anyhow::Result<()> {
        Ok(())
    }

    async fn post_encode(
        &self,
        _ctx: &VendorCtx<'_>,
        _body: &mut Value,
        _headers: &mut HeaderMap,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn pre_parse(&self, _ctx: &VendorCtx<'_>, _resp: &mut Value) -> anyhow::Result<()> {
        Ok(())
    }

    async fn post_parse(&self, _ctx: &VendorCtx<'_>, _resp: &mut AiResponse) -> anyhow::Result<()> {
        Ok(())
    }

    async fn on_stream_raw_chunk(
        &self,
        _ctx: &VendorCtx<'_>,
        _chunk: &mut String,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn on_stream_delta(
        &self,
        _ctx: &VendorCtx<'_>,
        _delta: &mut AiStreamDelta,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn pre_request(
        &self,
        _ctx: &VendorCtx<'_>,
        _req: &mut AiRequest,
        _gw: &Gateway,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    // ── Orchestration (required) ──────────────────────────────────────────────

    /// Vendor identifier (matches `Provider.vendor` DB column).
    fn vendor_id(&self) -> &'static str;

    /// Protocols this vendor supports as egress.
    fn supported_protocols(&self) -> &'static [ProtocolId];

    /// Build the outbound request via the standard 7-step pipeline.
    async fn build_request(
        &self,
        req: &mut AiRequest,
        ctx: &ProviderCtx<'_>,
    ) -> Result<OutboundRequest, GatewayError>;

    /// Parse a non-streaming upstream response.
    async fn parse_response(
        &self,
        resp: InboundResponse,
        ctx: &ProviderCtx<'_>,
    ) -> Result<AiResponse, GatewayError>;

    /// Return a stream parser for SSE responses.
    fn stream_parser(&self, ctx: &ProviderCtx<'_>) -> Box<dyn ProviderStreamParser + Send>;

    /// Map a non-2xx upstream response to a `GatewayError`.
    fn map_error(&self, status: u16, body: Value) -> GatewayError;

    /// Validate pre-conditions before any request is attempted.
    fn validate_environment(&self, _provider: &Provider) -> Result<(), GatewayError> {
        Ok(())
    }

    // ── PassThrough declarations ───────────────────────────────────────────────

    /// Declares whether this vendor mutates the request body via pipeline hooks
    /// (`pre_request` / `pre_encode` / `post_encode`).
    ///
    /// When `false` **and** the protocol plan resolves to
    /// [`ProtocolMode::Native`][crate::proxy::planner::ProtocolMode::Native]
    /// (ingress == egress), the dispatcher skips the IR round-trip and forwards
    /// the client body verbatim via
    /// [`pipeline::passthrough_run`][super::common::pipeline::passthrough_run].
    ///
    /// Defaults to `true` (conservative). Only override to `false` when
    /// `pre_request`, `pre_encode`, and `post_encode` are all no-ops.
    fn declared_request_mutations(&self) -> bool {
        true
    }

    /// Declares whether this vendor mutates the response via pipeline hooks
    /// (`pre_parse` / `post_parse`).
    ///
    /// When `false` **and** the protocol plan resolves to
    /// [`ProtocolMode::Native`][crate::proxy::planner::ProtocolMode::Native],
    /// the dispatcher returns the upstream JSON verbatim without going through
    /// `parse_response` → IR → `format_response`.
    ///
    /// Defaults to `true` (conservative). Only override to `false` when
    /// `pre_parse` and `post_parse` are both no-ops.
    fn declared_response_mutations(&self) -> bool {
        true
    }
}
