//! Google Generate API (`POST /v1beta/models/:model:generateContent`).
//!
//! Family is `google` (the company), not `gemini` (the product) — the same
//! family will host Vertex AI dialects later. Wire version `v1beta` matches
//! Google's URL versioning.
//!
//! `override_model_in_body` is true: the encoder embeds the actual model name
//! in the request body / URL path rather than a top-level `model` field,
//! matching the legacy Google branch in `proxy/handler.rs`.

use crate::protocol::codec::google;
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
        Box::new(google::decoder::GoogleDecoder)
    }
    fn make_encoder(&self) -> Box<dyn EgressEncoder + Send> {
        Box::new(google::encoder::GoogleEncoder)
    }
    fn make_response_parser(&self) -> Box<dyn ResponseParser> {
        Box::new(google::stream::GoogleResponseParser)
    }
    fn make_response_formatter(&self) -> Box<dyn ResponseFormatter> {
        Box::new(google::stream::GoogleResponseFormatter)
    }
    fn make_stream_parser(&self) -> Box<dyn StreamParser> {
        Box::new(google::stream::GoogleStreamParser::new())
    }
    fn make_stream_formatter(&self) -> Box<dyn StreamFormatter> {
        Box::new(google::stream::GoogleStreamFormatter::new())
    }
}

inventory::submit! {
    ProtocolRegistration { make: || Box::new(GoogleGenerateV1Beta) }
}
