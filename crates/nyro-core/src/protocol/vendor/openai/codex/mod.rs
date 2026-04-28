//! OpenAI Codex channel (ChatGPT-backed, OAuth).
//!
//! In the current architecture all codex-specific headers
//! (`chatgpt-account-id`, `Authorization: Bearer <oauth_token>` …) are
//! produced by `OpenAIOAuthDriver::bind_runtime` and injected through
//! `RuntimeBinding.extra_headers`. This channel extension therefore
//! exists primarily so the three-tier resolver can dispatch on
//! `(vendor=openai, channel=codex)` — the actual hook bodies fall back
//! to the OpenAI-compat defaults until PR3 wires the resolver into the
//! request pipeline.
//!
//! Metadata for this channel lives on the parent vendor
//! ([`super::METADATA`](super::OpenAiVendor)) so we return `None` from
//! `metadata()` here; that keeps `VendorRegistry::list_metadata()`
//! one-vendor-one-row.

use reqwest::header::HeaderMap;

use crate::protocol::vendor::defaults::OpenAiDefault;
use crate::protocol::vendor::{VendorCtx, VendorExtension, VendorRegistration, VendorScope};

pub struct OpenAiCodexChannel;

impl VendorExtension for OpenAiCodexChannel {
    fn scope(&self) -> VendorScope {
        VendorScope::Channel {
            vendor_id: "openai",
            channel_id: "codex",
        }
    }

    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap {
        OpenAiDefault.auth_headers(ctx)
    }

    fn build_url(&self, ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String {
        OpenAiDefault.build_url(ctx, base_url, path)
    }
}

inventory::submit! {
    VendorRegistration { make: || Box::new(OpenAiCodexChannel) }
}
