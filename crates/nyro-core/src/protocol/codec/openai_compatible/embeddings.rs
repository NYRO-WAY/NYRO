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
//! # PR-12: full field mapping + vendor extensions 三段化
//!
//! The decoder now explicitly parses every known field and segregates
//! vendor-specific (unknown) fields into `__vendor_ingress` following
//! the three-segment pattern (`ingress / egress / passthrough_safe`).
//! The encoder reconstructs the upstream body from the explicit keys,
//! forwarding only the fields allowed by `VendorFieldPolicy`.
//!
//! The original passthrough body is preserved in [`EMBEDDINGS_BODY_KEY`]
//! as a safety-net fallback so that no data is lost if the decoder
//! encounters an unexpected body shape.

use std::collections::HashMap;

use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::protocol::ids::{OPENAI_EMBEDDINGS_V1, EndpointCapabilities, ProtocolEndpoint};
use crate::protocol::registry::EndpointRegistration;
use crate::protocol::traits::*;
use crate::protocol::types::{InternalRequest, InternalResponse, StreamDelta, TokenUsage};
use crate::protocol::SseEvent;

/// Key under which the complete original request body is kept as a
/// fallback (used by `embeddings_proxy` in handler.rs).
pub const EMBEDDINGS_BODY_KEY: &str = "__embeddings_passthrough_body__";

/// OpenAI-spec field names for the embeddings endpoint.
const KNOWN_EMBEDDINGS_FIELDS: &[&str] = &[
    "model",
    "input",
    "dimensions",
    "encoding_format",
    "user",
];

const CAPS: EndpointCapabilities = EndpointCapabilities {
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
    multimodal: false,
    structured_output: false,
    function_calling: false,
    parallel_tool_calls: false,
    extended_reasoning: false,
    deterministic_seed: false,
    stream: crate::protocol::ids::StreamCaps::DEFAULT,
    unknown_field_policy: crate::protocol::ids::VendorFieldPolicy::Drop,
    lossy_default_reject: true,
};

pub struct OpenAIEmbeddingsV1;

impl EndpointHandler for OpenAIEmbeddingsV1 {
    fn id(&self) -> ProtocolEndpoint {
        OPENAI_EMBEDDINGS_V1
    }
    fn capabilities(&self) -> &'static EndpointCapabilities {
        &CAPS
    }
    fn make_decoder(&self) -> Box<dyn IngressDecoder + Send> {
        Box::new(EmbeddingsDecoder)
    }
    fn make_encoder(&self) -> Box<dyn EgressEncoder + Send> {
        Box::new(EmbeddingsEncoder)
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
    EndpointRegistration { make: || Box::new(OpenAIEmbeddingsV1) }
}

// ── Decoder ───────────────────────────────────────────────────────────────────

struct EmbeddingsDecoder;

impl IngressDecoder for EmbeddingsDecoder {
    fn decode_request(&self, body: Value) -> anyhow::Result<InternalRequest> {
        let obj = body
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("embeddings request must be a JSON object"))?;

        let model = obj
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(ToString::to_string)
            .ok_or_else(|| anyhow::anyhow!("model is required for embeddings"))?;

        let mut extra: HashMap<String, Value> = HashMap::new();

        // Keep the original body so `embeddings_proxy` can forward it.
        extra.insert(EMBEDDINGS_BODY_KEY.to_string(), body.clone());

        // ── Parse known fields ────────────────────────────────────────────────
        // `input` can be: string | string[] | integer[] | integer[][]
        if let Some(input) = obj.get("input") {
            extra.insert("__emb_input".into(), input.clone());
        }
        if let Some(dims) = obj.get("dimensions") {
            extra.insert("__emb_dimensions".into(), dims.clone());
        }
        if let Some(ef) = obj.get("encoding_format") {
            extra.insert("__emb_encoding_format".into(), ef.clone());
        }
        if let Some(user) = obj.get("user") {
            extra.insert("__emb_user".into(), user.clone());
        }

        // ── Vendor extensions – ingress segment ───────────────────────────────
        // Collect unknown fields into __vendor_ingress.
        // Policy is VendorFieldPolicy::Drop, so the encoder will not forward
        // them unless the policy is relaxed per-provider in the future.
        let mut vendor_ingress = serde_json::Map::new();
        for (k, v) in obj {
            if !KNOWN_EMBEDDINGS_FIELDS.contains(&k.as_str()) {
                vendor_ingress.insert(k.clone(), v.clone());
            }
        }
        if !vendor_ingress.is_empty() {
            extra.insert("__vendor_ingress".into(), Value::Object(vendor_ingress));
        }

        Ok(InternalRequest {
            messages: Vec::new(),
            model,
            stream: false,
            temperature: None,
            max_tokens: None,
            top_p: None,
            tools: None,
            tool_choice: None,
            source_protocol: OPENAI_EMBEDDINGS_V1,
            extra,
        })
    }
}

// ── Encoder ───────────────────────────────────────────────────────────────────

struct EmbeddingsEncoder;

impl EgressEncoder for EmbeddingsEncoder {
    fn encode_request(&self, req: &InternalRequest) -> anyhow::Result<(Value, HeaderMap)> {
        // Build the egress body from explicit parsed fields.
        // Override `model` because routing may have changed it.
        let mut obj = serde_json::Map::new();

        obj.insert("model".into(), Value::String(req.model.clone()));

        if let Some(input) = req.extra.get("__emb_input") {
            obj.insert("input".into(), input.clone());
        } else if let Some(pb) = req.extra.get(EMBEDDINGS_BODY_KEY) {
            // Fallback: take input from the original passthrough body.
            if let Some(inp) = pb.get("input") {
                obj.insert("input".into(), inp.clone());
            }
        }

        if let Some(dims) = req.extra.get("__emb_dimensions") {
            obj.insert("dimensions".into(), dims.clone());
        }
        if let Some(ef) = req.extra.get("__emb_encoding_format") {
            obj.insert("encoding_format".into(), ef.clone());
        }
        if let Some(user) = req.extra.get("__emb_user") {
            obj.insert("user".into(), user.clone());
        }

        // ── Vendor extensions – egress segment ───────────────────────────────
        // CAPS.unknown_field_policy = Drop; vendor fields are suppressed.
        // If a future per-provider override sets Passthrough, emit them here:
        //
        //   if policy == VendorFieldPolicy::Passthrough {
        //       if let Some(Value::Object(vi)) = req.extra.get("__vendor_ingress") {
        //           for (k, v) in vi { obj.insert(k.clone(), v.clone()); }
        //       }
        //   }

        Ok((Value::Object(obj), HeaderMap::new()))
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
             revisit codec/openai/embeddings.rs before routing through it"
        )
    }
}

struct EmbeddingsResponseFormatter;

impl ResponseFormatter for EmbeddingsResponseFormatter {
    fn format_response(&self, _resp: &InternalResponse) -> Value {
        unreachable!(
            "embeddings_proxy bypasses the codec pipeline; \
             revisit codec/openai/embeddings.rs before routing through it"
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
        ..TokenUsage::default()
    }
}
