//! Vendor extension layer.
//!
//! `VendorExtension` is the per-vendor (and optionally per-channel) hook
//! point that replaces the old `ProviderAdapter` trait. Where the old
//! adapter dispatched on a `Protocol` enum, vendor extensions dispatch
//! on `(provider.vendor, provider.channel)` with a fallback to the
//! protocol family. Three layers, resolved in priority order:
//!
//! 1. **Channel** — `VendorScope::Channel { vendor_id, channel_id }`
//!    (e.g. `openai/codex`).
//! 2. **Vendor** — `VendorScope::Vendor { vendor_id }` (e.g. `ollama`).
//! 3. **Family** — `VendorScope::Family(ProtocolFamily::OpenAI)` —
//!    last-resort defaults that match any provider whose protocol
//!    belongs to the family.
//!
//! Extensions register themselves with `inventory::submit!` from their
//! own module, so adding a new vendor is purely additive — no central
//! match block to edit.
//!
//! ## Hook surface (9 hooks)
//!
//! - `auth_headers` / `build_url` — synchronous, replaces the old
//!   `ProviderAdapter` surface.
//! - `pre_encode` / `post_encode` — mutate `InternalRequest` and the
//!   serialized body just before/after encoding.
//! - `pre_parse` / `post_parse` — mutate the upstream JSON and decoded
//!   `InternalResponse`.
//! - `on_stream_raw_chunk` / `on_stream_delta` — vendor-specific stream
//!   normalization at raw-text and decoded-event granularity.
//! - `pre_request` — async hook for capability probing (Ollama) or
//!   model-alias rewrites that must happen before encoding.
//!
//! All hooks have no-op defaults so vendors only override what they
//! need.

pub mod defaults;
pub mod registry;
pub mod types;

// ── Phase 1: vendors backed by `assets/providers.json` (PR2A + PR2B) ──
pub mod anthropic;
pub mod deepseek;
pub mod google;
pub mod minimax;
pub mod moonshotai;
pub mod nvidia;
pub mod nyro;
pub mod ollama;
pub mod openai;
pub mod openrouter;
pub mod xai;
pub mod zai;
pub mod zhipuai;

// ── Phase 2: placeholders. Modules exist to reserve the layout but
//    are not registered with `inventory` until their auth/runtime
//    implementations land in follow-up PRs. ──
pub mod aws_bedrock;
pub mod azure_foundry;
pub mod google_vertex;

use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::Gateway;
use crate::auth::types::StoredCredential;
use crate::db::models::Provider;
use crate::protocol::ids::ProtocolId;
use crate::protocol::types::{InternalRequest, InternalResponse, StreamDelta};

pub use registry::{VendorRegistration, VendorRegistry, VendorScope};
pub use types::{
    AuthMode, ChannelDef, Label, OAuthConfig, ProtocolBaseUrl, RuntimeConfig, VendorMetadata,
};

/// Runtime context handed to every hook.
pub struct VendorCtx<'a> {
    pub provider: &'a Provider,
    pub protocol_id: ProtocolId,
    pub api_key: &'a str,
    pub actual_model: &'a str,
    pub credential: Option<&'a StoredCredential>,
}

/// Per-vendor / per-channel extension. Implementations register via
/// `inventory::submit!` from their own module.
#[async_trait]
pub trait VendorExtension: Send + Sync + 'static {
    /// Identifies which provider rows this extension applies to.
    fn scope(&self) -> VendorScope;

    /// Static metadata for the WebUI / preset list. Channel-scoped
    /// extensions return `None` because their data is folded into the
    /// vendor-scoped `VendorMetadata`.
    fn metadata(&self) -> Option<&'static VendorMetadata> {
        None
    }

    fn auth_headers(&self, _ctx: &VendorCtx<'_>) -> HeaderMap {
        HeaderMap::new()
    }

    fn build_url(&self, _ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String {
        format!("{}{}", base_url.trim_end_matches('/'), path)
    }

    async fn pre_encode(
        &self,
        _ctx: &VendorCtx<'_>,
        _req: &mut InternalRequest,
    ) -> anyhow::Result<()> {
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

    async fn post_parse(
        &self,
        _ctx: &VendorCtx<'_>,
        _resp: &mut InternalResponse,
    ) -> anyhow::Result<()> {
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
        _delta: &mut StreamDelta,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    /// Async pre-flight hook. Used by Ollama to probe `/api/show` and
    /// strip tool definitions when the model lacks tool support.
    async fn pre_request(
        &self,
        _ctx: &VendorCtx<'_>,
        _req: &mut InternalRequest,
        _gw: &Gateway,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
