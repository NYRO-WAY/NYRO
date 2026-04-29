//! Protocol layer.
//!
//! # Two-layer identity
//!
//! Canonical form: `{family}/{dialect}/{wire_version}`.
//!
//! - `family`: closed enum `openai` / `anthropic` / `google`.
//! - `dialect`: wire-format verb/noun (`chat`, `responses`, `messages`, `generate`).
//! - `wire_version`: schema version as the vendor labels it (`v1`, `2023-06-01`, `v1beta`).
//!
//! See [`ids`], [`traits`], [`registry`], [`codec`], and [`handler`] for the model.
//!
//! ## Alias table (resolved at runtime in [`registry::ProtocolRegistry::resolve_alias`])
//!
//! Primary short names: `openai-chat`, `openai-responses`, `anthropic-messages`,
//! `google-generate` (kebab-case `{family}-{dialect}` form).
//!
//! Friendly aliases: `responses` → OpenAI Responses, `claude` → Anthropic Messages,
//! `embeddings` → OpenAI Embeddings.
//!
//! Legacy strings from the pre-PR4 `Protocol` enum (`openai`, `openai_responses`,
//! `anthropic`, `gemini`) remain resolvable for back-compat: the DB startup
//! migration rewrites stored values to canonical [`ids::ProtocolId`] strings, but
//! older yaml configs / older DB snapshots may still carry the legacy spellings.

pub mod types;
pub mod codec;
pub mod semantic;

pub mod ids;
pub mod traits;
pub mod registry;
pub mod handler;
pub mod vendor;
pub mod normalize;

use std::collections::HashMap;

use reqwest::header::HeaderMap;

use crate::db::models::{Provider, ProtocolEndpointEntry};
use crate::protocol::ids::ProtocolId;

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

// ── Provider multi-protocol negotiation ──

#[derive(Debug, Clone)]
pub struct ProviderProtocols {
    pub default: ProtocolId,
    pub endpoints: HashMap<ProtocolId, ProtocolEndpointEntry>,
}

#[derive(Debug, Clone)]
pub struct ResolvedEgress {
    pub protocol: ProtocolId,
    pub base_url: String,
    pub needs_conversion: bool,
}

impl ProviderProtocols {
    /// Best-effort string → [`ProtocolId`] resolver.
    ///
    /// Accepts legacy strings (`openai` / `openai_responses` / `anthropic` /
    /// `gemini`), short aliases (`openai-chat`, `responses`, `claude`, …),
    /// and canonical `family/dialect/version` strings — all routed through
    /// [`registry::ProtocolRegistry::resolve_alias`].
    ///
    /// Returns `None` for unknown keys so we silently drop garbage from old
    /// DB rows instead of panicking.
    pub fn parse_protocol_key(s: &str) -> Option<ProtocolId> {
        registry::ProtocolRegistry::global().resolve_alias(s)
    }

    pub fn from_provider(provider: &Provider) -> Self {
        let raw_map = provider.parsed_protocol_endpoints();
        let mut endpoints: HashMap<ProtocolId, ProtocolEndpointEntry> = HashMap::new();
        for (key, entry) in &raw_map {
            if let Some(id) = Self::parse_protocol_key(key)
                && !endpoints.contains_key(&id)
            {
                endpoints.insert(id, entry.clone());
            }
        }

        let declared_default = Self::parse_protocol_key(&provider.effective_default_protocol());
        let default = declared_default
            .filter(|id| endpoints.contains_key(id))
            .or_else(|| endpoints.keys().next().copied())
            .or(declared_default)
            .unwrap_or(ids::OPENAI_CHAT_V1);

        Self { default, endpoints }
    }

    pub fn supports(&self, protocol: ProtocolId) -> bool {
        self.endpoints.contains_key(&protocol)
    }

    /// Three-tier egress resolution:
    ///
    /// 1. **Exact [`ProtocolId`] match** — same wire format on both sides.
    /// 2. **Same-family fallback** — e.g. ingress `openai/responses/v1` against
    ///    a provider that only declares `openai/chat/v1`; we still talk OpenAI,
    ///    but the codec layer must translate.
    /// 3. **Global default** — last resort, also conversion needed.
    pub fn resolve_egress(&self, ingress: ProtocolId) -> ResolvedEgress {
        if let Some(ep) = self.endpoints.get(&ingress) {
            return ResolvedEgress {
                protocol: ingress,
                base_url: ep.base_url.clone(),
                needs_conversion: false,
            };
        }

        if let Some((id, ep)) = self
            .endpoints
            .iter()
            .find(|(id, _)| id.family == ingress.family)
        {
            return ResolvedEgress {
                protocol: *id,
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
