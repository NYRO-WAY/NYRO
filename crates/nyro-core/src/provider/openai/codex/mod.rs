//! OpenAI Codex channel (ChatGPT-backed, OAuth).

use reqwest::header::HeaderMap;

use crate::provider::common::openai::{openai_bearer_auth_headers, openai_build_url};
use crate::provider::registry::{ExtensionRegistration, VendorScope};
use crate::provider::vendor_ext::{VendorCtx, VendorExtension};

pub struct OpenAiCodexChannel;

impl VendorExtension for OpenAiCodexChannel {
    fn scope(&self) -> VendorScope {
        VendorScope::Channel {
            vendor_id: "openai",
            channel_id: "codex",
        }
    }
    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap {
        openai_bearer_auth_headers(ctx)
    }
    fn build_url(&self, _ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String {
        openai_build_url(base_url, path)
    }
}

inventory::submit! {
    ExtensionRegistration { make: || Box::new(OpenAiCodexChannel) }
}
