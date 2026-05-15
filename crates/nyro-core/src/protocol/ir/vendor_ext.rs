//! Vendor extensions — three-segment model.
//!
//! Every `AiRequest` and `AiResponse` carries a `VendorExtensions` bag that
//! holds fields which don't have a home in the canonical IR schema.
//!
//! ## Three segments
//!
//! - **`ingress`** — extra fields extracted from the *client* body.  These
//!   belong to the ingress protocol family (e.g. OpenAI `service_tier`).
//!   Forwarded to the egress by the codec if the egress vendor understands them.
//!
//! - **`egress`** — fields injected by the egress codec or `ProviderAdapter`
//!   just before the upstream call.  Examples: `anthropic-beta` header hints,
//!   Google `cachedContent` token.  Not present in the ingress body.
//!
//! - **`passthrough_safe`** — fields the gateway does not understand at all but
//!   explicitly allowed to pass through.  The codec copies them verbatim to the
//!   egress body after a whitelist check.  Lossy-reject mode (`EndpointCapability`
//!   in PR-07) will reject requests with non-empty `passthrough_safe` entries
//!   unless `allow_passthrough` is set on the route.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VendorExtensions {
    /// Extra fields from the ingress body (client side).
    pub ingress: HashMap<String, Value>,
    /// Extra fields for the egress body (provider side).
    pub egress: HashMap<String, Value>,
    /// Fields to copy verbatim; subject to whitelist / lossy-reject.
    pub passthrough_safe: HashMap<String, Value>,
}

impl VendorExtensions {
    pub fn is_empty(&self) -> bool {
        self.ingress.is_empty() && self.egress.is_empty() && self.passthrough_safe.is_empty()
    }
}
