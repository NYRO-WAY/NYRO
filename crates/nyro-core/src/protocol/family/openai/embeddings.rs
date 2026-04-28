//! OpenAI Embeddings API (`POST /v1/embeddings`).
//!
//! Embeddings is a passthrough endpoint — the gateway forwards the
//! request body verbatim and only inspects `usage.prompt_tokens` from
//! the response. We still register a `ProtocolHandler` so that:
//!
//! 1. `ProtocolRegistry::find_by_ingress_route` resolves
//!    `POST /v1/embeddings` (and `/embeddings`) to a known
//!    `ProtocolId` — same routing model as chat / responses /
//!    messages.
//! 2. `capabilities()` advertises `embeddings = true` (and
//!    `streaming`, `tools`, `force_upstream_stream` = `false`) so call
//!    sites branch declaratively.
//! 3. Vendor extension lookup goes through the same `(provider,
//!    protocol_id)` pair as chat completions.
//!
//! The decoder / encoder are minimal passthroughs that round-trip the
//! original body via [`EMBEDDINGS_BODY_KEY`] in
//! `InternalRequest.extra`. The response parser / formatter and stream
//! traits are unreachable: `proxy::handler::embeddings_proxy` short-
//! circuits before invoking them, and `capabilities().streaming =
//! false` guards against accidental stream use. They `unreachable!`
//! loudly if a future refactor sends embeddings through the standard
//! codec pipeline without revisiting this module.

use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::protocol::ids::{OPENAI_EMBEDDINGS_V1, ProtocolCapabilities, ProtocolId};
use crate::protocol::registry::ProtocolRegistration;
use crate::protocol::traits::*;
use crate::protocol::types::{InternalRequest, InternalResponse, StreamDelta, TokenUsage};
use crate::protocol::{Protocol, SseEvent};

pub const EMBEDDINGS_BODY_KEY: &str = "__embeddings_passthrough_body__";

const CAPS: ProtocolCapabilities = ProtocolCapabilities {
    streaming: false,
    tools: false,
    reasoning: false,
    embeddings: true,
    force_upstream_stream: false,
    override_model_in_body: false,
    ingress_routes: &[
        ("POST", "/v1/embeddings"),
        ("POST", "/embeddings"),
    ],
};

pub struct OpenAIEmbeddingsV1;

impl ProtocolHandler for OpenAIEmbeddingsV1 {
    fn id(&self) -> ProtocolId {
        OPENAI_EMBEDDINGS_V1
    }
    fn capabilities(&self) -> &'static ProtocolCapabilities {
        &CAPS
    }
    fn make_decoder(&self) -> Box<dyn IngressDecoder + Send> {
        Box::new(EmbeddingsPassthroughDecoder)
    }
    fn make_encoder(&self) -> Box<dyn EgressEncoder + Send> {
        Box::new(EmbeddingsPassthroughEncoder)
    }
    fn make_response_parser(&self) -> Box<dyn ResponseParser> {
        Box::new(EmbeddingsResponseParser)
    }
    fn make_response_formatter(&self) -> Box<dyn ResponseFormatter> {
        Box::new(EmbeddingsResponseFormatter)
    }
    fn make_stream_parser(&self) -> Box<dyn StreamParser> {
        Box::new(EmbeddingsStreamParser)
    }
    fn make_stream_formatter(&self) -> Box<dyn StreamFormatter> {
        Box::new(EmbeddingsStreamFormatter)
    }
}

inventory::submit! {
    ProtocolRegistration { make: || Box::new(OpenAIEmbeddingsV1) }
}

struct EmbeddingsPassthroughDecoder;

impl IngressDecoder for EmbeddingsPassthroughDecoder {
    fn decode_request(&self, body: Value) -> anyhow::Result<InternalRequest> {
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(ToString::to_string)
            .ok_or_else(|| anyhow::anyhow!("model is required"))?;

        let mut extra = std::collections::HashMap::new();
        extra.insert(EMBEDDINGS_BODY_KEY.to_string(), body);

        Ok(InternalRequest {
            messages: Vec::new(),
            model,
            stream: false,
            temperature: None,
            max_tokens: None,
            top_p: None,
            tools: None,
            tool_choice: None,
            source_protocol: Protocol::OpenAI,
            extra,
        })
    }
}

struct EmbeddingsPassthroughEncoder;

impl EgressEncoder for EmbeddingsPassthroughEncoder {
    fn encode_request(&self, req: &InternalRequest) -> anyhow::Result<(Value, HeaderMap)> {
        let body = req
            .extra
            .get(EMBEDDINGS_BODY_KEY)
            .cloned()
            .unwrap_or_else(|| serde_json::json!({ "model": req.model }));
        Ok((body, HeaderMap::new()))
    }

    fn egress_path(&self, _model: &str, _stream: bool) -> String {
        "/v1/embeddings".to_string()
    }
}

struct EmbeddingsResponseParser;

impl ResponseParser for EmbeddingsResponseParser {
    fn parse_response(&self, _resp: Value) -> anyhow::Result<InternalResponse> {
        unreachable!(
            "embeddings_proxy bypasses the codec pipeline; \
             revisit family/openai/embeddings.rs before routing through it"
        )
    }
}

struct EmbeddingsResponseFormatter;

impl ResponseFormatter for EmbeddingsResponseFormatter {
    fn format_response(&self, _resp: &InternalResponse) -> Value {
        unreachable!(
            "embeddings_proxy bypasses the codec pipeline; \
             revisit family/openai/embeddings.rs before routing through it"
        )
    }
}

struct EmbeddingsStreamParser;

impl StreamParser for EmbeddingsStreamParser {
    fn parse_chunk(&mut self, _raw: &str) -> anyhow::Result<Vec<StreamDelta>> {
        unreachable!("embeddings has streaming=false; check capabilities before calling")
    }
    fn finish(&mut self) -> anyhow::Result<Vec<StreamDelta>> {
        unreachable!("embeddings has streaming=false; check capabilities before calling")
    }
}

struct EmbeddingsStreamFormatter;

impl StreamFormatter for EmbeddingsStreamFormatter {
    fn format_deltas(&mut self, _deltas: &[StreamDelta]) -> Vec<SseEvent> {
        unreachable!("embeddings has streaming=false; check capabilities before calling")
    }
    fn format_done(&mut self) -> Vec<SseEvent> {
        unreachable!("embeddings has streaming=false; check capabilities before calling")
    }
    fn usage(&self) -> TokenUsage {
        TokenUsage::default()
    }
}

/// Pull `usage.prompt_tokens` out of an OpenAI embeddings response.
/// Shared with `proxy::handler::embeddings_proxy` so the passthrough
/// path and any future codec route agree on accounting.
pub fn parse_usage(payload: &Value) -> TokenUsage {
    let prompt = payload
        .get("usage")
        .and_then(|u| u.get("prompt_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    TokenUsage {
        input_tokens: prompt.max(0) as u32,
        output_tokens: 0,
    }
}
