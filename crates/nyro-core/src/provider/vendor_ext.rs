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
use crate::protocol::ir::{AiRequest, AiResponse, AiStreamDelta};
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

    /// Async pre-flight hook. Used by Ollama to probe `/api/show` and
    /// strip tool definitions when the model lacks tool support.
    async fn pre_request(
        &self,
        _ctx: &VendorCtx<'_>,
        _req: &mut AiRequest,
        _gw: &Gateway,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

// ── Blanket impl: Vendor → VendorExtension ────────────────────────────────────

/// Any type that implements [`Vendor`] automatically satisfies
/// `VendorExtension`.  This lets pipeline free-functions keep their
/// `V: VendorExtension` bound without change.
#[async_trait]
impl<T: crate::provider::vendor::Vendor> VendorExtension for T {
    fn scope(&self) -> VendorScope {
        crate::provider::vendor::Vendor::scope(self)
    }
    fn metadata(&self) -> Option<&'static crate::provider::metadata::VendorMetadata> {
        crate::provider::vendor::Vendor::metadata(self)
    }
    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap {
        crate::provider::vendor::Vendor::auth_headers(self, ctx)
    }
    fn build_url(&self, ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String {
        crate::provider::vendor::Vendor::build_url(self, ctx, base_url, path)
    }
    async fn pre_encode(&self, ctx: &VendorCtx<'_>, req: &mut AiRequest) -> anyhow::Result<()> {
        crate::provider::vendor::Vendor::pre_encode(self, ctx, req).await
    }
    async fn post_encode(
        &self,
        ctx: &VendorCtx<'_>,
        body: &mut serde_json::Value,
        headers: &mut HeaderMap,
    ) -> anyhow::Result<()> {
        crate::provider::vendor::Vendor::post_encode(self, ctx, body, headers).await
    }
    async fn pre_parse(
        &self,
        ctx: &VendorCtx<'_>,
        resp: &mut serde_json::Value,
    ) -> anyhow::Result<()> {
        crate::provider::vendor::Vendor::pre_parse(self, ctx, resp).await
    }
    async fn post_parse(&self, ctx: &VendorCtx<'_>, resp: &mut AiResponse) -> anyhow::Result<()> {
        crate::provider::vendor::Vendor::post_parse(self, ctx, resp).await
    }
    async fn on_stream_raw_chunk(
        &self,
        ctx: &VendorCtx<'_>,
        chunk: &mut String,
    ) -> anyhow::Result<()> {
        crate::provider::vendor::Vendor::on_stream_raw_chunk(self, ctx, chunk).await
    }
    async fn on_stream_delta(
        &self,
        ctx: &VendorCtx<'_>,
        delta: &mut AiStreamDelta,
    ) -> anyhow::Result<()> {
        crate::provider::vendor::Vendor::on_stream_delta(self, ctx, delta).await
    }
    async fn pre_request(
        &self,
        ctx: &VendorCtx<'_>,
        req: &mut AiRequest,
        gw: &Gateway,
    ) -> anyhow::Result<()> {
        crate::provider::vendor::Vendor::pre_request(self, ctx, req, gw).await
    }
}

// ── VendorAsExt ───────────────────────────────────────────────────────────────

/// Thin proxy: presents the `VendorExtension` interface over an
/// `Arc<dyn Vendor>`.  Used by the registry's `resolve()` method so it can
/// return `Arc<dyn VendorExtension>` for both extension-only registrations
/// and full `Vendor` registrations without allocating on every call.
pub(crate) struct VendorAsExt(pub Arc<dyn crate::provider::vendor::Vendor>);

use std::sync::Arc;

#[async_trait]
impl VendorExtension for VendorAsExt {
    fn scope(&self) -> VendorScope {
        self.0.scope()
    }
    fn metadata(&self) -> Option<&'static crate::provider::metadata::VendorMetadata> {
        self.0.metadata()
    }
    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap {
        self.0.auth_headers(ctx)
    }
    fn build_url(&self, ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String {
        self.0.build_url(ctx, base_url, path)
    }
    // All async hooks use VendorExtension defaults (Ok(())).
    // VendorAsExt is only used for admin-path sync lookups.
}
