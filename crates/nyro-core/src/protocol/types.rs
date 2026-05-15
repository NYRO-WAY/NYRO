use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── InternalMessage — internal codec helper type ──────────────────────────────
//
// Used by codec encoders as an intermediate working type.
// `InternalRequest`/`InternalResponse` have been removed (PR-6).

#[derive(Debug, Clone)]
pub struct InternalMessage {
    pub role: Role,
    pub content: MessageContent,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
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

// ── Shared response types ─────────────────────────────────────────────────────

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
    Usage(TokenUsage),
    Done {
        stop_reason: String,
    },
    /// A verbatim SSE event that was not classified into a known delta type.
    RawEvent {
        event_type: String,
        data: Value,
    },
}
