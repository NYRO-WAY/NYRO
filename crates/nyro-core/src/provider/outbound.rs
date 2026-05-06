//! Outbound request type — the wire-format request sent to the upstream provider.

use reqwest::header::HeaderMap;
use serde_json::Value;

/// Fully resolved upstream request ready to hand off to the transport layer.
#[derive(Debug)]
pub struct OutboundRequest {
    /// Full upstream URL (base + path + optional query params).
    pub url: String,
    /// HTTP headers (auth, content-type, vendor-specific).
    pub headers: HeaderMap,
    /// JSON body.
    pub body: Value,
}
