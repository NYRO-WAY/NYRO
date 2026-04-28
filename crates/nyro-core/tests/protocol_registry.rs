//! Protocol registry acceptance.
//!
//! Five handlers are registered:
//! - OpenAI Chat / OpenAI Responses (PR1)
//! - Anthropic Messages / Google Generate (PR1)
//! - OpenAI Embeddings (PR3 — passthrough codec, registered for
//!   `find_by_ingress_route` and capability discovery; the standard
//!   request pipeline still bypasses its codec)
//!
//! The codec-equivalence tests skip embeddings because the legacy
//! factory functions don't have an embeddings variant to compare
//! against — the parity guarantee for this handler is its routing /
//! capability surface, asserted below.

use nyro_core::protocol::ids::{
    ANTHROPIC_MESSAGES_2023_06_01, GOOGLE_GENERATE_V1BETA, OPENAI_CHAT_V1, OPENAI_EMBEDDINGS_V1,
    OPENAI_RESPONSES_V1, ProtocolFamily, ProtocolId,
};
use nyro_core::protocol::registry::ProtocolRegistry;
use nyro_core::protocol::types::Role;
use nyro_core::protocol::{
    get_decoder, get_encoder, get_response_formatter, get_response_parser, get_stream_formatter,
    get_stream_parser, Protocol,
};
use serde_json::json;

#[test]
fn registers_all_handlers_with_correct_ids() {
    let reg = ProtocolRegistry::global();
    assert_eq!(reg.list().len(), 5);

    for id in [
        OPENAI_CHAT_V1,
        OPENAI_RESPONSES_V1,
        OPENAI_EMBEDDINGS_V1,
        ANTHROPIC_MESSAGES_2023_06_01,
        GOOGLE_GENERATE_V1BETA,
    ] {
        let h = reg.get(&id).unwrap_or_else(|| panic!("missing {id}"));
        assert_eq!(h.id(), id);
    }
}

#[test]
fn list_by_family_segments() {
    let reg = ProtocolRegistry::global();
    assert_eq!(reg.list_by_family(ProtocolFamily::OpenAI).len(), 3);
    assert_eq!(reg.list_by_family(ProtocolFamily::Anthropic).len(), 1);
    assert_eq!(reg.list_by_family(ProtocolFamily::Google).len(), 1);
}

#[test]
fn ingress_routes_match_axum_router() {
    let reg = ProtocolRegistry::global();

    let cases: &[(&str, &str, ProtocolId)] = &[
        ("POST", "/v1/chat/completions", OPENAI_CHAT_V1),
        ("POST", "/chat/completions", OPENAI_CHAT_V1),
        ("POST", "/v1/responses", OPENAI_RESPONSES_V1),
        ("POST", "/responses", OPENAI_RESPONSES_V1),
        ("POST", "/v1/messages", ANTHROPIC_MESSAGES_2023_06_01),
        ("POST", "/messages", ANTHROPIC_MESSAGES_2023_06_01),
        (
            "POST",
            "/v1beta/models/:model_action",
            GOOGLE_GENERATE_V1BETA,
        ),
        ("POST", "/models/:model_action", GOOGLE_GENERATE_V1BETA),
    ];

    for (method, path, expected) in cases {
        let h = reg
            .find_by_ingress_route(method, path)
            .unwrap_or_else(|| panic!("no handler for {method} {path}"));
        assert_eq!(h.id(), *expected, "wrong handler for {method} {path}");
    }
}

#[test]
fn capabilities_match_legacy_special_cases() {
    let reg = ProtocolRegistry::global();
    assert!(
        reg.get(&OPENAI_RESPONSES_V1)
            .unwrap()
            .capabilities()
            .force_upstream_stream,
        "Responses must force upstream streaming (legacy matches!(egress, ResponsesAPI))"
    );
    assert!(
        !reg.get(&OPENAI_CHAT_V1)
            .unwrap()
            .capabilities()
            .force_upstream_stream
    );
    assert!(
        reg.get(&GOOGLE_GENERATE_V1BETA)
            .unwrap()
            .capabilities()
            .override_model_in_body,
        "Google must override model in body (legacy Gemini branch)"
    );
}

