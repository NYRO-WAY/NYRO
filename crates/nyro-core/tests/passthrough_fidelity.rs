//! 16-cell protocol-pair fidelity contract tests (Plan 3).
//!
//! Matrix (ingress × egress):
//!
//! ```text
//!              egress:chat | responses | messages | generate
//! ingress:chat  Native     | Transform | Transform| Transform
//! ingress:resp  Transform  | Native    | Transform| Transform
//! ingress:msgs  Transform  | Transform | Native   | Transform
//! ingress:gen   Transform  | Transform | Transform| Native
//! ```
//!
//! Diagonal (Native): `negotiate()` must return `ProtocolMode::Native`;
//!   `passthrough_run()` must return the client body unchanged.
//!
//! Off-diagonal (Transform): `negotiate()` must return `ProtocolMode::Transform`
//!   or `ProtocolMode::LossyTransform`.

use std::path::PathBuf;

use async_trait::async_trait;
use nyro_core::Gateway;
use nyro_core::config::GatewayConfig;
use nyro_core::db::models::Provider;
use nyro_core::db::models::ProtocolEndpointEntry;
use nyro_core::error::GatewayError;
use nyro_core::protocol::ids::{
    ANTHROPIC_MESSAGES_2023_06_01, GOOGLE_GENERATE_V1BETA, OPENAI_CHAT_V1, OPENAI_RESPONSES_V1,
    ProtocolId,
};
use nyro_core::protocol::types::{InternalRequest, InternalResponse};
use nyro_core::protocol::ProviderProtocols;
use nyro_core::provider::inbound::InboundResponse;
use nyro_core::provider::outbound::OutboundRequest;
use nyro_core::provider::registry::VendorScope;
use nyro_core::provider::stream::ProviderStreamParser;
use nyro_core::provider::vendor::{ProviderCtx, Vendor};
use nyro_core::provider::vendor_ext::VendorCtx;
use nyro_core::proxy::context::RequestContext;
use nyro_core::proxy::planner::{ProtocolMode, negotiate};
use reqwest::header::HeaderMap;
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

async fn build_test_gateway() -> Gateway {
    let mut config = GatewayConfig::default();
    config.data_dir = PathBuf::from(std::env::temp_dir())
        .join(format!("nyro-passthrough-test-{}", Uuid::new_v4()));
    let (gw, _log_rx) = Gateway::new(config).await.expect("gateway init");
    gw
}

// ── Helper: four-protocol provider declaration ────────────────────────────────

fn all_four_decl() -> ProviderProtocols {
    let endpoints = vec![
        (OPENAI_CHAT_V1, ProtocolEndpointEntry { base_url: "https://chat.example.com".into() }),
        (OPENAI_RESPONSES_V1, ProtocolEndpointEntry { base_url: "https://responses.example.com".into() }),
        (ANTHROPIC_MESSAGES_2023_06_01, ProtocolEndpointEntry { base_url: "https://messages.example.com".into() }),
        (GOOGLE_GENERATE_V1BETA, ProtocolEndpointEntry { base_url: "https://generate.example.com".into() }),
    ];
    ProviderProtocols { default: OPENAI_CHAT_V1, endpoints }
}

fn single_decl(proto: ProtocolId, url: &str) -> ProviderProtocols {
    let endpoints = vec![(proto, ProtocolEndpointEntry { base_url: url.to_string() })];
    ProviderProtocols { default: proto, endpoints }
}

fn req_ctx(ingress: ProtocolId) -> RequestContext {
    RequestContext::new(ingress, Duration::from_secs(30))
}

// ── Minimal no-op Vendor for passthrough_run tests ────────────────────────────

struct BearerVendor(&'static str);

#[async_trait]
impl Vendor for BearerVendor {
    fn scope(&self) -> VendorScope { VendorScope::Vendor { vendor_id: self.0 } }
    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if !ctx.api_key.is_empty() {
            h.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Bearer {}", ctx.api_key))
                    .unwrap(),
            );
        }
        h
    }
    fn vendor_id(&self) -> &'static str { self.0 }
    fn supported_protocols(&self) -> &'static [ProtocolId] { &[OPENAI_CHAT_V1] }
    fn declared_request_mutations(&self) -> bool { false }
    fn declared_response_mutations(&self) -> bool { false }
    async fn build_request(
        &self,
        _req: &mut InternalRequest,
        _ctx: &ProviderCtx<'_>,
    ) -> Result<OutboundRequest, GatewayError> {
        unreachable!()
    }
    async fn parse_response(
        &self,
        _resp: InboundResponse,
        _ctx: &ProviderCtx<'_>,
    ) -> Result<InternalResponse, GatewayError> {
        unreachable!()
    }
    fn stream_parser(&self, _ctx: &ProviderCtx<'_>) -> Box<dyn ProviderStreamParser + Send> {
        unreachable!()
    }
    fn map_error(&self, status: u16, _body: Value) -> GatewayError {
        GatewayError::upstream_status(self.0, status, None)
    }
}

