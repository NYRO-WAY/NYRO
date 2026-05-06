//! `VendorExtension` trait — per-vendor hook points for the request/response
//! pipeline.
//!
//! Moved from `protocol/vendor/mod.rs` (PR-15). All vendor implementations
//! that previously lived under `protocol/vendor/<vendor>/` now live under
//! `provider/<vendor>/` and still register via `inventory::submit!`.
//!
//! ## Hook surface (9 hooks)
//!
//! - `auth_headers` / `build_url` — synchronous.
//! - `pre_encode` / `post_encode` — mutate `InternalRequest` / body.
//! - `pre_parse` / `post_parse` — mutate upstream JSON / `InternalResponse`.
//! - `on_stream_raw_chunk` / `on_stream_delta` — stream normalization.
//! - `pre_request` — async pre-flight (capability probing, model rewrites).

use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::Gateway;
use crate::auth::types::StoredCredential;
use crate::db::models::Provider;
use crate::protocol::ids::ProtocolId;
use crate::protocol::types::{InternalRequest, InternalResponse, StreamDelta};
use crate::provider::registry::VendorScope;

/// Runtime context handed to every `VendorExtension` hook.
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
    fn metadata(&self) -> Option<&'static crate::provider::metadata::VendorMetadata> {
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
