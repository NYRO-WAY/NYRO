//! Two-layer protocol identity: `ProtocolFamily` + `ProtocolId(family/dialect/version)`.
//!
//! Canonical string form: `{family}/{dialect}/{wire_version}`.
//!
//! - `family`: closed enum of protocol families (`openai` / `anthropic` / `google`).
//! - `dialect`: wire-format verb/noun, no path/version (`chat` / `responses` / `messages` / `generate`).
//! - `wire_version`: schema version as the vendor labels it (`v1`, `2023-06-01`, `v1beta`).
//!
//! `ProtocolId` is `Copy` and stores `&'static str` slices — values must be const.
//! Runtime parsing of arbitrary strings into a `ProtocolId` is the responsibility of
//! `ProtocolRegistry::resolve_alias`, which returns one of the registered const ids.

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProtocolFamily {
    OpenAI,
    Anthropic,
    Google,
}

impl ProtocolFamily {
    pub const fn as_str(&self) -> &'static str {
        match self {
            ProtocolFamily::OpenAI => "openai",
            ProtocolFamily::Anthropic => "anthropic",
            ProtocolFamily::Google => "google",
        }
    }
}

impl fmt::Display for ProtocolFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProtocolFamily {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "openai" => Ok(Self::OpenAI),
            "anthropic" => Ok(Self::Anthropic),
            "google" => Ok(Self::Google),
            other => anyhow::bail!("unknown protocol family: {other}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProtocolId {
    pub family: ProtocolFamily,
    pub dialect: &'static str,
    pub version: &'static str,
}

impl ProtocolId {
    pub const fn new(family: ProtocolFamily, dialect: &'static str, version: &'static str) -> Self {
        Self {
            family,
            dialect,
            version,
        }
    }

    /// Look up the registered handler for this id.
    ///
    /// Panics only if no `inventory::submit!` registration exists — a
    /// build-time invariant covered by `tests/protocol_registry.rs`.
    pub fn handler(self) -> &'static std::sync::Arc<dyn super::traits::ProtocolHandler> {
        super::registry::ProtocolRegistry::global()
            .get(&self)
            .unwrap_or_else(|| panic!("ProtocolHandler for {self} not registered"))
    }
}

impl fmt::Display for ProtocolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}/{}", self.family, self.dialect, self.version)
    }
}

// ── Canonical const `ProtocolId` values registered in PR1. ──
//
// New dialects added in later PRs append a const here and a matching
// `inventory::submit!` in `protocol/handler/...`. The alias table in
// `registry.rs` is the only place that maps short / legacy names to
// these canonical ids.

pub const OPENAI_CHAT_V1: ProtocolId = ProtocolId::new(ProtocolFamily::OpenAI, "chat", "v1");

pub const OPENAI_RESPONSES_V1: ProtocolId =
    ProtocolId::new(ProtocolFamily::OpenAI, "responses", "v1");

pub const ANTHROPIC_MESSAGES_2023_06_01: ProtocolId =
    ProtocolId::new(ProtocolFamily::Anthropic, "messages", "2023-06-01");

pub const GOOGLE_GENERATE_V1BETA: ProtocolId =
    ProtocolId::new(ProtocolFamily::Google, "generate", "v1beta");

pub const OPENAI_EMBEDDINGS_V1: ProtocolId =
    ProtocolId::new(ProtocolFamily::OpenAI, "embeddings", "v1");

/// Vendor field policy: what happens when the codec encounters a field
/// that the provider may or may not support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VendorFieldPolicy {
    /// The provider is known to support this field.
    Supported,
    /// The provider does not support this field; it MUST be dropped silently.
    Drop,
    /// Unknown — check at runtime via vendor extension.
    Unknown,
}

/// Stream capabilities for this endpoint.
#[derive(Debug, Clone, Copy)]
pub struct StreamCaps {
    /// Endpoint can produce SSE streaming responses.
    pub server_sent_events: bool,
    /// The `usage` object is present in the final stream chunk.
    pub usage_in_stream: bool,
    /// Provider requires the body to contain `"stream": true` to stream.
    pub requires_stream_flag: bool,
}

impl StreamCaps {
    pub const DEFAULT: Self = Self {
        server_sent_events: true,
        usage_in_stream: false,
        requires_stream_flag: true,
    };
}

