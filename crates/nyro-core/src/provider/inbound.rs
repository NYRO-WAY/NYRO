//! Inbound response type — the wire-format response received from the upstream.

use serde_json::Value;

/// Non-streaming response received from the upstream provider.
#[derive(Debug)]
pub struct InboundResponse {
    /// HTTP status code.
    pub status: u16,
    /// Parsed JSON body.
    pub body: Value,
}
