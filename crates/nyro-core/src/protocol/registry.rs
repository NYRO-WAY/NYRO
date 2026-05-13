//! Distributed `EndpointHandler` registration via the `inventory` crate.
//!
//! Each `protocol/codec/<protocol>/<endpoint>.rs` module emits one
//! `inventory::submit!` block. `ProtocolRegistry::global()` walks the
//! collected registrations once, indexes them by `ProtocolEndpoint` and ingress
//! route, and exposes alias resolution for human-friendly inputs.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use crate::protocol::ids::{
    ANTHROPIC_MESSAGES_2023_06_01, GOOGLE_GENERATE_CONTENT_V1BETA, OPENAI_CHAT_COMPLETIONS_V1,
    OPENAI_EMBEDDINGS_V1, OPENAI_RESPONSES_V1, Protocol, ProtocolEndpoint,
};
use crate::protocol::traits::EndpointHandler;

/// `inventory::submit!` payload. Each registered handler ships one of these.
pub struct EndpointRegistration {
    pub make: fn() -> Box<dyn EndpointHandler>,
}

/// Backward-compat alias — prefer `EndpointRegistration`.
pub type ProtocolRegistration = EndpointRegistration;

inventory::collect!(EndpointRegistration);

/// Global registry of protocol handlers, alias table, and ingress route index.
pub struct ProtocolRegistry {
    by_id: HashMap<ProtocolEndpoint, Arc<dyn EndpointHandler>>,
    endpoint_aliases: HashMap<&'static str, ProtocolEndpoint>,
    protocol_aliases: HashMap<&'static str, Protocol>,
    routes: Vec<RouteEntry>,
}

struct RouteEntry {
    method: &'static str,
    path: &'static str,
    id: ProtocolEndpoint,
}

