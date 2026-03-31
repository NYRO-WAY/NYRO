pub mod types;
pub mod openai;
pub mod anthropic;
pub mod gemini;
pub mod semantic;

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
        Protocol::OpenAI | Protocol::ResponsesAPI => Box::new(openai::encoder::OpenAIEncoder),
        Protocol::Anthropic => Box::new(anthropic::encoder::AnthropicEncoder),
        Protocol::Gemini => Box::new(gemini::encoder::GeminiEncoder),
    }
}

pub fn get_response_parser(protocol: Protocol) -> Box<dyn ResponseParser> {
    match protocol {
        Protocol::OpenAI | Protocol::ResponsesAPI => Box::new(openai::stream::OpenAIResponseParser),
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
        Protocol::OpenAI | Protocol::ResponsesAPI => {
            Box::new(openai::stream::OpenAIStreamParser::new())
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
    pub fn from_provider(provider: &Provider) -> Self {
        let raw_map = provider.parsed_protocol_endpoints();
        let mut endpoints = HashMap::new();
        for (key, entry) in &raw_map {
            if let Ok(p) = key.parse::<Protocol>() {
                endpoints.insert(p, entry.clone());
            }
        }

        let default = provider
            .effective_default_protocol()
            .parse::<Protocol>()
            .unwrap_or_else(|_| {
                endpoints
                    .keys()
                    .next()
                    .copied()
                    .unwrap_or(Protocol::OpenAI)
            });

        Self { default, endpoints }
    }

    pub fn supports(&self, protocol: Protocol) -> bool {
        self.endpoints.contains_key(&protocol)
    }

    /// Resolve egress protocol and base_url for an incoming ingress protocol.
    /// ResponsesAPI maps to OpenAI for endpoint resolution.
    pub fn resolve_egress(&self, ingress: Protocol) -> ResolvedEgress {
        let lookup = match ingress {
            Protocol::ResponsesAPI => Protocol::OpenAI,
            other => other,
        };

        if let Some(ep) = self.endpoints.get(&lookup) {
            ResolvedEgress {
                protocol: lookup,
                base_url: ep.base_url.clone(),
                needs_conversion: false,
            }
        } else if let Some(ep) = self.endpoints.get(&self.default) {
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
