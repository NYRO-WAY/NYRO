//! Protocol-domain strong-typed extensions (`ProtocolExt`).
//!
//! Each variant holds fields that are specific to one protocol family and
//! influence codec behaviour but are **not** consumed by gateway infrastructure
//! (router, quota, cache-key, dispatcher).  Fields in this module are only
//! read by the matching protocol encoder or stream formatter.
//!
//! ## Design
//!
//! Using a concrete enum (rather than `Box<dyn Any>`) keeps downcasting
//! ergonomic and avoids allocations for the common case where no extension is
//! needed.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Protocol-specific request extension.
///
/// Populated by the ingress codec decoder; consumed by the egress encoder.
/// When Nyro translates between protocols (e.g. Anthropic → OpenAI), the
/// source `ProtocolExt` is discarded and the encoder relies solely on the
/// core `AiRequest` fields.
#[derive(Debug, Clone)]
pub enum ProtocolExt {
    OpenAiChat(OpenAIChatExt),
    OpenAiResponses(OpenAIResponsesExt),
    Anthropic(AnthropicExt),
    Google(GoogleExt),
}

// ── OpenAI Chat Completions ────────────────────────────────────────────────

/// Extension fields for `POST /v1/chat/completions`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIChatExt {
    /// Audio output configuration (`audio.voice`, `audio.format`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<Value>,
    /// Token logit biases (map from token ID to bias value −100..100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logit_bias: Option<HashMap<String, f64>>,
    /// Whether to return log-probabilities of output tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<bool>,
    /// Number of top log-probabilities per token position (requires `logprobs = true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_logprobs: Option<u32>,
    /// Output modalities (`["text"]`, `["text", "audio"]`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modalities: Option<Vec<String>>,
    /// Number of candidate completions to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    /// Speculative decoding prediction content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prediction: Option<Value>,
    /// Extended prompt-cache retention (`"in_memory"` | `"24h"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_retention: Option<String>,
    /// Streaming options (e.g. `include_usage`, `include_obfuscation`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<Value>,
    /// Response verbosity hint (`"low"` | `"medium"` | `"high"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbosity: Option<String>,
    /// Web search tool options.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_search_options: Option<Value>,
}

// ── OpenAI Responses API ──────────────────────────────────────────────────

/// Extension fields for `POST /v1/responses`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIResponsesExt {
    /// Run the model in background mode (fire-and-forget).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<bool>,
    /// Context management configuration (compression strategies).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_management: Option<Value>,
    /// Conversation the response belongs to (stateful multi-turn).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation: Option<Value>,
    /// Additional output data to include in the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    /// Previous response ID for multi-turn stateless conversations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    /// Prompt template reference and variable bindings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<Value>,
    /// Extended prompt-cache retention (`"in_memory"` | `"24h"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_retention: Option<String>,
    /// Streaming options.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<Value>,
    /// Top log-probabilities per token position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_logprobs: Option<u32>,
    /// Context truncation strategy (`"auto"` | `"disabled"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation: Option<String>,
    /// Protocol-specific `tool_choice` variants not representable in the core IR
    /// (e.g. `allowed_tools`, MCP tool choice).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice_ext: Option<Value>,
}

// ── Anthropic Messages ────────────────────────────────────────────────────

/// Extension fields for `POST /v1/messages`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnthropicExt {
    /// Top-k nucleus sampling (Anthropic-specific).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Code execution container configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<Value>,
    /// Inference geo-routing preference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inference_geo: Option<String>,
    /// Output configuration (`effort` + `format: json_schema`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_config: Option<Value>,
    /// Anthropic service tier (different enum space from OAI).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    /// Server-side tool specifications (bash, code_execution, web_search, etc.).
    /// Encoded verbatim into the `tools` array alongside user-defined tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_tools: Option<Vec<Value>>,
}

// ── Google GenAI ──────────────────────────────────────────────────────────

/// Extension fields for `POST /v1beta/{model}:generateContent`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoogleExt {
    /// Top-k token sampling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Number of candidate responses to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_count: Option<u32>,
    /// Whether to include token log-probabilities in the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_logprobs: Option<bool>,
    /// Number of top log-probabilities per token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<u32>,
    /// Output MIME type (`"text/plain"` | `"application/json"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_mime_type: Option<String>,
    /// JSON Schema alternative to `response_format` (full JSON Schema standard).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_json_schema: Option<Value>,
    /// Function calling mode configuration (`toolConfig`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<Value>,
    /// Server-side cached content resource name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_content: Option<String>,
    /// Requested response modalities (`["TEXT"]`, `["IMAGE"]`, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_modalities: Option<Vec<String>>,
    /// Thinking / reasoning budget configuration (`thinkingBudget`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<Value>,
    /// Image generation configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_config: Option<Value>,
}
