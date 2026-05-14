//! Tests for `ProtocolRegistry::normalize_endpoints_json`.
//!
//! Guarantees:
//! 1. Endpoint-keyed (old) keys are promoted to protocol-keyed (new) form.
//! 2. Legacy `endpoints` sub-array inside an entry is stripped; only `base_url` survives.
//! 3. Already-canonical protocol keys are preserved as-is.
//! 4. Duplicate keys (same protocol, two aliases) keep the first `base_url`.
//! 5. Empty / `{}` input passes through unchanged.

use nyro_core::protocol::registry::ProtocolRegistry;
use serde_json::json;

fn reg() -> &'static ProtocolRegistry {
    ProtocolRegistry::global()
}

fn normalize(input: &serde_json::Value) -> serde_json::Value {
    let s = input.to_string();
    let out = reg().normalize_endpoints_json(&s);
    serde_json::from_str(&out).expect("normalize must produce valid JSON")
}

#[test]
fn endpoint_keyed_old_format_converts_to_protocol_keyed() {
    // Old DB rows used endpoint-id keys like "openai/chat/v1".
    // After normalization they should appear under the protocol short-name.
    let input = json!({
        "openai/chat/v1": { "base_url": "https://a.example/v1" },
        "anthropic/messages/2023-06-01": { "base_url": "https://b.example/v1" },
    });
    let out = normalize(&input);
    let obj = out.as_object().expect("result must be an object");
    assert!(
        obj.contains_key("openai-compat"),
        "openai-compat key expected"
    );
    assert!(
        obj.contains_key("anthropic-msgs"),
        "anthropic-msgs key expected"
    );
    assert_eq!(
        obj["openai-compat"]["base_url"].as_str().unwrap(),
        "https://a.example/v1"
    );
    assert_eq!(
        obj["anthropic-msgs"]["base_url"].as_str().unwrap(),
        "https://b.example/v1"
    );
}

#[test]
fn legacy_endpoints_array_is_stripped() {
    // Prior to Q2, entries could carry an `endpoints` sub-array that restricted
    // which API operations were supported. That field has been removed; only
    // `base_url` must survive normalization.
    let input = json!({
        "openai-compat": {
            "base_url": "https://a.example/v1",
            "endpoints": ["chat-completions", "embeddings"]
        }
    });
    let out = normalize(&input);
    let entry = &out["openai-compat"];
    assert!(entry.is_object(), "entry must be an object");
    assert_eq!(
        entry["base_url"].as_str().unwrap(),
        "https://a.example/v1",
        "base_url must be preserved"
    );
    assert!(
        entry.get("endpoints").is_none(),
        "endpoints array must be stripped after Q2"
    );
}

#[test]
fn already_canonical_protocol_keys_are_preserved() {
    let input = json!({
        "openai-compat": { "base_url": "https://a.example/v1" },
        "anthropic-msgs": { "base_url": "https://b.example/v1" },
        "google-genai": { "base_url": "https://c.example/v1" },
    });
    let out = normalize(&input);
    let obj = out.as_object().expect("object");
    assert!(obj.contains_key("openai-compat"));
    assert!(obj.contains_key("anthropic-msgs"));
    assert!(obj.contains_key("google-genai"));
}

#[test]
fn empty_and_empty_object_pass_through_unchanged() {
    assert_eq!(reg().normalize_endpoints_json(""), "");
    assert_eq!(reg().normalize_endpoints_json("{}"), "{}");
}

#[test]
fn duplicate_aliases_for_same_protocol_keep_first_base_url() {
    // "openai-chat" and "openai/chat/v1" both resolve to the same protocol
    // endpoint. Only the first `base_url` should survive.
    let input = json!({
        "openai-chat": { "base_url": "https://first.example/v1" },
        "openai/chat/v1": { "base_url": "https://second.example/v1" },
    });
    let out = normalize(&input);
    let obj = out.as_object().expect("object");
    let entry = obj.get("openai-compat").expect("openai-compat key present");
    assert_eq!(
        entry["base_url"].as_str().unwrap(),
        "https://first.example/v1",
        "first base_url must win on duplicate"
    );
    // Exactly one openai-compat key.
    let count = obj.keys().filter(|k| *k == "openai-compat").count();
    assert_eq!(count, 1, "no duplicate protocol keys after normalization");
}

#[test]
fn short_aliases_resolve_correctly() {
    // "openai", "claude", "gemini" are tier-3 legacy brand aliases.
    let input = json!({
        "openai":   { "base_url": "https://a.example/v1" },
        "claude":   { "base_url": "https://b.example/v1" },
        "gemini":   { "base_url": "https://c.example/v1" },
        "responses":{ "base_url": "https://d.example/v1" },
    });
    let out = normalize(&input);
    let obj = out.as_object().expect("object");
    assert!(obj.contains_key("openai-compat"), "openai → openai-compat");
    assert!(
        obj.contains_key("anthropic-msgs"),
        "claude → anthropic-msgs"
    );
    assert!(obj.contains_key("google-genai"), "gemini → google-genai");
    assert!(obj.contains_key("openai-resps"), "responses → openai-resps");
}
