//! Distributed `ProtocolHandler` registration via the `inventory` crate.
//!
//! Each `protocol/handler/<family>/<dialect>.rs` module emits one
//! `inventory::submit!` block. `ProtocolRegistry::global()` walks the
//! collected registrations once, indexes them by `ProtocolId` and ingress
//! route, and exposes alias resolution for human-friendly inputs.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use crate::protocol::ids::{
    ANTHROPIC_MESSAGES_2023_06_01, GOOGLE_GENERATE_V1BETA, OPENAI_CHAT_V1, OPENAI_EMBEDDINGS_V1,
    OPENAI_RESPONSES_V1, ProtocolFamily, ProtocolId,
};
use crate::protocol::traits::ProtocolHandler;

/// `inventory::submit!` payload. Each registered handler ships one of these.
pub struct ProtocolRegistration {
    pub make: fn() -> Box<dyn ProtocolHandler>,
}

inventory::collect!(ProtocolRegistration);

/// Global registry of protocol handlers, alias table, and ingress route index.
pub struct ProtocolRegistry {
    by_id: HashMap<ProtocolId, Arc<dyn ProtocolHandler>>,
    aliases: HashMap<&'static str, ProtocolId>,
    routes: Vec<RouteEntry>,
}

struct RouteEntry {
    method: &'static str,
    path: &'static str,
    id: ProtocolId,
}

impl ProtocolRegistry {
    pub fn global() -> &'static Self {
        static REG: OnceLock<ProtocolRegistry> = OnceLock::new();
        REG.get_or_init(Self::build)
    }

    fn build() -> Self {
        let mut by_id: HashMap<ProtocolId, Arc<dyn ProtocolHandler>> = HashMap::new();
        let mut routes: Vec<RouteEntry> = Vec::new();

        for reg in inventory::iter::<ProtocolRegistration> {
            let handler: Arc<dyn ProtocolHandler> = Arc::from((reg.make)());
            let id = handler.id();

            for (method, path) in handler.capabilities().ingress_routes {
                routes.push(RouteEntry { method, path, id });
            }

            if by_id.insert(id, handler).is_some() {
                tracing::warn!(
                    target: "nyro_core::protocol",
                    "duplicate ProtocolHandler registration for {id}"
                );
            }
        }

        Self {
            by_id,
            aliases: default_aliases(),
            routes,
        }
    }

    /// Look up a handler by canonical id.
    pub fn get(&self, id: &ProtocolId) -> Option<&Arc<dyn ProtocolHandler>> {
        self.by_id.get(id)
    }

    /// Resolve a string into a registered `ProtocolId`.
    ///
    /// Accepts (in priority order):
    /// 1. Canonical `family/dialect/version` form (e.g. `openai/chat/v1`)
    /// 2. Short alias from the default table (e.g. `openai-chat`)
    /// 3. Legacy enum string (e.g. `openai`, `gemini`, `openai_responses`)
    ///
    /// Returns `None` if no registered handler matches.
    pub fn resolve_alias(&self, raw: &str) -> Option<ProtocolId> {
        let key = raw.trim();
        if key.is_empty() {
            return None;
        }

        if let Some(id) = self.parse_canonical(key) {
            return Some(id);
        }

        let lower = key.to_ascii_lowercase();
        if let Some(id) = self.aliases.get(lower.as_str()) {
            return Some(*id);
        }

        None
    }

    fn parse_canonical(&self, raw: &str) -> Option<ProtocolId> {
        let parts: Vec<&str> = raw.splitn(3, '/').collect();
        if parts.len() != 3 {
            return None;
        }
        let family = parts[0].parse::<ProtocolFamily>().ok()?;
        self.by_id
            .keys()
            .find(|id| id.family == family && id.dialect == parts[1] && id.version == parts[2])
            .copied()
    }

    /// All registered handlers belonging to the given family, sorted by id.
    pub fn list_by_family(&self, family: ProtocolFamily) -> Vec<&Arc<dyn ProtocolHandler>> {
        let mut handlers: Vec<_> = self
            .by_id
            .iter()
            .filter_map(|(id, h)| if id.family == family { Some(h) } else { None })
            .collect();
        handlers.sort_by_key(|h| h.id());
        handlers
    }

    /// All registered handlers, sorted by id (for stable `list_metadata`-style outputs).
    pub fn list(&self) -> Vec<&Arc<dyn ProtocolHandler>> {
        let mut handlers: Vec<_> = self.by_id.values().collect();
        handlers.sort_by_key(|h| h.id());
        handlers
    }

    /// Resolve an HTTP ingress (method, path) to its handler.
    ///
    /// Path matching is exact — axum-style `:param` segments are matched as
    /// literals because axum already extracts params before this is called.
    pub fn find_by_ingress_route(
        &self,
        method: &str,
        path: &str,
    ) -> Option<&Arc<dyn ProtocolHandler>> {
        for entry in &self.routes {
            if entry.method.eq_ignore_ascii_case(method) && entry.path == path {
                return self.by_id.get(&entry.id);
            }
        }
        None
    }
}

