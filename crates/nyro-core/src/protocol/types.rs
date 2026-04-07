use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Protocol;

// ── Ingress: client request → internal ──

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
    pub source_protocol: Protocol,
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct InternalMessage {
    pub role: Role,
    pub content: MessageContent,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
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
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalResponse {
    pub id: String,
    pub model: String,
    pub content: String,
    pub reasoning_content: Option<String>,
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
    MessageStart { id: String, model: String },
    ReasoningDelta(String),
    TextDelta(String),
    ToolCallStart { index: usize, id: String, name: String },
    ToolCallDelta { index: usize, arguments: String },
    Usage(TokenUsage),
    Done { stop_reason: String },
}
