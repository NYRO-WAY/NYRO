use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ids::ProtocolId;

// ── Ingress: client request → internal ──
//
// NOTE: `InternalRequest` and `InternalResponse` are superseded by
// `crate::protocol::ir::{AiRequest, AiResponse}` (PR-06).
// These types remain for codec backward compatibility during PR-08–12.
// Do not add new fields here; add them to `AiRequest`/`AiResponse` instead.

#[derive(Debug, Clone)]
pub struct InternalRequest {
    pub messages: Vec<InternalMessage>,
    pub model: String,
    pub stream: bool,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub top_p: Option<f64>,
    pub tools: Option<Vec<ToolDef>>,
    pub tool_choice: Option<Value>,
    pub source_protocol: ProtocolId,
    /// Superseded by `AiRequest.meta.vendor.ingress`. Will be removed once all
    /// codec decoders migrate to the new IR (protocol/ir/request.rs).
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct InternalMessage {
    pub role: Role,
    pub content: MessageContent,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
    /// Superseded by per-message metadata in `AiRequest`. Will be removed once all
    /// codec decoders migrate to the new IR (protocol/ir/request.rs).
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

impl MessageContent {
    pub fn as_text(&self) -> String {
        match self {
            MessageContent::Text(t) => t.clone(),
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(""),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Image {
        source: ImageSource,
    },
    Reasoning {
        text: String,
        signature: Option<String>,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: Value,
    },
}

#[derive(Debug, Clone)]
pub struct ImageSource {
    pub media_type: String,
    pub data: String,
}

// ── Egress: internal → upstream response ──

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerToolUsage {
    pub web_search_requests: u32,
    pub web_fetch_requests: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Number of tokens read from the prompt cache (Anthropic / compatible providers).
    pub cache_read_input_tokens: Option<u32>,
    /// Number of tokens written to the prompt cache.
    pub cache_creation_input_tokens: Option<u32>,
    /// Server-side tool call counts (web search / web fetch).
    pub server_tool_use: Option<ServerToolUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalResponse {
    pub id: String,
    pub model: String,
    pub content: String,
    pub reasoning_content: Option<String>,
    pub reasoning_signature: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub response_items: Option<Vec<ResponseItem>>,
    pub stop_reason: Option<String>,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseItem {
    Reasoning {
        text: String,
    },
    FunctionCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    Message {
        text: String,
    },
}

// ── Streaming ──

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
    Usage(TokenUsage),
    Done {
        stop_reason: String,
    },
    /// A verbatim SSE event that was not classified into a known delta type.
    /// Forwarded as-is by same-protocol formatters so no upstream data is silently dropped.
    /// Other formatters (e.g. OpenAI, Google) ignore it.
    RawEvent {
        event_type: String,
        data: Value,
    },
}
