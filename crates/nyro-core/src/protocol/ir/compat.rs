//! Compatibility helpers for progressive codec migration.
//!
//! PR-A removed `InternalMessage`, `TokenUsage` and all by-ref encoder helpers.
//! This module now provides only:
//! - `AiStreamDelta` ↔ old `StreamDelta` conversions (used by `LegacyStreamParserAdapter`)
//!
//! PR-B will remove this module entirely once the 4 stream parsers emit
//! `AiStreamDelta` directly.

use crate::protocol::ir::stream::StreamDelta as AiStreamDelta;
use crate::protocol::ir::usage::Usage;
use crate::protocol::types::StreamDelta as OldStreamDelta;

// ── StreamDelta ↔ AiStreamDelta conversions ───────────────────────────────────

/// Convert an old `StreamDelta` to the new IR `AiStreamDelta`.
pub fn old_stream_delta_to_new(d: &OldStreamDelta) -> AiStreamDelta {
    match d {
        OldStreamDelta::MessageStart { id, model } => AiStreamDelta::MessageStart {
            id: id.clone(),
            model: model.clone(),
        },
        OldStreamDelta::ReasoningDelta(s) => AiStreamDelta::ThinkingDelta(s.clone()),
        OldStreamDelta::ReasoningSignature(s) => AiStreamDelta::ThinkingSignature(s.clone()),
        OldStreamDelta::TextDelta(s) => AiStreamDelta::TextDelta(s.clone()),
        OldStreamDelta::ToolCallStart { index, id, name } => AiStreamDelta::ToolCallStart {
            index: *index,
            id: id.clone(),
            name: name.clone(),
        },
        OldStreamDelta::ToolCallDelta { index, arguments } => AiStreamDelta::ToolCallDelta {
            index: *index,
            arguments: arguments.clone(),
        },
        OldStreamDelta::Usage(u) => AiStreamDelta::Usage(u.clone()),
        OldStreamDelta::Done { stop_reason } => AiStreamDelta::Done {
            stop_reason: stop_reason.clone(),
        },
        OldStreamDelta::RawEvent { event_type, data } => AiStreamDelta::Unknown {
            raw: format!("event: {event_type}\ndata: {data}"),
        },
    }
}

/// Convert a new IR `AiStreamDelta` back to the old `StreamDelta`.
pub fn ai_stream_delta_to_old(d: &AiStreamDelta) -> OldStreamDelta {
    match d {
        AiStreamDelta::MessageStart { id, model } => OldStreamDelta::MessageStart {
            id: id.clone(),
            model: model.clone(),
        },
        AiStreamDelta::TextDelta(s) => OldStreamDelta::TextDelta(s.clone()),
        AiStreamDelta::ThinkingDelta(s) => OldStreamDelta::ReasoningDelta(s.clone()),
        AiStreamDelta::ThinkingSignature(s) => OldStreamDelta::ReasoningSignature(s.clone()),
        AiStreamDelta::ToolCallStart { index, id, name } => OldStreamDelta::ToolCallStart {
            index: *index,
            id: id.clone(),
            name: name.clone(),
        },
        AiStreamDelta::ToolCallDelta { index, arguments } => OldStreamDelta::ToolCallDelta {
            index: *index,
            arguments: arguments.clone(),
        },
        AiStreamDelta::ToolCallComplete { index, tool_call } => OldStreamDelta::ToolCallDelta {
            index: *index,
            arguments: tool_call.arguments.clone(),
        },
        AiStreamDelta::Usage(u) => OldStreamDelta::Usage(u.clone()),
        AiStreamDelta::Done { stop_reason } => OldStreamDelta::Done {
            stop_reason: stop_reason.clone(),
        },
        AiStreamDelta::StreamError { .. } => OldStreamDelta::Done {
            stop_reason: "error".to_string(),
        },
        AiStreamDelta::UnexpectedEof => OldStreamDelta::Done {
            stop_reason: "error".to_string(),
        },
        AiStreamDelta::Unknown { raw } => {
            let mut lines = raw.splitn(2, '\n');
            let event_type = lines
                .next()
                .and_then(|l| l.strip_prefix("event: "))
                .unwrap_or("unknown")
                .to_string();
            let data_str = lines
                .next()
                .and_then(|l| l.strip_prefix("data: "))
                .unwrap_or("{}");
            let data = serde_json::from_str(data_str)
                .unwrap_or_else(|_| serde_json::Value::String(data_str.to_string()));
            OldStreamDelta::RawEvent { event_type, data }
        }
    }
}

/// Helper: convert `ir::Usage` field names to match old `TokenUsage` field names
/// where the wire output JSON still needs old names (e.g. "input_tokens").
pub fn usage_to_wire(u: &Usage) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "input_tokens": u.prompt_tokens,
        "output_tokens": u.completion_tokens,
    });
    if u.total_tokens > 0 {
        obj["total_tokens"] = u.total_tokens.into();
    }
    if let Some(v) = u.cache_read_tokens {
        obj["cache_read_input_tokens"] = v.into();
    }
    if let Some(v) = u.cache_creation_tokens {
        obj["cache_creation_input_tokens"] = v.into();
    }
    obj
}
