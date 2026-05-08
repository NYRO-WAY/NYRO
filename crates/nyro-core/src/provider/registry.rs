//! Unified vendor registry.
//!
//! Two `inventory` collections are used:
//!
//! * [`VendorRegistration`] — full vendors that implement [`Vendor`].
//!   One per vendor module (no duplicate registration needed).
//! * [`ExtensionRegistration`] — channel-scoped or family-scoped extensions
//!   that implement only [`VendorExtension`] (e.g. `claude-code`, `codex`,
//!   and family fallbacks).
//!
//! The combined [`VendorRegistry`] exposes two resolution strategies:
//! * `get_vendor(vendor_id)` — vendor-id lookup for the proxy pipeline.
//! * `resolve(provider, protocol_id)` — three-tier Channel → Vendor → Family
//!   resolution used by admin/auth paths.

use std::sync::{Arc, OnceLock};

use crate::db::models::Provider;
use crate::protocol::ids::{ProtocolFamily, ProtocolId};
use crate::provider::metadata::VendorMetadata;
use crate::provider::vendor::Vendor;
use crate::provider::vendor_ext::{VendorAsExt, VendorExtension};

// ── VendorScope ───────────────────────────────────────────────────────────────

/// Selector for extension/vendor matching. Resolution order:
/// `Channel` → `Vendor` → `Family`.
#[derive(Debug, Clone, Copy)]
pub enum VendorScope {
    Family(ProtocolFamily),
    Vendor {
        vendor_id: &'static str,
    },
    Channel {
        vendor_id: &'static str,
        channel_id: &'static str,
    },
}

// ── VendorRegistration ────────────────────────────────────────────────────────

/// `inventory` registration record for a full [`Vendor`] implementation.
///
/// Replace the old pair of `VendorRegistration` + `ProviderAdapterRegistration`
/// with a single `inventory::submit!`.
pub struct VendorRegistration {
    pub make: fn() -> Box<dyn Vendor>,
}

inventory::collect!(VendorRegistration);

// ── ExtensionRegistration ─────────────────────────────────────────────────────

/// `inventory` registration record for a channel-scoped or family-scoped
/// [`VendorExtension`] that does NOT implement the full [`Vendor`] trait
/// (e.g. `AnthropicClaudeCodeChannel`, `OpenAIFamilyExt`).
pub struct ExtensionRegistration {
    pub make: fn() -> Box<dyn VendorExtension>,
}

inventory::collect!(ExtensionRegistration);

// ── VendorRegistry ────────────────────────────────────────────────────────────

/// Process-wide vendor registry.  Initialized once on first access.
pub struct VendorRegistry {
    /// Full vendor implementations (vendor-scoped).
    vendors: Vec<Arc<dyn Vendor>>,
    /// Pre-built `VendorAsExt` wrappers for each vendor entry, so
    /// `resolve()` can return `Arc<dyn VendorExtension>` without allocating.
    vendor_as_ext: Vec<Arc<dyn VendorExtension>>,
    /// Extension-only entries (channel-scoped or family-scoped).
    extensions: Vec<Arc<dyn VendorExtension>>,
}