/// Extended static capabilities of a `ProtocolHandler` endpoint.
///
/// PR-07 adds the feature flags, vendor field policy, and stream caps
/// sections to the original `ProtocolCapabilities`.  The `lossy_default_reject`
/// flag is consumed by `negotiator::negotiate` to decide whether to reject or
/// accept lossy cross-protocol transforms.
#[derive(Debug, Clone, Copy)]
pub struct ProtocolCapabilities {
    // ── Original fields (PR-01 through PR-06) ────────────────────────────────
    pub streaming: bool,
    pub tools: bool,
    pub reasoning: bool,
    pub embeddings: bool,
    /// Force the upstream call to be made in streaming mode regardless of the
    /// client's `stream` flag. Currently only true for OpenAI Responses API.
    pub force_upstream_stream: bool,
    /// The encoder writes the actual model name into the request body rather
    /// than the URL path. Currently only true for Google Generate.
    pub override_model_in_body: bool,
    /// Ingress routes this handler claims, as `(method, path)` tuples.
    /// Used by `ProtocolRegistry::find_by_ingress_route` for declarative routing.
    pub ingress_routes: &'static [(&'static str, &'static str)],

    // ── PR-07 additions ───────────────────────────────────────────────────────
    /// Whether multimodal (vision) input is accepted.
    pub multimodal: bool,
    /// Whether the provider accepts structured output / JSON-mode requests.
    pub structured_output: bool,
    /// Whether the provider supports named function tools.
    pub function_calling: bool,
    /// Whether the provider supports parallel tool calls.
    pub parallel_tool_calls: bool,
    /// Whether the provider exposes extended reasoning / thinking.
    pub extended_reasoning: bool,
    /// Whether the provider honours the `seed` parameter for determinism.
    pub deterministic_seed: bool,
    /// Stream capabilities for this endpoint.
    pub stream: StreamCaps,
    /// Default policy for unrecognised vendor fields in the egress body.
    ///
    /// If `Drop`, the codec will silently omit unknown extras.  If this is set
    /// to `Supported`, the codec copies `VendorExtensions::passthrough_safe`
    /// verbatim.
    pub unknown_field_policy: VendorFieldPolicy,
    /// When `true`, a request requiring a lossy cross-protocol transform is
    /// **rejected** with `GatewayError::ProtocolLossyRejected` unless the
    /// route has `allow_lossy = true`.  When `false`, the lossy transform is
    /// accepted silently.
    pub lossy_default_reject: bool,
}

impl ProtocolCapabilities {
    pub const EMPTY: Self = Self {
        streaming: false,
        tools: false,
        reasoning: false,
        embeddings: false,
        force_upstream_stream: false,
        override_model_in_body: false,
        ingress_routes: &[],
        multimodal: false,
        structured_output: false,
        function_calling: false,
        parallel_tool_calls: false,
        extended_reasoning: false,
        deterministic_seed: false,
        stream: StreamCaps::DEFAULT,
        unknown_field_policy: VendorFieldPolicy::Drop,
        lossy_default_reject: true,
    };

    /// The standard set of capabilities for a typical chat-completions endpoint.
    pub const CHAT_STANDARD: Self = Self {
        streaming: true,
        tools: true,
        reasoning: false,
        embeddings: false,
        force_upstream_stream: false,
        override_model_in_body: false,
        ingress_routes: &[],
        multimodal: true,
        structured_output: true,
        function_calling: true,
        parallel_tool_calls: true,
        extended_reasoning: false,
        deterministic_seed: true,
        stream: StreamCaps {
            server_sent_events: true,
            usage_in_stream: true,
            requires_stream_flag: true,
        },
        unknown_field_policy: VendorFieldPolicy::Drop,
        lossy_default_reject: true,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_canonical_form() {
        assert_eq!(OPENAI_CHAT_V1.to_string(), "openai/chat/v1");
        assert_eq!(OPENAI_RESPONSES_V1.to_string(), "openai/responses/v1");
        assert_eq!(
            ANTHROPIC_MESSAGES_2023_06_01.to_string(),
            "anthropic/messages/2023-06-01"
        );
        assert_eq!(GOOGLE_GENERATE_V1BETA.to_string(), "google/generate/v1beta");
    }

    #[test]
    fn family_round_trip() {
        for f in [
            ProtocolFamily::OpenAI,
            ProtocolFamily::Anthropic,
            ProtocolFamily::Google,
        ] {
            assert_eq!(f.to_string().parse::<ProtocolFamily>().unwrap(), f);
        }
    }

    #[test]
    fn protocol_id_is_copy_and_hashable() {
        use std::collections::HashSet;
        let id = OPENAI_CHAT_V1;
        let copied = id;
        let mut set = HashSet::new();
        set.insert(id);
        set.insert(copied);
        assert_eq!(set.len(), 1);
    }
}
