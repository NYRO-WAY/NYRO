//! `AiResponse` — the unified egress IR produced by all codec response parsers.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::ir::error::AiError;
use crate::protocol::ir::vendor_ext::VendorExtensions;
use crate::protocol::types::TokenUsage;

// ── ResponseItem ──────────────────────────────────────────────────────────────

/// A typed item in the response item graph (native for OpenAI Responses API).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseItem {
    /// A text output block.
    OutputText { text: String },
    /// A thinking / reasoning block.
    Thinking { text: String },
    /// A tool call issued by the model.
    FunctionCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    /// A tool result provided by the client (in multi-turn Responses API).
    FunctionCallOutput { call_id: String, output: String },
    /// A web-search result block (OpenAI built-in tool).
    WebSearchResult {
        url: String,
        title: Option<String>,
        snippet: Option<String>,
    },
    /// Unknown item type — preserved verbatim.
    Unknown { raw: Value },
}

// ── AiResponse ────────────────────────────────────────────────────────────────

/// Unified egress IR produced by all codec response parsers and the accumulator.
#[derive(Debug, Clone)]
pub struct AiResponse {
    /// The unique response ID assigned by the provider.
    pub id: String,
    /// The model variant that was actually used.
    pub model: String,
    /// The primary text content (convenience field; also present in `content_blocks`).
    pub content: String,
    /// Extended thinking / reasoning output (convenience field).
    pub reasoning_content: Option<String>,
    /// Thinking signature for multi-turn passback (Anthropic).
    pub reasoning_signature: Option<String>,
    /// Tool calls issued by the model (convenience field; also in `content_blocks`).
    pub tool_calls: Vec<crate::protocol::ir::request::ToolCall>,
    /// Item graph (native for OpenAI Responses; synthesized for other protocols).
    pub items: Option<Vec<ResponseItem>>,
    /// Stop reason (e.g. `"stop"`, `"tool_use"`, `"length"`).
    pub stop_reason: Option<String>,
    /// Token usage.
    pub usage: TokenUsage,
    /// Normalized error — populated when the provider returns an error response
    /// or the parser detects a mid-stream error.
    pub error: Option<AiError>,
    /// Vendor-specific extra fields.
    pub vendor: VendorExtensions,
}

impl AiResponse {
    pub fn new(id: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            model: model.into(),
            content: String::new(),
            reasoning_content: None,
            reasoning_signature: None,
            tool_calls: Vec::new(),
            items: None,
            stop_reason: None,
            usage: TokenUsage::default(),
            error: None,
            vendor: VendorExtensions::default(),
        }
    }

    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }
}
