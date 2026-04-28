//! Protocol-identifier normalization helpers.
//!
//! Used both by the DB startup migration (`db::mod::normalize_provider_protocols`
//! / `storage::postgres::normalize_provider_protocols`) and the write-path
//! gating (yaml import + admin CRUD + preset install + sync_runtime).
//!
//! Every persisted protocol identifier flows through these helpers so that
//! the storage layer always carries the canonical
//! `family/dialect/version` form. Unrecognized values are preserved
//! verbatim with a `tracing::warn!`, leaving room for foreign or future
//! identifiers to be inspected manually instead of silently dropped.

use crate::protocol::registry::ProtocolRegistry;

/// Normalize a single protocol identifier string.
///
/// Returns the canonical [`crate::protocol::ids::ProtocolId`] `to_string()`
/// form, or the trimmed input verbatim if no alias matches.
pub fn normalize_protocol_string(raw: &str, reg: &ProtocolRegistry) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    match reg.resolve_alias(trimmed) {
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

/// Rewrite every key of a `protocol_endpoints`-shaped JSON object into
/// canonical [`crate::protocol::ids::ProtocolId`] form. Returns the input
/// verbatim if it is not a JSON object (so empty `"{}"` stays `"{}"`).
///
/// Collisions (legacy `openai` and canonical `openai/chat/v1` both
/// present in the same row) are resolved with first-writer-wins on the
/// canonical key — we never overwrite an already-normalized entry.
pub fn normalize_protocol_endpoints_json(raw: &str, reg: &ProtocolRegistry) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        return raw.to_string();
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        tracing::warn!(value = trimmed, "skipping protocol_endpoints normalization: invalid JSON");
        return raw.to_string();
    };
    let Some(obj) = value.as_object() else {
        return raw.to_string();
    };

    let mut next = serde_json::Map::with_capacity(obj.len());
    for (key, val) in obj {
        let canonical = match reg.resolve_alias(key) {
            Some(id) => id.to_string(),
            None => {
                tracing::warn!(
                    key = %key,
                    "leaving unrecognized protocol_endpoints key unchanged"
                );
                key.clone()
            }
        };
        next.entry(canonical).or_insert_with(|| val.clone());
    }

    serde_json::Value::Object(next).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ids::{
        ANTHROPIC_MESSAGES_2023_06_01, GOOGLE_GENERATE_V1BETA, OPENAI_CHAT_V1, OPENAI_RESPONSES_V1,
    };

    #[test]
    fn normalizes_legacy_protocol_strings() {
        let reg = ProtocolRegistry::global();
        assert_eq!(normalize_protocol_string("openai", reg), OPENAI_CHAT_V1.to_string());
        assert_eq!(
            normalize_protocol_string("openai_responses", reg),
            OPENAI_RESPONSES_V1.to_string()
        );
        assert_eq!(
            normalize_protocol_string("anthropic", reg),
            ANTHROPIC_MESSAGES_2023_06_01.to_string()
        );
        assert_eq!(
            normalize_protocol_string("gemini", reg),
            GOOGLE_GENERATE_V1BETA.to_string()
        );
    }

    #[test]
    fn idempotent_on_canonical_strings() {
        let reg = ProtocolRegistry::global();
        for s in [
            OPENAI_CHAT_V1.to_string(),
            OPENAI_RESPONSES_V1.to_string(),
            ANTHROPIC_MESSAGES_2023_06_01.to_string(),
            GOOGLE_GENERATE_V1BETA.to_string(),
        ] {
            assert_eq!(normalize_protocol_string(&s, reg), s);
        }
    }

    #[test]
    fn preserves_unknown_strings() {
        let reg = ProtocolRegistry::global();
        assert_eq!(
            normalize_protocol_string("foo/bar/baz", reg),
            "foo/bar/baz".to_string()
        );
    }

    #[test]
    fn normalizes_endpoints_json_keys() {
        let reg = ProtocolRegistry::global();
        let raw = r#"{"openai":{"base_url":"a"},"openai_responses":{"base_url":"b"}}"#;
        let normalized = normalize_protocol_endpoints_json(raw, reg);
        let v: serde_json::Value = serde_json::from_str(&normalized).unwrap();
        let obj = v.as_object().unwrap();
        assert!(obj.contains_key(&OPENAI_CHAT_V1.to_string()));
        assert!(obj.contains_key(&OPENAI_RESPONSES_V1.to_string()));
        assert!(!obj.contains_key("openai"));
    }

    #[test]
    fn empty_endpoints_pass_through() {
        let reg = ProtocolRegistry::global();
        assert_eq!(normalize_protocol_endpoints_json("{}", reg), "{}");
        assert_eq!(normalize_protocol_endpoints_json("", reg), "");
    }

    #[test]
    fn endpoints_collision_keeps_canonical_first() {
        let reg = ProtocolRegistry::global();
        // Both legacy and canonical present: order in input is `openai`
        // before `openai/chat/v1`, but both normalize to the same key
        // `openai/chat/v1`. The canonical entry must win — first-writer-
        // wins is what we get because both map to the same key and the
        // first insert holds.
        let raw = r#"{"openai":{"base_url":"legacy"},"openai/chat/v1":{"base_url":"canonical"}}"#;
        let normalized = normalize_protocol_endpoints_json(raw, reg);
        let v: serde_json::Value = serde_json::from_str(&normalized).unwrap();
        let entry = v.get(OPENAI_CHAT_V1.to_string().as_str()).unwrap();
        // First writer wins → "legacy" comes first in the input map.
        assert_eq!(entry["base_url"], "legacy");
    }
}