fn fake_provider(api_key: &str) -> Provider {
    Provider {
        id: "p".into(),
        name: "p".into(),
        vendor: Some("test".into()),
        protocol: "openai".into(),
        base_url: "https://upstream.local".into(),
        default_protocol: "openai".into(),
        protocol_endpoints: String::new(),
        preset_key: None,
        channel: Some("default".into()),
        models_source: None,
        capabilities_source: None,
        static_models: None,
        api_key: api_key.into(),
        auth_mode: "apikey".into(),
        use_proxy: false,
        last_test_success: None,
        last_test_at: None,
        is_enabled: true,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

// ── 4 diagonal cells: Native mode ─────────────────────────────────────────────

#[test]
fn diagonal_chat_chat_is_native() {
    let decl = single_decl(OPENAI_CHAT_V1, "https://api.openai.com");
    let mut ctx = req_ctx(OPENAI_CHAT_V1);
    let plan = negotiate(OPENAI_CHAT_V1, None, Some(&decl), &mut ctx).unwrap();
    assert_eq!(plan.mode, ProtocolMode::Native, "chat→chat must be Native");
    assert_eq!(plan.egress, OPENAI_CHAT_V1);
    assert!(!plan.needs_conversion);
}

#[test]
fn diagonal_responses_responses_is_native() {
    let decl = single_decl(OPENAI_RESPONSES_V1, "https://api.openai.com");
    let mut ctx = req_ctx(OPENAI_RESPONSES_V1);
    let plan = negotiate(OPENAI_RESPONSES_V1, None, Some(&decl), &mut ctx).unwrap();
    assert_eq!(plan.mode, ProtocolMode::Native, "responses→responses must be Native");
    assert_eq!(plan.egress, OPENAI_RESPONSES_V1);
    assert!(!plan.needs_conversion);
}

#[test]
fn diagonal_messages_messages_is_native() {
    let decl = single_decl(ANTHROPIC_MESSAGES_2023_06_01, "https://api.anthropic.com");
    let mut ctx = req_ctx(ANTHROPIC_MESSAGES_2023_06_01);
    let plan =
        negotiate(ANTHROPIC_MESSAGES_2023_06_01, None, Some(&decl), &mut ctx).unwrap();
    assert_eq!(plan.mode, ProtocolMode::Native, "messages→messages must be Native");
    assert_eq!(plan.egress, ANTHROPIC_MESSAGES_2023_06_01);
    assert!(!plan.needs_conversion);
}

#[test]
fn diagonal_generate_generate_is_native() {
    let decl = single_decl(GOOGLE_GENERATE_V1BETA, "https://generativelanguage.googleapis.com");
    let mut ctx = req_ctx(GOOGLE_GENERATE_V1BETA);
    let plan = negotiate(GOOGLE_GENERATE_V1BETA, None, Some(&decl), &mut ctx).unwrap();
    assert_eq!(plan.mode, ProtocolMode::Native, "generate→generate must be Native");
    assert_eq!(plan.egress, GOOGLE_GENERATE_V1BETA);
    assert!(!plan.needs_conversion);
}

// ── 12 off-diagonal cells: Transform mode ────────────────────────────────────

macro_rules! off_diagonal_test {
    ($name:ident, ingress = $ing:expr, egress = $eg:expr) => {
        #[test]
        fn $name() {
            let decl = single_decl($eg, "https://upstream.example.com");
            let mut ctx = req_ctx($ing);
            let plan = negotiate($ing, None, Some(&decl), &mut ctx).unwrap();
            assert_ne!(
                plan.mode,
                ProtocolMode::Native,
                "{} → {} should not be Native",
                $ing,
                $eg
            );
            assert!(plan.needs_conversion, "{} → {} must need conversion", $ing, $eg);
        }
    };
}

off_diagonal_test!(chat_to_responses_is_transform,    ingress = OPENAI_CHAT_V1, egress = OPENAI_RESPONSES_V1);
off_diagonal_test!(chat_to_messages_is_transform,     ingress = OPENAI_CHAT_V1, egress = ANTHROPIC_MESSAGES_2023_06_01);
off_diagonal_test!(chat_to_generate_is_transform,     ingress = OPENAI_CHAT_V1, egress = GOOGLE_GENERATE_V1BETA);

off_diagonal_test!(responses_to_chat_is_transform,    ingress = OPENAI_RESPONSES_V1, egress = OPENAI_CHAT_V1);
off_diagonal_test!(responses_to_messages_is_transform,ingress = OPENAI_RESPONSES_V1, egress = ANTHROPIC_MESSAGES_2023_06_01);
off_diagonal_test!(responses_to_generate_is_transform,ingress = OPENAI_RESPONSES_V1, egress = GOOGLE_GENERATE_V1BETA);

off_diagonal_test!(messages_to_chat_is_transform,     ingress = ANTHROPIC_MESSAGES_2023_06_01, egress = OPENAI_CHAT_V1);
off_diagonal_test!(messages_to_responses_is_transform,ingress = ANTHROPIC_MESSAGES_2023_06_01, egress = OPENAI_RESPONSES_V1);
off_diagonal_test!(messages_to_generate_is_transform, ingress = ANTHROPIC_MESSAGES_2023_06_01, egress = GOOGLE_GENERATE_V1BETA);

off_diagonal_test!(generate_to_chat_is_transform,     ingress = GOOGLE_GENERATE_V1BETA, egress = OPENAI_CHAT_V1);
off_diagonal_test!(generate_to_responses_is_transform,ingress = GOOGLE_GENERATE_V1BETA, egress = OPENAI_RESPONSES_V1);
off_diagonal_test!(generate_to_messages_is_transform, ingress = GOOGLE_GENERATE_V1BETA, egress = ANTHROPIC_MESSAGES_2023_06_01);

// ── passthrough_run body fidelity ─────────────────────────────────────────────

#[tokio::test]
async fn passthrough_run_preserves_vendor_specific_fields() {
    let gw = build_test_gateway().await;
    let provider = fake_provider("sk-test");
    let vendor = BearerVendor("test");

    // A body with vendor-specific extension fields that the IR would normally drop.
    let raw_body = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hello"}],
        "stream": false,
        "vendor_extension_field": "must-survive",
        "another_custom": {"nested": true}
    });

    let ctx = ProviderCtx {
        provider: &provider,
        protocol: OPENAI_CHAT_V1,
        egress_base_url: "https://api.openai.com",
        api_key: &provider.api_key,
        actual_model: "gpt-4o",
        credential: None,
        gw: &gw,
        disable_default_auth: false,
    };

    let out = nyro_core::provider::common::pipeline::passthrough_run(
        &vendor,
        raw_body.clone(),
        &ctx,
    )
    .await
    .expect("passthrough_run must succeed");

    // Body is forwarded verbatim (JSON-value equality).
    assert_eq!(
        out.body, raw_body,
        "passthrough_run must not modify the request body"
    );
    // Auth header must be present.
    assert!(
        out.headers.contains_key(reqwest::header::AUTHORIZATION),
        "auth header must be set"
    );
    // URL must be the chat completions path.
    assert!(
        out.url.contains("/v1/chat/completions"),
        "URL must include the egress path, got: {}",
        out.url
    );
}

