//! Protocol layer.
//!
//! # Two-layer identity (PR1+)
//!
//! Canonical form: `{family}/{dialect}/{wire_version}`.
//!
//! - `family`: closed enum `openai` / `anthropic` / `google`.
//! - `dialect`: wire-format verb/noun (`chat`, `responses`, `messages`, `generate`).
//! - `wire_version`: schema version as the vendor labels it (`v1`, `2023-06-01`, `v1beta`).
//!
//! See [`ids`], [`traits`], [`registry`], and [`family`] for the new model.
//! The legacy [`Protocol`] enum, factory functions, and [`ProviderProtocols`]
//! below remain in place during PR1; PR3/PR4 migrate the call sites and
//! delete them.
//!
//! ## Default alias table (resolved at runtime in [`registry::ProtocolRegistry::resolve_alias`])
//!
//! Primary short names: `openai-chat`, `openai-responses`, `anthropic-messages`,
//! `google-generate` (kebab-case `{family}-{dialect}` form).
//!
//! Friendly aliases: `responses` → OpenAI Responses, `claude` → Anthropic Messages.
//!
//! Legacy values from the old `Protocol` enum: `openai`, `openai_responses`,
//! `anthropic`, `gemini` — still resolvable for back-compat with old DB rows
//! and yaml configs until PR4 normalizes the write paths.

pub mod types;
pub mod openai;
pub mod anthropic;
pub mod gemini;
pub mod semantic;

pub mod ids;
pub mod traits;
pub mod registry;
pub mod family;
pub mod vendor;

use std::collections::HashMap;

use reqwest::header::HeaderMap;

use crate::db::models::{Provider, ProtocolEndpointEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    OpenAI,
    Anthropic,
    Gemini,
    /// OpenAI Responses API (`POST /v1/responses`).
    /// Routes as "openai" but uses Responses-specific formatters.
    #[serde(rename = "openai_responses")]
    ResponsesAPI,
}

impl Protocol {
    /// The base protocol string used for route matching.
    /// `ResponsesAPI` shares routes with `OpenAI`.
    pub fn route_protocol(&self) -> &'static str {
        match self {
            Protocol::OpenAI | Protocol::ResponsesAPI => "openai",
            Protocol::Anthropic => "anthropic",
            Protocol::Gemini => "gemini",
        }
    }

    /// Bridge from the legacy enum to the new `ProtocolId`.
    ///
    /// PR3 plumbing only — once PR4 deletes the enum, every call site
    /// will already use `ProtocolId` natively and this disappears.
    pub fn to_protocol_id(self) -> ids::ProtocolId {
        match self {
            Protocol::OpenAI => ids::OPENAI_CHAT_V1,
            Protocol::ResponsesAPI => ids::OPENAI_RESPONSES_V1,
            Protocol::Anthropic => ids::ANTHROPIC_MESSAGES_2023_06_01,
            Protocol::Gemini => ids::GOOGLE_GENERATE_V1BETA,
        }
    }

    /// Resolve this protocol's registered handler. Panics only if the
    /// `inventory::submit!` registration is missing — a build-time
    /// invariant covered by `tests/protocol_registry.rs`.
    pub fn handler(self) -> &'static std::sync::Arc<dyn traits::ProtocolHandler> {
        let id = self.to_protocol_id();
        registry::ProtocolRegistry::global()
            .get(&id)
            .unwrap_or_else(|| panic!("ProtocolHandler for {id} not registered"))
    }
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protocol::OpenAI => write!(f, "openai"),
            Protocol::Anthropic => write!(f, "anthropic"),
            Protocol::Gemini => write!(f, "gemini"),
            Protocol::ResponsesAPI => write!(f, "openai_responses"),
        }
    }
}

impl std::str::FromStr for Protocol {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "openai" => Ok(Protocol::OpenAI),
            "anthropic" => Ok(Protocol::Anthropic),
            "gemini" => Ok(Protocol::Gemini),
            "openai_responses" => Ok(Protocol::ResponsesAPI),
            _ => anyhow::bail!("unknown protocol: {s}"),
        }
    }
}

// ── Client → Gateway ──

pub trait IngressDecoder {
    fn decode_request(&self, body: serde_json::Value) -> anyhow::Result<types::InternalRequest>;
}

// ── Gateway → Provider ──

pub trait EgressEncoder {
    fn encode_request(
        &self,
        req: &types::InternalRequest,
    ) -> anyhow::Result<(serde_json::Value, HeaderMap)>;