// ── Equivalence with legacy factory functions ──

fn pair(id: ProtocolId, legacy: Protocol) -> (ProtocolId, Protocol) {
    (id, legacy)
}

fn all_pairs() -> Vec<(ProtocolId, Protocol)> {
    vec![
        pair(OPENAI_CHAT_V1, Protocol::OpenAI),
        pair(OPENAI_RESPONSES_V1, Protocol::ResponsesAPI),
        pair(ANTHROPIC_MESSAGES_2023_06_01, Protocol::Anthropic),
        pair(GOOGLE_GENERATE_V1BETA, Protocol::Gemini),
    ]
}

fn sample_body(legacy: Protocol) -> serde_json::Value {
    match legacy {
        Protocol::OpenAI => json!({
            "model": "gpt-4o-mini",
            "messages": [
                {"role": "system", "content": "be helpful"},
                {"role": "user", "content": "hi"}
            ],
            "stream": false,
            "temperature": 0.5
        }),
        Protocol::ResponsesAPI => json!({
            "model": "gpt-4o-mini",
            "instructions": "be helpful",
            "input": [
                {"role": "user", "content": [{"type": "input_text", "text": "hi"}]}
            ],
            "stream": false,
            "temperature": 0.5
        }),
        Protocol::Anthropic => json!({
            "model": "claude-3-5-sonnet",
            "system": "be helpful",
            "messages": [
                {"role": "user", "content": "hi"}
            ],
            "max_tokens": 256,
            "stream": false
        }),
        Protocol::Gemini => json!({
            "system_instruction": {"parts": [{"text": "be helpful"}]},
            "contents": [
                {"role": "user", "parts": [{"text": "hi"}]}
            ]
        }),
    }
}

#[test]
fn decoder_output_equivalent_to_legacy_factory() {
    let reg = ProtocolRegistry::global();
    for (id, legacy) in all_pairs() {
        let body = sample_body(legacy);

        let new_req = reg
            .get(&id)
            .unwrap()
            .make_decoder()
            .decode_request(body.clone())
            .unwrap_or_else(|e| panic!("new decoder failed for {id}: {e}"));
        let old_req = get_decoder(legacy)
            .decode_request(body)
            .unwrap_or_else(|e| panic!("legacy decoder failed for {legacy}: {e}"));

        assert_eq!(new_req.model, old_req.model, "model mismatch for {id}");
        assert_eq!(
            new_req.messages.len(),
            old_req.messages.len(),
            "message count mismatch for {id}"
        );
        assert_eq!(new_req.stream, old_req.stream, "stream mismatch for {id}");
        assert_eq!(
            new_req.source_protocol, old_req.source_protocol,
            "source_protocol mismatch for {id}"
        );

        let roles_new: Vec<Role> = new_req.messages.iter().map(|m| m.role).collect();
        let roles_old: Vec<Role> = old_req.messages.iter().map(|m| m.role).collect();
        assert_eq!(roles_new, roles_old, "role sequence mismatch for {id}");
    }
}

#[test]
fn encoder_output_equivalent_to_legacy_factory() {
    let reg = ProtocolRegistry::global();
    for (id, legacy) in all_pairs() {
        let body = sample_body(legacy);

        let internal = get_decoder(legacy).decode_request(body).unwrap();

        let (new_body, new_headers) = reg
            .get(&id)
            .unwrap()
            .make_encoder()
            .encode_request(&internal)
            .unwrap_or_else(|e| panic!("new encoder failed for {id}: {e}"));
        let (old_body, old_headers) = get_encoder(legacy)
            .encode_request(&internal)
            .unwrap_or_else(|e| panic!("legacy encoder failed for {legacy}: {e}"));

        assert_eq!(new_body, old_body, "encoded body mismatch for {id}");

        let mut new_keys: Vec<_> = new_headers.keys().map(|k| k.as_str().to_string()).collect();
        let mut old_keys: Vec<_> = old_headers.keys().map(|k| k.as_str().to_string()).collect();
        new_keys.sort();
        old_keys.sort();
        assert_eq!(new_keys, old_keys, "header keys mismatch for {id}");

        let new_path = reg
            .get(&id)
            .unwrap()
            .make_encoder()
            .egress_path(&internal.model, internal.stream);
        let old_path = get_encoder(legacy).egress_path(&internal.model, internal.stream);
        assert_eq!(new_path, old_path, "egress_path mismatch for {id}");
    }
}

