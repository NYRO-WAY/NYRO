//! Stream deltas for `AiResponse`.

use crate::protocol::types::TokenUsage;
use crate::protocol::ir::request::ToolCall;

/// A single parsed delta from a streaming response.
#[derive(Debug, Clone)]
pub enum StreamDelta {
    MessageStart { id: String, model: String },
    TextDelta(String),
    ReasoningDelta(String),
    ReasoningSignature(String),
    ToolCallStart { index: usize, id: String, name: String },
    ToolCallDelta { index: usize, arguments: String },
    ToolCallComplete { index: usize, tool_call: ToolCall },
    Usage(TokenUsage),
    Done { stop_reason: String },
    Unknown { raw: String },
}