    fn egress_path(&self, model: &str, stream: bool) -> String;
}

// ── Provider response → internal ──

pub trait ResponseParser: Send {
    fn parse_response(
        &self,
        resp: serde_json::Value,
    ) -> anyhow::Result<types::InternalResponse>;
}

// ── Internal → client response ──

pub trait ResponseFormatter: Send {
    fn format_response(&self, resp: &types::InternalResponse) -> serde_json::Value;
}

// ── Streaming: provider → internal deltas ──

pub trait StreamParser: Send {
    fn parse_chunk(&mut self, raw: &str) -> anyhow::Result<Vec<types::StreamDelta>>;
    fn finish(&mut self) -> anyhow::Result<Vec<types::StreamDelta>>;
}

// ── Streaming: internal deltas → client SSE ──

pub trait StreamFormatter: Send {
    fn format_deltas(&mut self, deltas: &[types::StreamDelta]) -> Vec<SseEvent>;
    fn format_done(&mut self) -> Vec<SseEvent>;
    fn usage(&self) -> types::TokenUsage;
}

// ── SSE helper ──

#[derive(Debug, Clone)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

impl SseEvent {
    pub fn new(event: Option<&str>, data: impl Into<String>) -> Self {
        Self {
            event: event.map(|e| e.to_string()),
            data: data.into(),
        }
    }

    pub fn to_sse_string(&self) -> String {
        let mut s = String::new();
        if let Some(ref event) = self.event {
            s.push_str(&format!("event: {event}\n"));
        }
        s.push_str(&format!("data: {}\n\n", self.data));
        s
    }
}

// ── Factory functions ──

pub fn get_decoder(protocol: Protocol) -> Box<dyn IngressDecoder + Send> {
    match protocol {
        Protocol::OpenAI => Box::new(openai::decoder::OpenAIDecoder),
        Protocol::Anthropic => Box::new(anthropic::decoder::AnthropicDecoder),
        Protocol::Gemini => Box::new(gemini::decoder::GeminiDecoder),
        Protocol::ResponsesAPI => Box::new(openai::responses::decoder::ResponsesDecoder),
    }
}

pub fn get_encoder(protocol: Protocol) -> Box<dyn EgressEncoder + Send> {
    match protocol {
        Protocol::OpenAI => Box::new(openai::encoder::OpenAIEncoder),
        Protocol::ResponsesAPI => Box::new(openai::responses::encoder::ResponsesEncoder),
        Protocol::Anthropic => Box::new(anthropic::encoder::AnthropicEncoder),
        Protocol::Gemini => Box::new(gemini::encoder::GeminiEncoder),
    }
}

pub fn get_response_parser(protocol: Protocol) -> Box<dyn ResponseParser> {
    match protocol {
        Protocol::OpenAI => Box::new(openai::stream::OpenAIResponseParser),
        Protocol::ResponsesAPI => Box::new(openai::responses::parser::ResponsesResponseParser),
        Protocol::Anthropic => Box::new(anthropic::stream::AnthropicResponseParser),
        Protocol::Gemini => Box::new(gemini::stream::GeminiResponseParser),
    }
}

pub fn get_response_formatter(protocol: Protocol) -> Box<dyn ResponseFormatter> {
    match protocol {
        Protocol::OpenAI => Box::new(openai::stream::OpenAIResponseFormatter),
        Protocol::Anthropic => Box::new(anthropic::stream::AnthropicResponseFormatter),
        Protocol::Gemini => Box::new(gemini::stream::GeminiResponseFormatter),
        Protocol::ResponsesAPI => {
            Box::new(openai::responses::formatter::ResponsesResponseFormatter)
        }
    }
}

pub fn get_stream_parser(protocol: Protocol) -> Box<dyn StreamParser> {
    match protocol {
        Protocol::OpenAI => Box::new(openai::stream::OpenAIStreamParser::new()),
        Protocol::ResponsesAPI => {
            Box::new(openai::responses::parser::ResponsesStreamParser::new())
        }
        Protocol::Anthropic => Box::new(anthropic::stream::AnthropicStreamParser::new()),
        Protocol::Gemini => Box::new(gemini::stream::GeminiStreamParser::new()),
    }
}

