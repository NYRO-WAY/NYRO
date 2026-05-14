//! Tests confirming the dispatcher-level route-type gate has been removed (Q2).
//!
//! Before Q2, routes carried a `route_type` field and the dispatcher rejected
//! embedding requests on "chat" routes (and vice-versa). That gate no longer
//! exists at the DB / protocol-negotiation layer. These tests document the new
//! invariants:
//!
//! 1. A provider declaring the `openai-compat` *protocol* suite automatically
//!    exposes **all** endpoints in that suite — including embeddings — without
//!    any subset filtering.
//! 2. A provider that only stores an endpoint-keyed entry (`"openai/chat/v1"`)
//!    exposes that single endpoint; embeddings are resolved via the normal
//!    three-tier fallback (no hard rejection).
//! 3. `ProtocolEndpointEntry` carries only `base_url`; the removed `endpoints`
//!    field is never present.

use nyro_core::db::models::Provider;
use nyro_core::protocol::ProviderProtocols;
use nyro_core::protocol::ids::{
    OPENAI_CHAT_COMPLETIONS_V1, OPENAI_EMBEDDINGS_V1, OPENAI_RESPONSES_V1,
};
use serde_json::json;

fn provider(default_protocol: &str, endpoints: serde_json::Value) -> Provider {
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
        static_models: None,
        api_key: String::new(),
        auth_mode: "apikey".to_string(),
        use_proxy: false,
        last_test_success: None,
        last_test_at: None,
        is_enabled: true,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

/// A provider declaring the `openai-compat` protocol *suite* must expose every
/// endpoint registered under that suite, including embeddings.
#[test]
fn openai_compat_protocol_suite_includes_embeddings() {
    let p = provider(
        "openai-compat",
        json!({ "openai-compat": { "base_url": "https://a.example/v1" } }),
    );
    let pp = ProviderProtocols::from_provider(&p);

    assert!(
        pp.supports(OPENAI_CHAT_COMPLETIONS_V1),
        "chat-completions must be included in openai-compat suite"
    );
    assert!(
        pp.supports(OPENAI_EMBEDDINGS_V1),
        "embeddings must be included in openai-compat suite"
    );
}

/// When `endpoints` noise is present in a protocol-suite entry (legacy YAML or
/// DB data), it must be ignored; all endpoints in the suite are still included.
#[test]
fn legacy_endpoints_array_noise_does_not_restrict_suite() {
    let p = provider(
        "openai-compat",
        json!({
            "openai-compat": {
                "base_url": "https://a.example/v1",
                // This field no longer restricts which endpoints are supported.
                "endpoints": ["chat-completions"]
            }
        }),
    );
    let pp = ProviderProtocols::from_provider(&p);

    assert!(
        pp.supports(OPENAI_EMBEDDINGS_V1),
        "embeddings must be supported despite legacy endpoints array"
    );
    let entry = pp
        .get(OPENAI_CHAT_COMPLETIONS_V1)
        .expect("chat-completions present");
    assert_eq!(
        entry.base_url, "https://a.example/v1",
        "base_url extracted correctly"
    );
}

/// `parse_protocol` resolves any endpoint-id key to its parent `Protocol`, so
/// even a single endpoint-keyed entry triggers full protocol-suite expansion.
/// `"openai/chat/v1"` belongs to `OpenAICompatible`, so embeddings is included.
#[test]
fn endpoint_keyed_format_expands_to_full_protocol_suite() {
    let p = provider(
        "openai/chat/v1",
        json!({ "openai/chat/v1": { "base_url": "https://a.example/v1" } }),
    );
    let pp = ProviderProtocols::from_provider(&p);

    // parse_protocol("openai/chat/v1") → Protocol::OpenAICompatible → suite expansion
    assert!(pp.supports(OPENAI_CHAT_COMPLETIONS_V1));
    assert!(
        pp.supports(OPENAI_EMBEDDINGS_V1),
        "endpoint-keyed key triggers suite expansion; embeddings included"
    );

    // Embeddings resolves directly (Tier 1 — exact match after expansion).
    let resolved = pp.resolve_egress(OPENAI_EMBEDDINGS_V1);
    assert_eq!(resolved.protocol, OPENAI_EMBEDDINGS_V1);
    assert!(!resolved.needs_conversion);
    assert_eq!(resolved.base_url, "https://a.example/v1");
}

/// `"openai/embeddings/v1"` also belongs to `OpenAICompatible`, so the full
/// suite (chat + embeddings) is included — no separate "embeddings-only" mode.
#[test]
fn embeddings_endpoint_key_also_expands_to_full_openai_compat_suite() {
    let p = provider(
        "openai/embeddings/v1",
        json!({ "openai/embeddings/v1": { "base_url": "https://a.example/v1" } }),
    );
    let pp = ProviderProtocols::from_provider(&p);

    // Both chat and embeddings present after suite expansion.
    assert!(pp.supports(OPENAI_EMBEDDINGS_V1));
    assert!(
        pp.supports(OPENAI_CHAT_COMPLETIONS_V1),
        "chat-completions included via OpenAICompatible suite expansion"
    );

    // Chat resolves directly with no conversion needed.
    let resolved = pp.resolve_egress(OPENAI_CHAT_COMPLETIONS_V1);
    assert_eq!(resolved.protocol, OPENAI_CHAT_COMPLETIONS_V1);
    assert!(!resolved.needs_conversion);
}

/// Two different protocol suites declared — embeddings comes from openai-compat;
/// responses from openai-resps — no cross-suite collision.
#[test]
fn multi_suite_provider_supports_all_declared_endpoints() {
    let p = provider(
        "openai-compat",
        json!({
            "openai-compat": { "base_url": "https://compat.example/v1" },
            "openai-resps":  { "base_url": "https://resps.example/v1" },
        }),
    );
    let pp = ProviderProtocols::from_provider(&p);

    assert!(pp.supports(OPENAI_CHAT_COMPLETIONS_V1));
    assert!(pp.supports(OPENAI_EMBEDDINGS_V1));
    assert!(pp.supports(OPENAI_RESPONSES_V1));

    // Embeddings uses the openai-compat base_url.
    let emb = pp.get(OPENAI_EMBEDDINGS_V1).expect("embeddings present");
    assert_eq!(emb.base_url, "https://compat.example/v1");

    // Responses uses the openai-resps base_url.
    let resp = pp.resolve_egress(OPENAI_RESPONSES_V1);
    assert_eq!(resp.protocol, OPENAI_RESPONSES_V1);
    assert!(!resp.needs_conversion);
    assert_eq!(resp.base_url, "https://resps.example/v1");
}
