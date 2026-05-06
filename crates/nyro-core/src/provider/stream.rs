//! Provider-level stream parser trait and legacy adapter.

use crate::error::GatewayError;
use crate::protocol::types::StreamDelta;

/// Provider-level streaming parser. Wraps the codec's `StreamParser` and
/// exposes a `GatewayError`-typed interface to the dispatcher.
pub trait ProviderStreamParser: Send {
    /// Parse one raw SSE chunk. Returns `None` if the chunk produces no
    /// actionable delta (e.g. comments or keep-alive lines).
    fn parse_chunk(&mut self, chunk: &str) -> Result<Option<Vec<StreamDelta>>, GatewayError>;

    /// Called after the stream ends. Returns any final token-usage data
    /// extracted from the last chunk or a synthesized estimate.
    fn finish(&mut self) -> anyhow::Result<Vec<StreamDelta>>;
}

// ── LegacyStreamParserAdapter ─────────────────────────────────────────────────

/// Wraps the codec-level `Box<dyn StreamParser>` behind `ProviderStreamParser`.
///
/// Vendor-level stream hooks (`on_stream_raw_chunk` / `on_stream_delta`) are
/// not yet wired here; that is a PR-16 concern once the dispatcher calls this
/// trait directly.
pub struct LegacyStreamParserAdapter(pub Box<dyn crate::protocol::StreamParser>);

impl ProviderStreamParser for LegacyStreamParserAdapter {
    fn parse_chunk(&mut self, chunk: &str) -> Result<Option<Vec<StreamDelta>>, GatewayError> {
        let deltas = self
            .0
            .parse_chunk(chunk)
            .map_err(GatewayError::internal)?;
        if deltas.is_empty() {
            Ok(None)
        } else {
            Ok(Some(deltas))
        }
    }

    fn finish(&mut self) -> anyhow::Result<Vec<StreamDelta>> {
        self.0.finish()
    }
}

