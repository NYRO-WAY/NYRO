//! Google (Gemini) family default. Mirrors the legacy `GeminiAdapter`.

use crate::protocol::ids::ProtocolFamily;
use crate::protocol::vendor::{VendorCtx, VendorExtension, VendorRegistration, VendorScope};

pub struct GoogleDefault;

impl VendorExtension for GoogleDefault {
    fn scope(&self) -> VendorScope {
        VendorScope::Family(ProtocolFamily::Google)
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
    VendorRegistration { make: || Box::new(GoogleDefault) }
}