#[tokio::test]
async fn passthrough_run_sets_stream_path_for_streaming_body() {
    let gw = build_test_gateway().await;
    let provider = fake_provider("sk-test");
    let vendor = BearerVendor("test");
    let raw_body = json!({ "model": "gpt-4o", "messages": [], "stream": true });

    let ctx = ProviderCtx {
        provider: &provider,
        protocol: OPENAI_CHAT_V1,
        egress_base_url: "https://api.openai.com",
        api_key: &provider.api_key,
        actual_model: "gpt-4o",
        credential: None,
        gw: &gw,
        disable_default_auth: false,
    };

    let out = nyro_core::provider::common::pipeline::passthrough_run(
        &vendor,
        raw_body.clone(),
        &ctx,
    )
    .await
    .expect("passthrough_run must succeed");

    assert_eq!(out.body, raw_body);
    assert!(out.url.contains("/v1/chat/completions"), "stream path same as non-stream for chat");
}

// ── declared_mutations default for conservative vendors ───────────────────────

#[test]
fn vendor_declared_mutations_defaults_are_conservative() {
    struct DefaultVendor;
    #[async_trait]
    impl Vendor for DefaultVendor {
        fn scope(&self) -> VendorScope { VendorScope::Vendor { vendor_id: "default" } }
        fn vendor_id(&self) -> &'static str { "default" }
        fn supported_protocols(&self) -> &'static [ProtocolId] { &[OPENAI_CHAT_V1] }
        async fn build_request(&self, _: &mut InternalRequest, _: &ProviderCtx<'_>) -> Result<OutboundRequest, GatewayError> { unreachable!() }
        async fn parse_response(&self, _: InboundResponse, _: &ProviderCtx<'_>) -> Result<InternalResponse, GatewayError> { unreachable!() }
        fn stream_parser(&self, _: &ProviderCtx<'_>) -> Box<dyn ProviderStreamParser + Send> { unreachable!() }
        fn map_error(&self, s: u16, _: Value) -> GatewayError { GatewayError::upstream_status("default", s, None) }
    }
    let v = DefaultVendor;
    assert!(v.declared_request_mutations(), "default must be conservative (true)");
    assert!(v.declared_response_mutations(), "default must be conservative (true)");
}

// ── negotiate() with all-four provider: each ingress selects exact match ──────

#[test]
fn all_four_provider_each_ingress_selects_own_native() {
    let decl = all_four_decl();

    for proto in [
        OPENAI_CHAT_V1,
        OPENAI_RESPONSES_V1,
        ANTHROPIC_MESSAGES_2023_06_01,
        GOOGLE_GENERATE_V1BETA,
    ] {
        let mut ctx = req_ctx(proto);
        let plan = negotiate(proto, None, Some(&decl), &mut ctx).unwrap();
        assert_eq!(
            plan.mode,
            ProtocolMode::Native,
            "provider supporting all 4: ingress {} must get Native",
            proto
        );
        assert_eq!(plan.egress, proto);
    }
}