impl VendorRegistry {
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<VendorRegistry> = OnceLock::new();
        INSTANCE.get_or_init(Self::build)
    }

    fn build() -> Self {
        let mut vendors: Vec<Arc<dyn Vendor>> = Vec::new();
        for reg in inventory::iter::<VendorRegistration> {
            vendors.push(Arc::from((reg.make)()));
        }

        let vendor_as_ext: Vec<Arc<dyn VendorExtension>> = vendors
            .iter()
            .map(|v| Arc::new(VendorAsExt(v.clone())) as Arc<dyn VendorExtension>)
            .collect();

        let mut extensions: Vec<Arc<dyn VendorExtension>> = Vec::new();
        for reg in inventory::iter::<ExtensionRegistration> {
            extensions.push(Arc::from((reg.make)()));
        }

        Self { vendors, vendor_as_ext, extensions }
    }

    /// Look up a full [`Vendor`] by `vendor_id` (case-insensitive).
    /// Used by the proxy pipeline's target iteration loop.
    pub fn get_vendor(&self, vendor_id: &str) -> Option<&Arc<dyn Vendor>> {
        self.vendors
            .iter()
            .find(|v| v.vendor_id().eq_ignore_ascii_case(vendor_id))
    }

    /// Three-tier resolution: Channel → Vendor → Family.
    ///
    /// Returns `Arc<dyn VendorExtension>` for use by admin and auth paths
    /// that only need sync hooks (`auth_headers`, `build_url`, `metadata`).
    pub fn resolve(
        &self,
        provider: &Provider,
        protocol_id: ProtocolId,
    ) -> Option<&Arc<dyn VendorExtension>> {
        let vendor_id = provider
            .vendor
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty());
        let channel_id = provider
            .channel
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty());

        // 1. Channel-scoped (extension-only entries only, since vendor-level
        //    implementations don't carry channel scope).
        if let (Some(v), Some(c)) = (vendor_id, channel_id) {
            for ext in &self.extensions {
                if let VendorScope::Channel { vendor_id: vk, channel_id: ck } = ext.scope()
                    && vk.eq_ignore_ascii_case(v)
                    && ck.eq_ignore_ascii_case(c)
                {
                    return Some(ext);
                }
            }
        }

        // 2. Vendor-scoped (prefer full Vendor entries, then extension-only).
        if let Some(v) = vendor_id {
            for (i, vendor) in self.vendors.iter().enumerate() {
                if let VendorScope::Vendor { vendor_id: vk } = vendor.scope()
                    && vk.eq_ignore_ascii_case(v)
                {
                    return Some(&self.vendor_as_ext[i]);
                }
            }
            for ext in &self.extensions {
                if let VendorScope::Vendor { vendor_id: vk } = ext.scope()
                    && vk.eq_ignore_ascii_case(v)
                {
                    return Some(ext);
                }
            }
        }

        // 3. Family-scoped.
        for ext in &self.extensions {
            if let VendorScope::Family(family) = ext.scope()
                && family == protocol_id.family
            {
                return Some(ext);
            }
        }
        // Check vendor-level entries for Family scope (unlikely but possible).
        for (i, vendor) in self.vendors.iter().enumerate() {
            if let VendorScope::Family(family) = vendor.scope()
                && family == protocol_id.family
            {
                return Some(&self.vendor_as_ext[i]);
            }
        }

        None
    }

    pub fn list_metadata(&self) -> Vec<&'static VendorMetadata> {
        let mut out: Vec<&'static VendorMetadata> = self
            .vendors
            .iter()
            .filter_map(|v| v.metadata())
            .collect();
        // Include any metadata from extension-only entries (rare).
        for ext in &self.extensions {
            if let Some(m) = ext.metadata() {
                out.push(m);
            }
        }
        out.sort_by_key(|m| m.id);
        out.dedup_by_key(|m| m.id);
        out
    }

    pub fn metadata(&self, vendor_id: &str) -> Option<&'static VendorMetadata> {
        self.list_metadata()
            .into_iter()
            .find(|m| m.id.eq_ignore_ascii_case(vendor_id))
    }

    /// Returns vendor metadata as JSON in the legacy WebUI order.
    pub fn list_metadata_for_webui(&self) -> Vec<serde_json::Value> {
        const LEGACY_ORDER: &[&str] = &[
            "custom",
            "openai",
            "anthropic",
            "google",
            "xai",
            "deepseek",
            "moonshotai",
            "minimax",
            "zhipuai",
            "zai",
            "nvidia",
            "openrouter",
            "ollama",
        ];
        let position = |id: &str| -> usize {
            LEGACY_ORDER
                .iter()
                .position(|known| known.eq_ignore_ascii_case(id))
                .unwrap_or(LEGACY_ORDER.len())
        };
        let mut metas = self.list_metadata();
        metas.sort_by(|a, b| position(a.id).cmp(&position(b.id)).then_with(|| a.id.cmp(b.id)));
        metas
            .into_iter()
            .filter_map(|m| serde_json::to_value(m).ok())
            .collect()
    }

    /// Kept for call-site compatibility; delegates to `list_metadata_for_webui`.
    #[inline]
    pub fn list_metadata_legacy_json(&self) -> Vec<serde_json::Value> {
        self.list_metadata_for_webui()
    }
}
