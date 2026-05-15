//! Stream deltas for `AiResponse`.

use crate::protocol::ir::error::AiError;
use crate::protocol::ir::request::ToolCall;
use crate::protocol::types::TokenUsage;

/// A single parsed delta from a streaming response.
///
/// The stream parser emits a sequence of `StreamDelta` values.  The accumulator
/// (PR-4) coalesces them into a complete `AiResponse`.
#[derive(Debug, Clone)]
pub enum StreamDelta {
    /// First chunk — identifies the response and model.
    MessageStart {
        id: String,
        model: String,
    },
    /// Incremental text output.
    TextDelta(String),
    /// Incremental thinking / reasoning output (Anthropic `ThinkingBlockParam`,
    /// Google `Part{thought=true}`, OpenAI reasoning items).
    ThinkingDelta(String),
    /// Thinking signature for multi-turn passback (Anthropic).
    ThinkingSignature(String),
    /// A tool call is starting.
    ToolCallStart {
        index: usize,
        id: String,
        name: String,
    },
    /// Incremental tool call argument JSON fragment.
    ToolCallDelta {
        index: usize,
        arguments: String,
    },
    /// Tool call arguments are complete.
    ToolCallComplete {
        index: usize,
        tool_call: ToolCall,
    },
    /// Final token usage statistics.
    Usage(TokenUsage),
    /// Stream ended normally.
    Done {
        stop_reason: String,
    },
    /// A mid-stream error detected by the parser (e.g. OAI `data: {"error":{...}}`,
    /// Anthropic `event: error`, Google `promptFeedback.blockReason` in first chunk).
    StreamError {
        error: AiError,
    },
    /// Stream was truncated without a `[DONE]` sentinel.
    UnexpectedEof,
    /// A verbatim SSE event not classified into any other variant.
    Unknown {
        raw: String,
    },
}