impl ProtocolRegistry {
    pub fn global() -> &'static Self {
        static REG: OnceLock<ProtocolRegistry> = OnceLock::new();
        REG.get_or_init(Self::build)
    }

    fn build() -> Self {
        let mut by_id: HashMap<ProtocolEndpoint, Arc<dyn EndpointHandler>> = HashMap::new();
        let mut routes: Vec<RouteEntry> = Vec::new();

        for reg in inventory::iter::<EndpointRegistration> {
            let handler: Arc<dyn EndpointHandler> = Arc::from((reg.make)());
            let id = handler.id();

            for (method, path) in handler.capabilities().ingress_routes {
                routes.push(RouteEntry { method, path, id });
            }

            if by_id.insert(id, handler).is_some() {
                tracing::warn!(
                    target: "nyro_core::protocol",
                    "duplicate EndpointHandler registration for {id}"
                );
            }
        }

        Self {
            by_id,
            endpoint_aliases: default_endpoint_aliases(),
            protocol_aliases: default_protocol_aliases(),
            routes,
        }
    }

    /// Look up a handler by canonical endpoint id.
    pub fn get(&self, id: &ProtocolEndpoint) -> Option<&Arc<dyn EndpointHandler>> {
        self.by_id.get(id)
    }

    /// Resolve a string into a registered `ProtocolEndpoint`.
    ///
    /// Accepts (in priority order):
    /// 1. New canonical `protocol/name/version` form (e.g. `openai-compat/chat-completions/v1`)
    /// 2. Old canonical `family/dialect/version` form (e.g. `openai/chat/v1`) — via alias table
    /// 3. Short alias from the alias table (e.g. `openai-chat-completions`)
    /// 4. Legacy enum string (e.g. `openai`, `gemini`, `openai_responses`)
    ///
    /// Returns `None` if no registered handler matches.
    pub fn resolve_alias(&self, raw: &str) -> Option<ProtocolEndpoint> {
        let key = raw.trim();
        if key.is_empty() {
            return None;
        }

        if let Some(id) = self.parse_canonical(key) {
            return Some(id);
        }

        let lower = key.to_ascii_lowercase();
        if let Some(id) = self.endpoint_aliases.get(lower.as_str()) {
            return Some(*id);
        }

        None
    }

    fn parse_canonical(&self, raw: &str) -> Option<ProtocolEndpoint> {
        let parts: Vec<&str> = raw.splitn(3, '/').collect();
        if parts.len() != 3 {
            return None;
        }
        let protocol = parts[0].parse::<Protocol>().ok()?;
        self.by_id
            .keys()
            .find(|id| id.protocol == protocol && id.name == parts[1] && id.version == parts[2])
            .copied()
    }

    /// Resolve a string into a `Protocol` (suite-level, not endpoint-level).
    pub fn parse_protocol(&self, raw: &str) -> Option<Protocol> {
        let key = raw.trim().to_ascii_lowercase();
        if key.is_empty() {
            return None;
        }
        // Try as a registered protocol alias first
        if let Some(p) = self.protocol_aliases.get(key.as_str()) {
            return Some(*p);
        }
        // Try via endpoint alias → extract protocol from it
        if let Some(ep) = self.resolve_alias(raw) {
            return Some(ep.protocol);
        }
        None
    }

    /// All registered handlers belonging to the given protocol, sorted by id.
    pub fn list_by_protocol(&self, protocol: Protocol) -> Vec<&Arc<dyn EndpointHandler>> {
        let mut handlers: Vec<_> = self
            .by_id
            .iter()
            .filter_map(|(id, h)| {
                if id.protocol == protocol {
                    Some(h)
                } else {
                    None
                }
            })
            .collect();
        handlers.sort_by_key(|h| h.id());
        handlers
    }

    /// Returns the `Protocol` for a registered `ProtocolEndpoint`, or `None` if not found.
    pub fn protocol_of(&self, id: &ProtocolEndpoint) -> Option<Protocol> {
        if self.by_id.contains_key(id) {
            Some(id.protocol)
        } else {
            None
        }
    }

    /// All distinct protocols that have at least one registered handler.
    pub fn list_protocols(&self) -> Vec<Protocol> {
        let protocols: std::collections::BTreeSet<Protocol> =
            self.by_id.keys().map(|id| id.protocol).collect();
        protocols.into_iter().collect()
    }

    /// All registered handlers, sorted by id (for stable `list_metadata`-style outputs).
    pub fn list(&self) -> Vec<&Arc<dyn EndpointHandler>> {
        let mut handlers: Vec<_> = self.by_id.values().collect();
        handlers.sort_by_key(|h| h.id());
        handlers
    }

    // ── Normalize helpers (migrated from protocol/normalize.rs) ──────────────

    /// Normalize a single protocol identifier string to its canonical
    /// `protocol/name/version` form.  Unknown strings are returned verbatim.
    pub fn normalize_string(&self, raw: &str) -> String {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return String::new();
        }
        match self.resolve_alias(trimmed) {
            Some(id) => id.to_string(),
            None => {
                tracing::warn!(
                    value = trimmed,
                    "leaving unrecognized protocol identifier unchanged"
                );
                trimmed.to_string()
            }
        }
    }

    /// Rewrite a `protocol_endpoints`-shaped JSON object into protocol-keyed form.
    ///
    /// Handles three input shapes:
    /// 1. **New protocol-keyed**: `{"openai-compat": {"base_url": "...", "endpoints": [...]}}` —
    ///    keys are normalized to canonical short names, values are preserved.
    /// 2. **Old endpoint-keyed**: `{"openai-compat/chat-completions/v1": {"base_url": "..."}}` —
    ///    merged under the protocol key; `base_url` is taken from the first endpoint for that
    ///    protocol; conflicting base_urls emit a warning (best-effort: first wins).
    /// 3. **Legacy keys**: `{"openai": {"base_url": "..."}}` — resolved to canonical protocol.
    ///
    /// Collisions within the same protocol are resolved first-writer-wins.
    pub fn normalize_endpoints_json(&self, raw: &str) -> String {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed == "{}" {
            return raw.to_string();
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            tracing::warn!(
                value = trimmed,
                "skipping protocol_endpoints normalization: invalid JSON"
            );
            return raw.to_string();
        };
        let Some(obj) = value.as_object() else {
            return raw.to_string();
        };

        // Accumulator: protocol_canonical_short → serde_json::Value (the entry object)
        let mut next: serde_json::Map<String, serde_json::Value> =
            serde_json::Map::with_capacity(obj.len());

        for (key, val) in obj {
            // First try to resolve as a ProtocolEndpoint (endpoint-keyed old format)
            if let Some(ep) = self.resolve_alias(key) {
                let protocol_key = ep.protocol.as_str().to_string();

                if next.contains_key(&protocol_key) {
                    // Protocol entry already accumulated — check base_url consistency
                    if let Some(obj_val) = next.get_mut(&protocol_key)
                        && let Some(entry_obj) = obj_val.as_object_mut()
                    {
                        let existing_url = entry_obj
                            .get("base_url")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let incoming_url = val
                            .as_object()
                            .and_then(|o| o.get("base_url"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if !existing_url.is_empty()
                            && !incoming_url.is_empty()
                            && existing_url != incoming_url
                        {
                            tracing::warn!(
                                protocol = %protocol_key,
                                existing = %existing_url,
                                incoming = %incoming_url,
                                "conflicting base_url for same protocol — keeping first"
                            );
                        }
                        // Accumulate endpoint name in the endpoints array
                        let endpoints = entry_obj
                            .entry("endpoints")
                            .or_insert(serde_json::Value::Array(vec![]));
                        if let Some(arr) = endpoints.as_array_mut() {
                            let ep_name = serde_json::Value::String(ep.name.to_string());
                            if !arr.contains(&ep_name) {
                                arr.push(ep_name);
                            }
                        }
                    }
                } else {
                    // First time we see this protocol — create the entry
                    let mut entry = if let Some(o) = val.as_object() {
                        o.clone()
                    } else {
                        serde_json::Map::new()
                    };
                    // Add the endpoint name to an endpoints array (marks it as endpoint-keyed origin)
                    let ep_name = serde_json::Value::String(ep.name.to_string());
                    entry
                        .entry("endpoints")
                        .or_insert(serde_json::Value::Array(vec![ep_name.clone()]));
                    next.insert(protocol_key, serde_json::Value::Object(entry));
                }
                continue;
            }

            // Then try to resolve as a Protocol (protocol-keyed new format)
            if let Some(protocol) = self.parse_protocol(key) {
                let protocol_key = protocol.as_str().to_string();
                next.entry(protocol_key).or_insert_with(|| val.clone());
                continue;
            }

            // Unknown key — pass through verbatim with a warning
            tracing::warn!(key = %key, "leaving unrecognized protocol_endpoints key unchanged");
            next.entry(key.clone()).or_insert_with(|| val.clone());
        }

        // Post-process: if a protocol was endpoint-keyed but ends up with all
        // endpoints of its protocol, remove the explicit endpoints list (= "all supported")
        for (protocol_key, entry_val) in next.iter_mut() {
            if let Ok(protocol) = protocol_key.parse::<Protocol>()
                && let Some(obj) = entry_val.as_object_mut()
            {
                if let Some(eps_val) = obj.get("endpoints").and_then(|v| v.as_array()) {
                    let all_for_protocol: Vec<_> = self
                        .list_by_protocol(protocol)
                        .iter()
                        .map(|h| h.id().name)
                        .collect();
                    let all_present = all_for_protocol
                        .iter()
                        .all(|n| eps_val.iter().any(|v| v.as_str() == Some(n)));
                    if all_present && all_for_protocol.len() == eps_val.len() {
                        obj.remove("endpoints");
                    }
                }
            }
        }

        serde_json::Value::Object(next).to_string()
    }

    /// Resolve an HTTP ingress (method, path) to its handler.
    ///
    /// Path matching is exact — axum-style `:param` segments are matched as
    /// literals because axum already extracts params before this is called.
    pub fn find_by_ingress_route(
        &self,
        method: &str,
        path: &str,
    ) -> Option<&Arc<dyn EndpointHandler>> {
        for entry in &self.routes {
            if entry.method.eq_ignore_ascii_case(method) && entry.path == path {
                return self.by_id.get(&entry.id);
            }
        }
        None
    }
}

/// Three-tier endpoint alias table.
///
/// Tier 1 — Old canonical strings (backward compatibility for DB / yaml data).
/// Tier 2 — Canonical short names (preferred for new configs).
/// Tier 3 — Legacy brand names (human-friendly shortcuts).
fn default_endpoint_aliases() -> HashMap<&'static str, ProtocolEndpoint> {
    let mut m = HashMap::new();

    // ── Tier 1: Old canonical (backward compat) ───────────────────────────────
    m.insert("openai/chat/v1", OPENAI_CHAT_COMPLETIONS_V1);
    m.insert("openai/embeddings/v1", OPENAI_EMBEDDINGS_V1);
    m.insert("openai/responses/v1", OPENAI_RESPONSES_V1);
    m.insert(
        "anthropic/messages/2023-06-01",
        ANTHROPIC_MESSAGES_2023_06_01,
    );
    m.insert("google/generate/v1beta", GOOGLE_GENERATE_CONTENT_V1BETA);

    // ── Tier 2: Canonical short names ─────────────────────────────────────────
    m.insert("openai-chat", OPENAI_CHAT_COMPLETIONS_V1);
    m.insert("openai-chat-completions", OPENAI_CHAT_COMPLETIONS_V1);
    m.insert("openai-responses", OPENAI_RESPONSES_V1);
    m.insert("openai-embeddings", OPENAI_EMBEDDINGS_V1);
    m.insert("anthropic-messages", ANTHROPIC_MESSAGES_2023_06_01);
    m.insert("google-generate", GOOGLE_GENERATE_CONTENT_V1BETA);
    m.insert("google-generate-content", GOOGLE_GENERATE_CONTENT_V1BETA);

    // ── Tier 3: Legacy brand / friendly aliases ────────────────────────────────
    m.insert("openai", OPENAI_CHAT_COMPLETIONS_V1);
    m.insert("openai_responses", OPENAI_RESPONSES_V1);
    m.insert("responses", OPENAI_RESPONSES_V1);
    m.insert("embeddings", OPENAI_EMBEDDINGS_V1);
    m.insert("anthropic", ANTHROPIC_MESSAGES_2023_06_01);
    m.insert("claude", ANTHROPIC_MESSAGES_2023_06_01);
    m.insert("gemini", GOOGLE_GENERATE_CONTENT_V1BETA);

    m
}

