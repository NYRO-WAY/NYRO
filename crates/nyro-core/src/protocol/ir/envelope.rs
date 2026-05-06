//! Raw request envelope — a snapshot of the original bytes / headers.
//!
//! Preserved for:
//! - Pass-through mode (body forwarded verbatim).
//! - Audit logging (what did the client actually send?).
//! - Debug round-trip verification.

use std::collections::HashMap;
use serde_json::Value;

/// A snapshot of the original inbound request, captured before any codec
/// transformation.
#[derive(Debug, Clone, Default)]
pub struct RawEnvelope {
    /// The original JSON body as received from the client.
    pub body: Option<Value>,
    /// Flattened request headers (lowercase keys).
    pub headers: HashMap<String, String>,
    /// The HTTP method (e.g. `"POST"`).
    pub method: String,
    /// The request path (e.g. `"/v1/chat/completions"`).
    pub path: String,
}

impl RawEnvelope {
    pub fn new(body: Option<Value>, headers: HashMap<String, String>, method: &str, path: &str) -> Self {
        Self {
            body,
            headers,
            method: method.to_string(),
            path: path.to_string(),
        }
    }
}