/// Default alias table.
///
/// Short names follow `{family}-{dialect}` (kebab-case, no version) — same
/// shape as `ProtocolId` minus the version. Legacy values are kept so old
/// DB / yaml inputs continue to resolve until the PR4 normalization.
fn default_aliases() -> HashMap<&'static str, ProtocolId> {
    let mut m = HashMap::new();

    // Primary short names (preferred for new configs).
    m.insert("openai-chat", OPENAI_CHAT_V1);
    m.insert("openai-responses", OPENAI_RESPONSES_V1);
    m.insert("openai-embeddings", OPENAI_EMBEDDINGS_V1);
    m.insert("anthropic-messages", ANTHROPIC_MESSAGES_2023_06_01);
    m.insert("google-generate", GOOGLE_GENERATE_V1BETA);

    // Friendly aliases.
    m.insert("responses", OPENAI_RESPONSES_V1);
    m.insert("embeddings", OPENAI_EMBEDDINGS_V1);
    m.insert("claude", ANTHROPIC_MESSAGES_2023_06_01);

    // Legacy values from the soon-to-be-removed `Protocol` enum.
    m.insert("openai", OPENAI_CHAT_V1);
    m.insert("openai_responses", OPENAI_RESPONSES_V1);
    m.insert("anthropic", ANTHROPIC_MESSAGES_2023_06_01);
    m.insert("gemini", GOOGLE_GENERATE_V1BETA);

    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_five_handlers() {
        let reg = ProtocolRegistry::global();
        assert!(reg.get(&OPENAI_CHAT_V1).is_some());
        assert!(reg.get(&OPENAI_RESPONSES_V1).is_some());
        assert!(reg.get(&OPENAI_EMBEDDINGS_V1).is_some());
        assert!(reg.get(&ANTHROPIC_MESSAGES_2023_06_01).is_some());
        assert!(reg.get(&GOOGLE_GENERATE_V1BETA).is_some());
        assert_eq!(reg.list().len(), 5);
    }

    #[test]
    fn alias_table_resolves_canonical_short_and_legacy() {
        let reg = ProtocolRegistry::global();
        assert_eq!(reg.resolve_alias("openai/chat/v1"), Some(OPENAI_CHAT_V1));
        assert_eq!(reg.resolve_alias("openai-chat"), Some(OPENAI_CHAT_V1));
        assert_eq!(reg.resolve_alias("openai"), Some(OPENAI_CHAT_V1));

        assert_eq!(
            reg.resolve_alias("openai-responses"),
            Some(OPENAI_RESPONSES_V1)
        );
        assert_eq!(reg.resolve_alias("responses"), Some(OPENAI_RESPONSES_V1));
        assert_eq!(
            reg.resolve_alias("openai_responses"),
            Some(OPENAI_RESPONSES_V1)
        );

        assert_eq!(
            reg.resolve_alias("anthropic-messages"),
            Some(ANTHROPIC_MESSAGES_2023_06_01)
        );
        assert_eq!(
            reg.resolve_alias("claude"),
            Some(ANTHROPIC_MESSAGES_2023_06_01)
        );
        assert_eq!(
            reg.resolve_alias("anthropic"),
            Some(ANTHROPIC_MESSAGES_2023_06_01)
        );

        assert_eq!(
            reg.resolve_alias("google-generate"),
            Some(GOOGLE_GENERATE_V1BETA)
        );
        assert_eq!(reg.resolve_alias("gemini"), Some(GOOGLE_GENERATE_V1BETA));
    }

    #[test]
    fn alias_resolution_is_case_insensitive_and_trims() {
        let reg = ProtocolRegistry::global();
        assert_eq!(reg.resolve_alias("  OpenAI  "), Some(OPENAI_CHAT_V1));
        assert_eq!(reg.resolve_alias("GEMINI"), Some(GOOGLE_GENERATE_V1BETA));
    }

    #[test]
    fn unknown_returns_none() {
        let reg = ProtocolRegistry::global();
        assert_eq!(reg.resolve_alias(""), None);
        assert_eq!(reg.resolve_alias("nope"), None);
        assert_eq!(reg.resolve_alias("openai/nope/v1"), None);
    }

    #[test]
    fn list_by_family_groups_correctly() {
        let reg = ProtocolRegistry::global();
        let openai = reg.list_by_family(ProtocolFamily::OpenAI);
        assert_eq!(openai.len(), 3);
        assert!(openai.iter().any(|h| h.id() == OPENAI_CHAT_V1));
        assert!(openai.iter().any(|h| h.id() == OPENAI_RESPONSES_V1));
        assert!(openai.iter().any(|h| h.id() == OPENAI_EMBEDDINGS_V1));

        assert_eq!(reg.list_by_family(ProtocolFamily::Anthropic).len(), 1);
        assert_eq!(reg.list_by_family(ProtocolFamily::Google).len(), 1);
    }

    #[test]
    fn ingress_route_matches_method_and_path() {
        let reg = ProtocolRegistry::global();
        assert_eq!(
            reg.find_by_ingress_route("POST", "/v1/chat/completions")
                .map(|h| h.id()),
            Some(OPENAI_CHAT_V1)
        );
        assert_eq!(
            reg.find_by_ingress_route("POST", "/v1/responses").map(|h| h.id()),
            Some(OPENAI_RESPONSES_V1)
        );
        assert_eq!(
            reg.find_by_ingress_route("POST", "/v1/messages").map(|h| h.id()),
            Some(ANTHROPIC_MESSAGES_2023_06_01)
        );
        assert_eq!(
            reg.find_by_ingress_route("post", "/chat/completions")
                .map(|h| h.id()),
            Some(OPENAI_CHAT_V1)
        );
        assert!(reg.find_by_ingress_route("GET", "/v1/chat/completions").is_none());
    }

    #[test]
    fn capabilities_match_legacy_special_cases() {
        let reg = ProtocolRegistry::global();
        let chat = reg.get(&OPENAI_CHAT_V1).unwrap();
        let responses = reg.get(&OPENAI_RESPONSES_V1).unwrap();
        let google = reg.get(&GOOGLE_GENERATE_V1BETA).unwrap();

        assert!(!chat.capabilities().force_upstream_stream);
        assert!(responses.capabilities().force_upstream_stream);
        assert!(google.capabilities().override_model_in_body);
        assert!(!chat.capabilities().override_model_in_body);
    }
}
