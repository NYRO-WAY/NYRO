//! Nyro vendor preset — the user-defined / "Custom" template.
//!
//! Behaviour delegates to the OpenAI family default; metadata exists
//! so the WebUI can render the "Nyro (Custom)" preset card. Renamed
//! from `custom` in PR2B to align with the product brand; the legacy
//! values `custom`/`preset_key='custom'` are migrated to `nyro` in
//! `db::migrate` for SQLite and the Postgres bootstrap.

use reqwest::header::HeaderMap;

use crate::protocol::vendor::defaults::OpenAiDefault;
use crate::protocol::vendor::types::{AuthMode, ChannelDef, Label, VendorMetadata};
use crate::protocol::vendor::{VendorCtx, VendorExtension, VendorRegistration, VendorScope};

const METADATA: VendorMetadata = VendorMetadata {
    id: "nyro",
    label: Label { zh: "自定义", en: "Custom" },
    icon: "nyro",
    default_protocol: "openai",
    channels: &[ChannelDef {
        id: "default",
        label: Label { zh: "默认", en: "Default" },
        base_urls: &[],
        api_key: None,
        models_source: None,
        capabilities_source: Some("ai://models.dev/"),
        static_models: &[],
        auth_mode: AuthMode::ApiKey,
        oauth: None,
        runtime: None,
    }],
};

pub struct NyroVendor;

impl VendorExtension for NyroVendor {
    fn scope(&self) -> VendorScope {
        VendorScope::Vendor { vendor_id: "nyro" }
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
}

inventory::submit! {
    VendorRegistration { make: || Box::new(NyroVendor) }
}
