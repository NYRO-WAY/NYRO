//! `AiRequest` — the new unified ingress IR for all supported protocols.
//!
//! PR-08–12 will update the codec decoders to produce `AiRequest` instead of
//! the old `InternalRequest`.  Until then, `compat.rs` provides lossless
//! round-trip `From` conversions.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::ids::ProtocolId;
use crate::protocol::ir::envelope::RawEnvelope;
use crate::protocol::ir::vendor_ext::VendorExtensions;

// ── Role ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

// ── Content blocks ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Image {
        media_type: String,
        data: String,
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
    /// A raw JSON block that the codec does not understand.  Preserved for
    /// pass-through and future extension.
    Unknown {
        raw: Value,
    },
}

impl ContentBlock {
    pub fn as_text(&self) -> Option<&str> {
        if let Self::Text { text } = self {
            Some(text)
        } else {
            None
        }
    }
}

/// Message content can be a plain string or a list of typed blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

impl MessageContent {
    pub fn to_text(&self) -> String {
        match self {
            Self::Text(t) => t.clone(),
            Self::Blocks(bs) => bs
                .iter()
                .filter_map(|b| b.as_text())
                .collect::<Vec<_>>()
                .join(""),
        }
    }
}

// ── Message ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: MessageContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// The `tool_call_id` of the preceding assistant tool call this result
    /// answers.  Required for `Role::Tool` messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Provider-specific extras for this individual message (e.g. Anthropic
    /// `cache_control`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

// ── Tool spec ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: Value,
    /// Vendor-specific extra fields (e.g. Google `codeExecution`, Anthropic
    /// `cache_control`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// Flexible `tool_choice` that can be a string sentinel or a named-tool object.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoice {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "none")]
    None,
    #[serde(rename = "required")]
    Required,
    Named {
        name: String,
    },
    Raw(Value),
}

// ── Generation config ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logit_bias: Option<std::collections::HashMap<String, f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
}

// ── Reasoning config ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReasoningConfig {
    /// Whether extended reasoning / thinking is enabled.
    pub enabled: bool,
    /// Budget in tokens (Anthropic `budget_tokens`, OpenAI `budget_tokens`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<u32>,
    /// Effort level (`"low" / "medium" / "high"`) for OpenAI `o` models.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

// ── Response format ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseFormat {
    Text,
    JsonObject,
    JsonSchema {
        name: String,
        schema: Value,
        strict: Option<bool>,
    },
}

// ── Stream config ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamConfig {
    pub enabled: bool,
    /// Whether the provider should include token usage in the final stream
    /// chunk (OpenAI `stream_options.include_usage`).
    pub include_usage: bool,
}

// ── Safety settings ───────────────────────────────────────────────────────────

/// Google SafetySettings are vendor-specific but important enough to have a
/// first-class home in the IR.  Other vendors ignore this field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetySettings {
    pub category: String,
    pub threshold: String,
}

// ── Request metadata ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct RequestMetadata {
    /// The protocol the client spoke.
    pub source_protocol: Option<ProtocolId>,
    /// Raw envelope preserved for pass-through / audit.
    pub raw: Option<RawEnvelope>,
    /// Three-segment vendor extension bag.
    pub vendor: VendorExtensions,
}

// ── AiRequest ─────────────────────────────────────────────────────────────────

/// Unified ingress IR.  Consumed by all codec encoders and the `dispatcher`.
///
/// Every field that the old `InternalRequest` had is present here.  Fields
/// added in PR-08–12 are new (marked with their PR number).
#[derive(Debug, Clone)]
pub struct AiRequest {
    // ── Core ──────────────────────────────────────────────────────────────────
    /// The model identifier as received from the client.
    pub model: String,
    /// Conversation history.
    pub messages: Vec<Message>,
    /// System prompt (extracted from a leading `system` message or the
    /// top-level `system` field in Anthropic Messages API).
    pub system: Option<String>,

    // ── Generation ────────────────────────────────────────────────────────────
    pub generation: GenerationConfig,

    // ── Streaming ────────────────────────────────────────────────────────────
    pub stream: StreamConfig,

    // ── Tools ─────────────────────────────────────────────────────────────────
    pub tools: Option<Vec<ToolSpec>>,
    pub tool_choice: Option<ToolChoice>,
    /// Whether the provider should call tools in parallel (OpenAI PR-08).
    pub parallel_tool_calls: Option<bool>,

    // ── Reasoning ─────────────────────────────────────────────────────────────
    pub reasoning: ReasoningConfig,

    // ── Output format ─────────────────────────────────────────────────────────
    pub response_format: Option<ResponseFormat>,
    /// Modalities to include in the response (OpenAI `modalities` PR-08).
    pub modalities: Option<Vec<String>>,

    // ── Safety ────────────────────────────────────────────────────────────────
    pub safety_settings: Option<Vec<SafetySettings>>,

    // ── Metadata / extensions ─────────────────────────────────────────────────
    pub meta: RequestMetadata,
}

impl AiRequest {
    /// Convenience constructor with minimal required fields.
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            system: None,
            generation: GenerationConfig::default(),
            stream: StreamConfig::default(),
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            reasoning: ReasoningConfig::default(),
            response_format: None,
            modalities: None,
            safety_settings: None,
            meta: RequestMetadata::default(),
        }
    }
}
