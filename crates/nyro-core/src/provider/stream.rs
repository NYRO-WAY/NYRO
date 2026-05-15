//! Provider-level stream parser trait and legacy adapter.

use crate::error::GatewayError;
use crate::protocol::ir::compat::ai_stream_delta_to_old;
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
/// Converts the codec's `AiStreamDelta` output back to the old `StreamDelta`
/// so vendor-level code that still uses `ProviderStreamParser` keeps working.
pub struct LegacyStreamParserAdapter(pub Box<dyn crate::protocol::StreamParser>);

impl ProviderStreamParser for LegacyStreamParserAdapter {
    fn parse_chunk(&mut self, chunk: &str) -> Result<Option<Vec<StreamDelta>>, GatewayError> {
        let ai_deltas = self.0.parse_chunk(chunk).map_err(GatewayError::internal)?;
        if ai_deltas.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ai_deltas.iter().map(ai_stream_delta_to_old).collect()))
        }
    }

    fn finish(&mut self) -> anyhow::Result<Vec<StreamDelta>> {
        let ai_deltas = self.0.finish()?;
        Ok(ai_deltas.iter().map(ai_stream_delta_to_old).collect())
    }
}