pub fn get_stream_formatter(protocol: Protocol) -> Box<dyn StreamFormatter> {
    match protocol {
        Protocol::OpenAI => Box::new(openai::stream::OpenAIStreamFormatter::new()),
        Protocol::Anthropic => Box::new(anthropic::stream::AnthropicStreamFormatter::new()),
        Protocol::Gemini => Box::new(gemini::stream::GeminiStreamFormatter::new()),
        Protocol::ResponsesAPI => {
            Box::new(openai::responses::stream::ResponsesStreamFormatter::new())
        }
    }
}

// ── Provider multi-protocol negotiation ──

#[derive(Debug, Clone)]
pub struct ProviderProtocols {
    pub default: Protocol,
    pub endpoints: HashMap<Protocol, ProtocolEndpointEntry>,
}

#[derive(Debug, Clone)]
pub struct ResolvedEgress {
    pub protocol: Protocol,
    pub base_url: String,
    pub needs_conversion: bool,
}

impl ProviderProtocols {
    /// Best-effort string → `Protocol` resolver. Accepts:
    ///
    /// - legacy strings (`openai` / `openai_responses` / `anthropic` / `gemini`),
    /// - the new alias table (`openai-chat`, `responses`, `claude`, …),
    /// - canonical [`ids::ProtocolId`] strings (`openai/chat/v1`, …).
    ///
    /// Used to read `provider.protocol_endpoints` JSON during runtime — DB
    /// rows may carry any of the three forms during the PR3/PR4 transition.
    fn parse_protocol_key(s: &str) -> Option<Protocol> {
        if let Ok(p) = s.parse::<Protocol>() {
            return Some(p);
        }
        let id = registry::ProtocolRegistry::global().resolve_alias(s)?;
        if id == ids::OPENAI_CHAT_V1 {
            Some(Protocol::OpenAI)
        } else if id == ids::OPENAI_RESPONSES_V1 {
            Some(Protocol::ResponsesAPI)
        } else if id == ids::ANTHROPIC_MESSAGES_2023_06_01 {
            Some(Protocol::Anthropic)
        } else if id == ids::GOOGLE_GENERATE_V1BETA {
            Some(Protocol::Gemini)
        } else {
            // Embeddings and other dialects are not part of the legacy
            // `Protocol` enum surface; they bypass `ProviderProtocols`.
            None
        }
    }

    pub fn from_provider(provider: &Provider) -> Self {
        let raw_map = provider.parsed_protocol_endpoints();
        let mut endpoints: HashMap<Protocol, ProtocolEndpointEntry> = HashMap::new();
        for (key, entry) in &raw_map {
            if let Some(p) = Self::parse_protocol_key(key)
                && !endpoints.contains_key(&p)
            {
                endpoints.insert(p, entry.clone());
            }
        }

        let declared_default = Self::parse_protocol_key(&provider.effective_default_protocol());
        let default = declared_default
            .filter(|p| endpoints.contains_key(p))
            .or_else(|| endpoints.keys().next().copied())
            .or(declared_default)
            .unwrap_or(Protocol::OpenAI);

        Self { default, endpoints }
    }

    pub fn supports(&self, protocol: Protocol) -> bool {
        self.endpoints.contains_key(&protocol)
    }

    /// Three-tier egress resolution:
    ///
    /// 1. **Exact `ProtocolId` match** — same wire format on both sides.
    /// 2. **Same-family fallback** — e.g. ingress `openai-responses` against
    ///    a provider that only declares `openai-chat`; we still talk OpenAI,
    ///    but the codec layer must translate.
    /// 3. **Global default** — last resort, also conversion needed.
    pub fn resolve_egress(&self, ingress: Protocol) -> ResolvedEgress {
        if let Some(ep) = self.endpoints.get(&ingress) {
            return ResolvedEgress {
                protocol: ingress,
                base_url: ep.base_url.clone(),
                needs_conversion: false,
            };
        }

        let target_family = ingress.to_protocol_id().family;
        if let Some((p, ep)) = self
            .endpoints
            .iter()
            .find(|(p, _)| p.to_protocol_id().family == target_family)
        {
            return ResolvedEgress {
                protocol: *p,
                base_url: ep.base_url.clone(),
                needs_conversion: true,
            };
        }

        if let Some(ep) = self.endpoints.get(&self.default) {
            ResolvedEgress {
                protocol: self.default,
                base_url: ep.base_url.clone(),
                needs_conversion: true,
            }
        } else {
            ResolvedEgress {
                protocol: self.default,
                base_url: String::new(),
                needs_conversion: true,
            }
        }
    }
}
