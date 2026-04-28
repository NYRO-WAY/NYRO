//! Acceptance tests for `ProviderProtocols`.
//!
//! Covers three guarantees:
//!
//! 1. Alias-aware key parsing — old DB rows stored as `"openai"` /
//!    `"anthropic"` and new rows stored as `"openai-chat"` /
//!    `"openai/chat/v1"` / `"responses"` all resolve to the same
//!    `ProtocolId`, so the runtime tolerates the migration transition
//!    without forcing a full rewrite of `provider.protocol_endpoints`.
//! 2. Three-tier `resolve_egress` — exact id → same family → global
//!    default. The family fallback lets a client speak Responses API
//!    against a provider that only declares chat/v1 (codec layer
//!    converts).
//! 3. `ProtocolId::handler()` — `proxy/handler.rs` calls
//!    `ingress.handler().make_decoder()` on every request, so we assert
//!    it returns a registered handler for every canonical id we ship.

use nyro_core::db::models::Provider;
use nyro_core::protocol::ProviderProtocols;
use nyro_core::protocol::ids::{
    ANTHROPIC_MESSAGES_2023_06_01, GOOGLE_GENERATE_V1BETA, OPENAI_CHAT_V1, OPENAI_RESPONSES_V1,
};
use nyro_core::protocol::registry::ProtocolRegistry;
use serde_json::json;

fn provider_with_endpoints(default_protocol: &str, endpoints: serde_json::Value) -> Provider {
    Provider {
        id: "p".to_string(),
        name: "p".to_string(),
        vendor: None,
        protocol: String::new(),
        base_url: String::new(),
        default_protocol: default_protocol.to_string(),
        protocol_endpoints: serde_json::to_string(&endpoints).unwrap(),
        preset_key: None,
        channel: None,
        models_source: None,
        capabilities_source: None,
        static_models: None,
        api_key: String::new(),
        auth_mode: "api_key".to_string(),
        use_proxy: false,
        last_test_success: None,
        last_test_at: None,
        is_enabled: true,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

#[test]
fn parses_legacy_protocol_keys() {
    let provider = provider_with_endpoints(
        "openai",
        json!({
            "openai": { "base_url": "https://a.example/v1" },
            "anthropic": { "base_url": "https://b.example/v1" },
            "gemini": { "base_url": "https://c.example/v1" },
            "openai_responses": { "base_url": "https://d.example/v1" },
        }),
    );
    let pp = ProviderProtocols::from_provider(&provider);

    assert!(pp.supports(OPENAI_CHAT_V1));
    assert!(pp.supports(ANTHROPIC_MESSAGES_2023_06_01));
    assert!(pp.supports(GOOGLE_GENERATE_V1BETA));
    assert!(pp.supports(OPENAI_RESPONSES_V1));
    assert_eq!(pp.default, OPENAI_CHAT_V1);
}

#[test]
fn parses_canonical_protocol_id_keys() {
    let provider = provider_with_endpoints(
        "openai/chat/v1",
        json!({
            "openai/chat/v1": { "base_url": "https://a.example/v1" },
            "anthropic/messages/2023-06-01": { "base_url": "https://b.example/v1" },
            "google/generate/v1beta": { "base_url": "https://c.example/v1" },
        }),
    );
    let pp = ProviderProtocols::from_provider(&provider);

    assert!(pp.supports(OPENAI_CHAT_V1));
    assert!(pp.supports(ANTHROPIC_MESSAGES_2023_06_01));
    assert!(pp.supports(GOOGLE_GENERATE_V1BETA));
    assert_eq!(pp.default, OPENAI_CHAT_V1);
}

#[test]
fn parses_short_name_aliases() {
    let provider = provider_with_endpoints(
        "openai-chat",
        json!({
            "openai-chat": { "base_url": "https://a.example/v1" },
            "claude": { "base_url": "https://b.example/v1" },
            "responses": { "base_url": "https://d.example/v1" },
        }),
    );
    let pp = ProviderProtocols::from_provider(&provider);

    assert!(pp.supports(OPENAI_CHAT_V1));
    assert!(pp.supports(ANTHROPIC_MESSAGES_2023_06_01));
    assert!(pp.supports(OPENAI_RESPONSES_V1));
    assert_eq!(pp.default, OPENAI_CHAT_V1);
}

#[test]
fn resolve_egress_exact_match_skips_conversion() {
    let provider = provider_with_endpoints(
        "openai",
        json!({ "openai": { "base_url": "https://a.example/v1" } }),
    );
    let pp = ProviderProtocols::from_provider(&provider);
    let r = pp.resolve_egress(OPENAI_CHAT_V1);

    assert_eq!(r.protocol, OPENAI_CHAT_V1);
    assert_eq!(r.base_url, "https://a.example/v1");
    assert!(!r.needs_conversion);
}

#[test]
fn resolve_egress_falls_back_to_same_family() {
    // Provider only declares OpenAI Chat; client speaks Responses API.
    // Without family fallback we'd jump to whatever the global default
    // happens to be; with family fallback we stay inside the OpenAI family
    // and let the codec layer convert chat ↔ responses.
    let provider = provider_with_endpoints(
        "anthropic",
        json!({
            "openai": { "base_url": "https://a.example/v1" },
            "anthropic": { "base_url": "https://b.example/v1" },
        }),
    );
    let pp = ProviderProtocols::from_provider(&provider);
    let r = pp.resolve_egress(OPENAI_RESPONSES_V1);

    assert_eq!(r.protocol, OPENAI_CHAT_V1, "should stay in OpenAI family");
    assert_eq!(r.base_url, "https://a.example/v1");
    assert!(r.needs_conversion);
}

#[test]
fn resolve_egress_falls_back_to_global_default_when_family_missing() {
    let provider = provider_with_endpoints(
        "openai",
        json!({ "openai": { "base_url": "https://a.example/v1" } }),
    );
    let pp = ProviderProtocols::from_provider(&provider);
    // Anthropic ingress, no Anthropic endpoint → fall back to default.
    let r = pp.resolve_egress(ANTHROPIC_MESSAGES_2023_06_01);

    assert_eq!(r.protocol, OPENAI_CHAT_V1);
    assert!(r.needs_conversion);
}

#[test]
fn protocol_handler_resolves_for_every_canonical_id() {
    let reg = ProtocolRegistry::global();

    for id in [
        OPENAI_CHAT_V1,
        OPENAI_RESPONSES_V1,
        ANTHROPIC_MESSAGES_2023_06_01,
        GOOGLE_GENERATE_V1BETA,
    ] {
        assert!(reg.get(&id).is_some(), "no handler registered for {id}");
        assert_eq!(id.handler().id(), id);
    }
}
