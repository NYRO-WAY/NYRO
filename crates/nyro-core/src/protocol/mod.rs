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
//! See [`ids`], [`traits`], [`registry`], and [`codec`] for the model.
//!
//! ## Codec layout (PR-14)
//!
//! Each `codec/<family>/` directory now co-locates the wire codecs **and** the
//! thin `ProtocolHandler` registration shell for every dialect:
//!
//! - `codec/openai/chat.rs` — `OpenAIChatV1` + `inventory::submit!`
//! - `codec/openai/embeddings.rs` — `OpenAIEmbeddingsV1` + codec types
//! - `codec/openai/responses/handler.rs` — `OpenAIResponsesV1`
//! - `codec/anthropic/messages.rs` — `AnthropicMessages2023`
//! - `codec/google/generate.rs` — `GoogleGenerateV1Beta`
//!
//! Shared semantic utilities live in `codec/reasoning.rs` and
//! `codec/tool_correlation.rs`.
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

pub mod ids;
pub mod ir;
pub mod traits;
pub mod registry;

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

/// Declared protocol capabilities of a single provider, built from the DB row.
///
/// **`endpoints` is a `Vec` (ordered, not `HashMap`) so that fallback
/// resolution is deterministic.**  The order matches the JSON key order of the
/// stored `protocol_endpoints` column after normalization; later entries have
/// lower priority.
#[derive(Debug, Clone)]
pub struct ProviderProtocols {
    pub default: ProtocolId,
    /// Ordered list of supported endpoints.  First match wins in fallback logic.
    pub endpoints: Vec<(ProtocolId, ProtocolEndpointEntry)>,
}

#[derive(Debug, Clone)]
pub struct ResolvedEgress {
    pub protocol: ProtocolId,
    pub base_url: String,
    pub needs_conversion: bool,
}

impl ProviderProtocols {
    /// Best-effort string → [`ProtocolId`] resolver.
    pub fn parse_protocol_key(s: &str) -> Option<ProtocolId> {
        registry::ProtocolRegistry::global().resolve_alias(s)
    }

    /// Build from a provider DB row.  The `endpoints` Vec preserves the
    /// iteration order of the JSON map (serde_json preserves insertion order).
    pub fn from_provider(provider: &Provider) -> Self {
        let raw_map = provider.parsed_protocol_endpoints();
        let mut seen = std::collections::HashSet::new();
        let mut endpoints: Vec<(ProtocolId, ProtocolEndpointEntry)> = Vec::new();

        for (key, entry) in &raw_map {
            if let Some(id) = Self::parse_protocol_key(key)
                && seen.insert(id) {
                    endpoints.push((id, entry.clone()));
                }
        }

        let declared_default = Self::parse_protocol_key(provider.effective_default_protocol());
        let default = declared_default
            .filter(|id| endpoints.iter().any(|(eid, _)| eid == id))
            .or_else(|| endpoints.first().map(|(id, _)| *id))
            .or(declared_default)
            .unwrap_or(ids::OPENAI_CHAT_V1);

        Self { default, endpoints }
    }

    /// Returns `true` if the provider declares support for `protocol`.
    pub fn supports(&self, protocol: ProtocolId) -> bool {
        self.endpoints.iter().any(|(id, _)| *id == protocol)
    }

    /// Look up the endpoint entry for a specific protocol.
    pub fn get(&self, protocol: ProtocolId) -> Option<&ProtocolEndpointEntry> {
        self.endpoints.iter().find_map(|(id, ep)| if *id == protocol { Some(ep) } else { None })
    }

    /// Deterministic three-tier egress resolution:
    ///
    /// 1. **Exact match** — ingress protocol declared by the provider.
    /// 2. **Same-family, first declared** — iterates `endpoints` in Vec order,
    ///    which is JSON insertion order after normalization.  Deterministic.
    /// 3. **Provider default** — last resort.
    pub fn resolve_egress(&self, ingress: ProtocolId) -> ResolvedEgress {
        // Tier 1: exact match.
        if let Some(ep) = self.get(ingress) {
            return ResolvedEgress {
                protocol: ingress,
                base_url: ep.base_url.clone(),
                needs_conversion: false,
            };
        }

        // Tier 2: same family, first in declared order.
        if let Some((id, ep)) = self.endpoints.iter().find(|(id, _)| id.family == ingress.family) {
            return ResolvedEgress {
                protocol: *id,
                base_url: ep.base_url.clone(),
                needs_conversion: true,
            };
        }

        // Tier 3: provider default.
        if let Some(ep) = self.get(self.default) {
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