#[test]
fn parser_and_formatter_factories_construct() {
    // Smoke: verify the four factory methods don't panic and the produced
    // trait objects implement the expected interface (compile-time check).
    let reg = ProtocolRegistry::global();
    for (id, _) in all_pairs() {
        let h = reg.get(&id).unwrap();
        let _ = h.make_response_parser();
        let _ = h.make_response_formatter();
        let _ = h.make_stream_parser();
        let _ = h.make_stream_formatter();

        // Cross-check: legacy factories also construct.
        let _ = get_response_parser(legacy_for(id));
        let _ = get_response_formatter(legacy_for(id));
        let _ = get_stream_parser(legacy_for(id));
        let _ = get_stream_formatter(legacy_for(id));
    }
}

// ── Embeddings (PR3) — registered for routing/capability surface only ──

#[test]
fn embeddings_handler_advertises_passthrough_capabilities() {
    let reg = ProtocolRegistry::global();
    let caps = reg.get(&OPENAI_EMBEDDINGS_V1).unwrap().capabilities();
    assert!(caps.embeddings);
    assert!(!caps.streaming);
    assert!(!caps.tools);
    assert!(!caps.force_upstream_stream);
    assert!(!caps.override_model_in_body);
}

#[test]
fn embeddings_routes_resolve_to_handler() {
    let reg = ProtocolRegistry::global();
    for path in ["/v1/embeddings", "/embeddings"] {
        let h = reg
            .find_by_ingress_route("POST", path)
            .unwrap_or_else(|| panic!("no handler for POST {path}"));
        assert_eq!(h.id(), OPENAI_EMBEDDINGS_V1);
    }
}

#[test]
fn embeddings_aliases_resolve() {
    let reg = ProtocolRegistry::global();
    assert_eq!(
        reg.resolve_alias("openai-embeddings"),
        Some(OPENAI_EMBEDDINGS_V1)
    );
    assert_eq!(reg.resolve_alias("embeddings"), Some(OPENAI_EMBEDDINGS_V1));
    assert_eq!(
        reg.resolve_alias("openai/embeddings/v1"),
        Some(OPENAI_EMBEDDINGS_V1)
    );
}

#[test]
fn embeddings_decoder_round_trips_body() {
    let reg = ProtocolRegistry::global();
    let body = json!({
        "model": "text-embedding-3-small",
        "input": ["hello", "world"],
        "encoding_format": "float"
    });
    let internal = reg
        .get(&OPENAI_EMBEDDINGS_V1)
        .unwrap()
        .make_decoder()
        .decode_request(body.clone())
        .unwrap();
    assert_eq!(internal.model, "text-embedding-3-small");
    assert!(!internal.stream);

    let (encoded, _headers) = reg
        .get(&OPENAI_EMBEDDINGS_V1)
        .unwrap()
        .make_encoder()
        .encode_request(&internal)
        .unwrap();
    assert_eq!(encoded, body, "encoder must round-trip the original body");
}

fn legacy_for(id: ProtocolId) -> Protocol {
    if id == OPENAI_CHAT_V1 {
        Protocol::OpenAI
    } else if id == OPENAI_RESPONSES_V1 {
        Protocol::ResponsesAPI
    } else if id == ANTHROPIC_MESSAGES_2023_06_01 {
        Protocol::Anthropic
    } else if id == GOOGLE_GENERATE_V1BETA {
        Protocol::Gemini
    } else {
        panic!("no legacy mapping for {id}")
    }
}
