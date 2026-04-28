//! PR2A acceptance: VendorRegistry resolves (channel → vendor → family)
//! correctly, every registered extension produces auth/url output that
//! matches the legacy `ProviderAdapter` surface, and `list_metadata()`
//! is field-equivalent to `assets/providers.json` for the three
//! vendors migrated in PR2A (`openai`, `ollama`, plus the OpenAI/codex
//! channel).

use nyro_core::auth::types::StoredCredential;
use nyro_core::db::models::Provider;
use nyro_core::protocol::ids::{
    ANTHROPIC_MESSAGES_2023_06_01, GOOGLE_GENERATE_V1BETA, OPENAI_CHAT_V1, OPENAI_RESPONSES_V1,
    ProtocolId,
};
use nyro_core::protocol::vendor::{VendorCtx, VendorRegistry, VendorScope};
use serde_json::Value;

fn make_provider(vendor: Option<&str>, channel: Option<&str>) -> Provider {
    Provider {
        id: "test".into(),
        name: "test".into(),
        vendor: vendor.map(str::to_string),
        protocol: "openai".into(),
        base_url: "https://api.example.com/v1".into(),
        default_protocol: "openai".into(),
        protocol_endpoints: "{}".into(),
        preset_key: None,
        channel: channel.map(str::to_string),
        models_source: None,
        capabilities_source: None,
        static_models: None,
        api_key: "sk-test".into(),
        auth_mode: "apikey".into(),
        use_proxy: false,
        last_test_success: None,
        last_test_at: None,
        is_enabled: true,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn ctx<'a>(
    provider: &'a Provider,
    protocol_id: ProtocolId,
    api_key: &'a str,
    actual_model: &'a str,
    credential: Option<&'a StoredCredential>,
) -> VendorCtx<'a> {
    VendorCtx {
        provider,
        protocol_id,
        api_key,
        actual_model,
        credential,
    }
}

// ── 1. Three-tier resolution ──────────────────────────────────────────────

#[test]
fn resolve_channel_scope_takes_priority() {
    let reg = VendorRegistry::global();
    let p = make_provider(Some("openai"), Some("codex"));
    let ext = reg
        .resolve(&p, OPENAI_RESPONSES_V1)
        .expect("codex channel ext");
    assert!(matches!(
        ext.scope(),
        VendorScope::Channel {
            vendor_id: "openai",
            channel_id: "codex",
        }
    ));
}

#[test]
fn resolve_falls_back_to_vendor_when_channel_unknown() {
    let reg = VendorRegistry::global();
    let p = make_provider(Some("openai"), Some("unknown-channel"));
    let ext = reg.resolve(&p, OPENAI_CHAT_V1).expect("openai vendor ext");
    assert!(matches!(
        ext.scope(),
        VendorScope::Vendor { vendor_id: "openai" }
    ));
}

#[test]
fn resolve_falls_back_to_family_when_vendor_unknown() {
    let reg = VendorRegistry::global();
    let p = make_provider(Some("unmapped-vendor"), None);
    let openai = reg.resolve(&p, OPENAI_CHAT_V1).expect("openai family");
    let anthropic = reg
        .resolve(&p, ANTHROPIC_MESSAGES_2023_06_01)
        .expect("anthropic family");
    let google = reg.resolve(&p, GOOGLE_GENERATE_V1BETA).expect("google family");

    assert!(matches!(
        openai.scope(),
        VendorScope::Family(nyro_core::protocol::ids::ProtocolFamily::OpenAI)
    ));
    assert!(matches!(
        anthropic.scope(),
        VendorScope::Family(nyro_core::protocol::ids::ProtocolFamily::Anthropic)
    ));
    assert!(matches!(
        google.scope(),
        VendorScope::Family(nyro_core::protocol::ids::ProtocolFamily::Google)
    ));
}

#[test]
fn resolve_uses_family_when_vendor_field_blank() {
    let reg = VendorRegistry::global();
    let p = make_provider(None, None);
    let ext = reg
        .resolve(&p, ANTHROPIC_MESSAGES_2023_06_01)
        .expect("family fallback");
    assert!(matches!(
        ext.scope(),
        VendorScope::Family(nyro_core::protocol::ids::ProtocolFamily::Anthropic)
    ));
}

#[test]
fn ollama_vendor_resolves_even_without_channel() {
    let reg = VendorRegistry::global();
    let p = make_provider(Some("ollama"), None);
    let ext = reg.resolve(&p, OPENAI_CHAT_V1).expect("ollama vendor");
    assert!(matches!(
        ext.scope(),
        VendorScope::Vendor { vendor_id: "ollama" }
    ));
}

// ── 2. auth_headers / build_url legacy parity ─────────────────────────────

#[test]
fn openai_family_default_emits_bearer() {
    let reg = VendorRegistry::global();
    let p = make_provider(None, None);
    let ext = reg.resolve(&p, OPENAI_CHAT_V1).unwrap();
    let h = ext.auth_headers(&ctx(&p, OPENAI_CHAT_V1, "sk-abc", "gpt-4", None));
    assert_eq!(h.get("Authorization").unwrap(), "Bearer sk-abc");
}

#[test]
fn anthropic_family_default_emits_x_api_key_and_version() {
    let reg = VendorRegistry::global();
    let p = make_provider(None, None);
    let ext = reg.resolve(&p, ANTHROPIC_MESSAGES_2023_06_01).unwrap();
    let h = ext.auth_headers(&ctx(
        &p,
        ANTHROPIC_MESSAGES_2023_06_01,
        "sk-ant",
        "claude",
        None,
    ));
    assert_eq!(h.get("x-api-key").unwrap(), "sk-ant");
    assert_eq!(h.get("anthropic-version").unwrap(), "2023-06-01");
}