/// Protocol-level alias table.
fn default_protocol_aliases() -> HashMap<&'static str, Protocol> {
    let mut m = HashMap::new();

    // Canonical short names
    m.insert("openai-compat", Protocol::OpenAICompatible);
    m.insert("openai-resps", Protocol::OpenAIResponses);
    m.insert("anthropic-msgs", Protocol::AnthropicMessages);
    m.insert("google-genai", Protocol::GoogleGenerativeAI);

    // Full names
    m.insert("openai-compatible", Protocol::OpenAICompatible);
    m.insert("openai-responses", Protocol::OpenAIResponses);
    m.insert("anthropic-messages", Protocol::AnthropicMessages);
    m.insert("google-generative-ai", Protocol::GoogleGenerativeAI);

    // Legacy brand names
    m.insert("openai", Protocol::OpenAICompatible);
    m.insert("anthropic", Protocol::AnthropicMessages);
    m.insert("claude", Protocol::AnthropicMessages);
    m.insert("gemini", Protocol::GoogleGenerativeAI);
    m.insert("google", Protocol::GoogleGenerativeAI);

    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_five_handlers() {
        let reg = ProtocolRegistry::global();
        assert!(reg.get(&OPENAI_CHAT_COMPLETIONS_V1).is_some());
        assert!(reg.get(&OPENAI_RESPONSES_V1).is_some());
        assert!(reg.get(&OPENAI_EMBEDDINGS_V1).is_some());
        assert!(reg.get(&ANTHROPIC_MESSAGES_2023_06_01).is_some());
        assert!(reg.get(&GOOGLE_GENERATE_CONTENT_V1BETA).is_some());
        assert_eq!(reg.list().len(), 5);
    }

    #[test]
    fn alias_table_resolves_new_canonical() {
        let reg = ProtocolRegistry::global();
        // New canonical form
        assert_eq!(
            reg.resolve_alias("openai-compat/chat-completions/v1"),
            Some(OPENAI_CHAT_COMPLETIONS_V1)
        );
        assert_eq!(
            reg.resolve_alias("openai-resps/responses/v1"),
            Some(OPENAI_RESPONSES_V1)
        );
        assert_eq!(
            reg.resolve_alias("anthropic-msgs/messages/2023-06-01"),
            Some(ANTHROPIC_MESSAGES_2023_06_01)
        );
        assert_eq!(
            reg.resolve_alias("google-genai/generate-content/v1beta"),
            Some(GOOGLE_GENERATE_CONTENT_V1BETA)
        );
    }

    #[test]
    fn alias_table_resolves_old_canonical_and_short() {
        let reg = ProtocolRegistry::global();
        // Old canonical (tier 1)
        assert_eq!(
            reg.resolve_alias("openai/chat/v1"),
            Some(OPENAI_CHAT_COMPLETIONS_V1)
        );
        assert_eq!(
            reg.resolve_alias("anthropic/messages/2023-06-01"),
            Some(ANTHROPIC_MESSAGES_2023_06_01)
        );
        assert_eq!(
            reg.resolve_alias("google/generate/v1beta"),
            Some(GOOGLE_GENERATE_CONTENT_V1BETA)
        );

        // Canonical short (tier 2)
        assert_eq!(
            reg.resolve_alias("openai-chat"),
            Some(OPENAI_CHAT_COMPLETIONS_V1)
        );
        assert_eq!(
            reg.resolve_alias("openai-chat-completions"),
            Some(OPENAI_CHAT_COMPLETIONS_V1)
        );
        assert_eq!(
            reg.resolve_alias("anthropic-messages"),
            Some(ANTHROPIC_MESSAGES_2023_06_01)
        );
        assert_eq!(
            reg.resolve_alias("google-generate"),
            Some(GOOGLE_GENERATE_CONTENT_V1BETA)
        );
        assert_eq!(
            reg.resolve_alias("google-generate-content"),
            Some(GOOGLE_GENERATE_CONTENT_V1BETA)
        );

        // Legacy brand (tier 3)
        assert_eq!(
            reg.resolve_alias("openai"),
            Some(OPENAI_CHAT_COMPLETIONS_V1)
        );
        assert_eq!(
            reg.resolve_alias("anthropic"),
            Some(ANTHROPIC_MESSAGES_2023_06_01)
        );
        assert_eq!(
            reg.resolve_alias("claude"),
            Some(ANTHROPIC_MESSAGES_2023_06_01)
        );
        assert_eq!(
            reg.resolve_alias("gemini"),
            Some(GOOGLE_GENERATE_CONTENT_V1BETA)
        );
    }

    #[test]
    fn alias_resolution_is_case_insensitive_and_trims() {
        let reg = ProtocolRegistry::global();
        assert_eq!(
            reg.resolve_alias("  OpenAI  "),
            Some(OPENAI_CHAT_COMPLETIONS_V1)
        );
        assert_eq!(
            reg.resolve_alias("GEMINI"),
            Some(GOOGLE_GENERATE_CONTENT_V1BETA)
        );
    }

    #[test]
    fn unknown_returns_none() {
        let reg = ProtocolRegistry::global();
        assert_eq!(reg.resolve_alias(""), None);
        assert_eq!(reg.resolve_alias("nope"), None);
        assert_eq!(reg.resolve_alias("openai/nope/v1"), None);
    }

    #[test]
    fn list_by_protocol_groups_correctly() {
        let reg = ProtocolRegistry::global();
        let openai_compat = reg.list_by_protocol(Protocol::OpenAICompatible);
        assert_eq!(openai_compat.len(), 2);
        assert!(
            openai_compat
                .iter()
                .any(|h| h.id() == OPENAI_CHAT_COMPLETIONS_V1)
        );
        assert!(openai_compat.iter().any(|h| h.id() == OPENAI_EMBEDDINGS_V1));

        assert_eq!(reg.list_by_protocol(Protocol::OpenAIResponses).len(), 1);
        assert_eq!(reg.list_by_protocol(Protocol::AnthropicMessages).len(), 1);
        assert_eq!(reg.list_by_protocol(Protocol::GoogleGenerativeAI).len(), 1);
    }

    #[test]
    fn parse_protocol_resolves_aliases() {
        let reg = ProtocolRegistry::global();
        assert_eq!(
            reg.parse_protocol("openai-compat"),
            Some(Protocol::OpenAICompatible)
        );
        assert_eq!(
            reg.parse_protocol("openai"),
            Some(Protocol::OpenAICompatible)
        );
        assert_eq!(
            reg.parse_protocol("claude"),
            Some(Protocol::AnthropicMessages)
        );
        assert_eq!(
            reg.parse_protocol("gemini"),
            Some(Protocol::GoogleGenerativeAI)
        );
        assert_eq!(
            reg.parse_protocol("google-genai"),
            Some(Protocol::GoogleGenerativeAI)
        );
    }

    #[test]
    fn list_protocols_returns_all_four() {
        let reg = ProtocolRegistry::global();
        let protocols = reg.list_protocols();
        assert_eq!(protocols.len(), 4);
        assert!(protocols.contains(&Protocol::OpenAICompatible));
        assert!(protocols.contains(&Protocol::OpenAIResponses));
        assert!(protocols.contains(&Protocol::AnthropicMessages));
        assert!(protocols.contains(&Protocol::GoogleGenerativeAI));
    }

    #[test]
    fn ingress_route_matches_method_and_path() {
        let reg = ProtocolRegistry::global();
        assert_eq!(
            reg.find_by_ingress_route("POST", "/v1/chat/completions")
                .map(|h| h.id()),
            Some(OPENAI_CHAT_COMPLETIONS_V1)
        );
        assert_eq!(
            reg.find_by_ingress_route("POST", "/v1/responses")
                .map(|h| h.id()),
            Some(OPENAI_RESPONSES_V1)
        );
        assert_eq!(
            reg.find_by_ingress_route("POST", "/v1/messages")
                .map(|h| h.id()),
            Some(ANTHROPIC_MESSAGES_2023_06_01)
        );
        assert_eq!(
            reg.find_by_ingress_route("POST", "/v1/embeddings")
                .map(|h| h.id()),
            Some(OPENAI_EMBEDDINGS_V1)
        );
        assert!(
            reg.find_by_ingress_route("GET", "/v1/chat/completions")
                .is_none()
        );
    }

    #[test]
    fn capabilities_match_legacy_special_cases() {
        let reg = ProtocolRegistry::global();
        let chat = reg.get(&OPENAI_CHAT_COMPLETIONS_V1).unwrap();
        let responses = reg.get(&OPENAI_RESPONSES_V1).unwrap();
        let google = reg.get(&GOOGLE_GENERATE_CONTENT_V1BETA).unwrap();

        assert!(!chat.capabilities().force_upstream_stream);
        assert!(responses.capabilities().force_upstream_stream);
        assert!(google.capabilities().override_model_in_body);
        assert!(!chat.capabilities().override_model_in_body);
    }
}
