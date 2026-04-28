//! Google vendor (Gemini direct API). Reuses the Google family default
//! for `?key=` query-string auth.
//!
//! `gemini-cli` is a planned OAuth channel — see the placeholder
//! submodule (`gemini_cli`) which is intentionally not registered
//! until its OAuth driver lands.

pub mod gemini_cli;

use reqwest::header::HeaderMap;

use crate::protocol::vendor::defaults::GoogleDefault;
use crate::protocol::vendor::types::{
    AuthMode, ChannelDef, Label, ProtocolBaseUrl, VendorMetadata,
};
use crate::protocol::vendor::{VendorCtx, VendorExtension, VendorRegistration, VendorScope};

const METADATA: VendorMetadata = VendorMetadata {
    id: "google",
    label: Label {
        zh: "Google",
        en: "Google",
    },
    icon: "google",
    default_protocol: "gemini",
    channels: &[ChannelDef {
        id: "default",
        label: Label {
            zh: "默认",
            en: "Default",
        },
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
        models_source: Some("https://generativelanguage.googleapis.com/v1beta/openai/models"),
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
        VendorScope::Vendor {
            vendor_id: "google",
        }
    }

    fn metadata(&self) -> Option<&'static VendorMetadata> {
        Some(&METADATA)
    }

    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap {
        GoogleDefault.auth_headers(ctx)
    }

    fn build_url(&self, ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String {
        GoogleDefault.build_url(ctx, base_url, path)
    }
}

inventory::submit! {
    VendorRegistration { make: || Box::new(GoogleVendor) }
}