#[test]
fn google_family_default_appends_key_query_param() {
    let reg = VendorRegistry::global();
    let p = make_provider(None, None);
    let ext = reg.resolve(&p, GOOGLE_GENERATE_V1BETA).unwrap();
    let c = ctx(&p, GOOGLE_GENERATE_V1BETA, "AIzaXYZ", "gemini-1.5", None);

    let url1 = ext.build_url(&c, "https://generativelanguage.googleapis.com", "/v1beta/models");
    assert_eq!(
        url1,
        "https://generativelanguage.googleapis.com/v1beta/models?key=AIzaXYZ"
    );

    let url2 = ext.build_url(
        &c,
        "https://generativelanguage.googleapis.com/v1beta",
        "/models?alt=sse",
    );
    assert_eq!(
        url2,
        "https://generativelanguage.googleapis.com/v1beta/models?alt=sse&key=AIzaXYZ"
    );
}

#[test]
fn openai_compat_strips_v1_when_base_already_has_path() {
    let reg = VendorRegistry::global();
    let p = make_provider(None, None);
    let ext = reg.resolve(&p, OPENAI_CHAT_V1).unwrap();
    let c = ctx(&p, OPENAI_CHAT_V1, "k", "m", None);

    let stripped = ext.build_url(&c, "https://api.deepseek.com/v1", "/v1/chat/completions");
    assert_eq!(stripped, "https://api.deepseek.com/v1/chat/completions");

    let preserved = ext.build_url(&c, "https://api.openai.com", "/v1/chat/completions");
    assert_eq!(preserved, "https://api.openai.com/v1/chat/completions");
}

// ── 3. list_metadata field-level equivalence with providers.json ──────────

const PROVIDERS_JSON: &str = include_str!("../assets/providers.json");

fn providers_json() -> Vec<Value> {
    let v: Value = serde_json::from_str(PROVIDERS_JSON).unwrap();
    v.as_array().unwrap().clone()
}

#[test]
fn list_metadata_covers_every_providers_json_entry() {
    let reg = VendorRegistry::global();
    let registered: std::collections::HashSet<&str> =
        reg.list_metadata().into_iter().map(|m| m.id).collect();

    let mut missing = Vec::new();
    for entry in providers_json() {
        let id = entry["id"].as_str().unwrap();
        if !registered.contains(id) {
            missing.push(id.to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "providers.json entries not migrated to vendor metadata: {missing:?}"
    );
}

#[test]
fn metadata_field_equivalence_for_every_vendor() {
    let reg = VendorRegistry::global();
    for entry in providers_json() {
        let id = entry["id"].as_str().unwrap();
        let meta = reg
            .metadata(id)
            .unwrap_or_else(|| panic!("missing vendor metadata: {id}"));
        let ours = serde_json::to_value(meta).unwrap();

        // top-level vendor fields
        for k in ["id", "label", "icon", "defaultProtocol"] {
            assert_field_eq(&ours, &entry, k, id);
        }

        // channels: structural compare
        let ours_channels = ours["channels"].as_array().unwrap();
        let theirs_channels = entry["channels"].as_array().unwrap();
        assert_eq!(
            ours_channels.len(),
            theirs_channels.len(),
            "{id}: channel count differs",
        );
        for (oc, tc) in ours_channels.iter().zip(theirs_channels.iter()) {
            let ctx = format!("{id}/{}", oc["id"].as_str().unwrap_or("?"));
            for k in [
                "id",
                "label",
                "baseUrls",
                "apiKey",
                "modelsSource",
                "capabilitiesSource",
                "authMode",
                "oauth",
                "runtime",
            ] {
                assert_field_eq(oc, tc, k, &ctx);
            }
            // staticModels is normalized: missing == empty array
            let ours_sm = oc.get("staticModels").cloned().unwrap_or(Value::Null);
            let theirs_sm = tc.get("staticModels").cloned().unwrap_or(Value::Null);
            assert!(
                static_models_equal(&ours_sm, &theirs_sm),
                "{ctx}: staticModels differs:\n  ours   = {ours_sm}\n  theirs = {theirs_sm}",
            );
        }
    }
}

fn static_models_equal(a: &Value, b: &Value) -> bool {
    let normalize = |v: &Value| -> Value {
        match v {
            Value::Null => Value::Array(vec![]),
            Value::Array(arr) => Value::Array(arr.clone()),
            other => other.clone(),
        }
    };
    normalize(a) == normalize(b)
}

fn assert_field_eq(a: &Value, b: &Value, key: &str, ctx: &str) {
    let av = a.get(key).cloned().unwrap_or(Value::Null);
    let bv = b.get(key).cloned().unwrap_or(Value::Null);
    assert_eq!(
        av, bv,
        "{ctx}: field `{key}` differs:\n  ours   = {av}\n  theirs = {bv}",
    );
}

// ── 4. Phase-2 placeholder vendors must NOT be auto-registered ────────────

#[test]
fn placeholder_vendors_are_not_registered() {
    let reg = VendorRegistry::global();
    let registered: std::collections::HashSet<&str> =
        reg.list_metadata().into_iter().map(|m| m.id).collect();

    for placeholder in ["azure-foundry", "aws-bedrock", "google-vertex"] {
        assert!(
            !registered.contains(placeholder),
            "placeholder vendor `{placeholder}` should not yet be registered"
        );
    }
}
