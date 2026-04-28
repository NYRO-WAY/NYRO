//! Global registry of `VendorExtension` implementations.

use std::sync::{Arc, OnceLock};

use crate::db::models::Provider;
use crate::protocol::ids::{ProtocolFamily, ProtocolId};

use super::types::VendorMetadata;
use super::VendorExtension;

/// Selector for `VendorExtension` matching. Resolution order:
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

/// `inventory` registration record. Each vendor module submits one of
/// these per extension instance.
pub struct VendorRegistration {
    pub make: fn() -> Box<dyn VendorExtension>,
}

inventory::collect!(VendorRegistration);

/// Process-wide vendor registry.
pub struct VendorRegistry {
    extensions: Vec<Arc<dyn VendorExtension>>,
}

impl VendorRegistry {
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<VendorRegistry> = OnceLock::new();
        INSTANCE.get_or_init(Self::build)
    }

    fn build() -> Self {
        let mut extensions: Vec<Arc<dyn VendorExtension>> = Vec::new();
        for reg in inventory::iter::<VendorRegistration> {
            extensions.push(Arc::from((reg.make)()));
        }
        Self { extensions }
    }

    /// Three-tier resolution: channel → vendor → family.
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

        if let (Some(v), Some(c)) = (vendor_id, channel_id) {
            for ext in &self.extensions {
                if let VendorScope::Channel {
                    vendor_id: vk,
                    channel_id: ck,
                } = ext.scope()
                    && vk.eq_ignore_ascii_case(v)
                    && ck.eq_ignore_ascii_case(c)
                {
                    return Some(ext);
                }
            }
        }

        if let Some(v) = vendor_id {
            for ext in &self.extensions {
                if let VendorScope::Vendor { vendor_id: vk } = ext.scope()
                    && vk.eq_ignore_ascii_case(v)
                {
                    return Some(ext);
                }
            }
        }

        for ext in &self.extensions {
            if let VendorScope::Family(family) = ext.scope()
                && family == protocol_id.family
            {
                return Some(ext);
            }
        }

        None
    }

    /// Static metadata sorted by vendor id. Used by the WebUI preset
    /// list and (in PR5) replaces `assets/providers.json`.
    pub fn list_metadata(&self) -> Vec<&'static VendorMetadata> {
        let mut out: Vec<&'static VendorMetadata> = self
            .extensions
            .iter()
            .filter_map(|ext| ext.metadata())
            .collect();
        out.sort_by_key(|m| m.id);
        out.dedup_by_key(|m| m.id);
        out
    }

    /// Look up metadata by vendor id.
    pub fn metadata(&self, vendor_id: &str) -> Option<&'static VendorMetadata> {
        self.list_metadata()
            .into_iter()
            .find(|m| m.id.eq_ignore_ascii_case(vendor_id))
    }
}
