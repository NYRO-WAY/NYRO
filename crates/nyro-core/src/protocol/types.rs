use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Retained for stream parser internal use (PR-B will remove these) ──────────
//
// `InternalMessage`, `Role`, `MessageContent`, `ContentBlock`, `ImageSource`,
// `ToolDef`, `ResponseItem`, `TokenUsage` and `ServerToolUsage` have been
// removed in PR-A.  Only `ToolCall` and `StreamDelta` remain here while the 4
// stream parsers still emit the old delta type before PR-B migrates them to
// `ir::AiStreamDelta`.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

// ── Streaming ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum StreamDelta {
    MessageStart {
        id: String,
        model: String,
    },
    ReasoningDelta(String),
    ReasoningSignature(String),
    TextDelta(String),
    ToolCallStart {
        index: usize,
        id: String,
        name: String,
    },
    ToolCallDelta {
        index: usize,
        arguments: String,
    },
    Usage(crate::protocol::ir::usage::Usage),
    Done {
        stop_reason: String,
    },
    /// A verbatim SSE event that was not classified into a known delta type.
    RawEvent {
        event_type: String,
        data: Value,
    },
}
