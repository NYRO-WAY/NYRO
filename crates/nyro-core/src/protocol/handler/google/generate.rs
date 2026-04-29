//! Google Generate API (`POST /v1beta/models/:model:generateContent`).
//!
//! Family is `google` (the company), not `gemini` (the product) — the same
//! family will host Vertex AI dialects later. Wire version `v1beta` matches
//! Google's URL versioning.
//!
//! `override_model_in_body` is true: the encoder embeds the actual model name
//! in the request body / URL path rather than a top-level `model` field,
//! matching the legacy Gemini branch in `handler.rs`.

use crate::protocol::gemini;
use crate::protocol::ids::{GOOGLE_GENERATE_V1BETA, ProtocolCapabilities, ProtocolId};
use crate::protocol::registry::ProtocolRegistration;
use crate::protocol::traits::*;

pub struct GoogleGenerateV1Beta;

const CAPS: ProtocolCapabilities = ProtocolCapabilities {
    streaming: true,
    tools: true,
    reasoning: true,
    embeddings: false,
    force_upstream_stream: false,
    override_model_in_body: true,
    ingress_routes: &[
        ("POST", "/v1beta/models/:model_action"),
        ("POST", "/models/:model_action"),
    ],
};

impl ProtocolHandler for GoogleGenerateV1Beta {
    fn id(&self) -> ProtocolId {
        GOOGLE_GENERATE_V1BETA
    }
    fn capabilities(&self) -> &'static ProtocolCapabilities {
        &CAPS
    }
    fn make_decoder(&self) -> Box<dyn IngressDecoder + Send> {
        Box::new(gemini::decoder::GeminiDecoder)
    }
    fn make_encoder(&self) -> Box<dyn EgressEncoder + Send> {
        Box::new(gemini::encoder::GeminiEncoder)
    }
    fn make_response_parser(&self) -> Box<dyn ResponseParser> {
        Box::new(gemini::stream::GeminiResponseParser)
    }
    fn make_response_formatter(&self) -> Box<dyn ResponseFormatter> {
        Box::new(gemini::stream::GeminiResponseFormatter)
    }
    fn make_stream_parser(&self) -> Box<dyn StreamParser> {
        Box::new(gemini::stream::GeminiStreamParser::new())
    }
    fn make_stream_formatter(&self) -> Box<dyn StreamFormatter> {
        Box::new(gemini::stream::GeminiStreamFormatter::new())
    }
}

inventory::submit! {
    ProtocolRegistration { make: || Box::new(GoogleGenerateV1Beta) }
}
