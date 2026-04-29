//! OpenAI Responses API (`POST /v1/responses`).
//!
//! `force_upstream_stream` is true: the upstream call is always made in
//! streaming mode regardless of the client's `stream` flag, matching the
//! legacy `matches!(egress, Protocol::ResponsesAPI)` branch in `handler.rs`.

use crate::protocol::codec::openai::responses;
use crate::protocol::ids::{OPENAI_RESPONSES_V1, ProtocolCapabilities, ProtocolId};
use crate::protocol::registry::ProtocolRegistration;
use crate::protocol::traits::*;

pub struct OpenAIResponsesV1;

const CAPS: ProtocolCapabilities = ProtocolCapabilities {
    streaming: true,
    tools: true,
    reasoning: true,
    embeddings: false,
    force_upstream_stream: true,
    override_model_in_body: false,
    ingress_routes: &[("POST", "/v1/responses"), ("POST", "/responses")],
};

impl ProtocolHandler for OpenAIResponsesV1 {
    fn id(&self) -> ProtocolId {
        OPENAI_RESPONSES_V1
    }
    fn capabilities(&self) -> &'static ProtocolCapabilities {
        &CAPS
    }
    fn make_decoder(&self) -> Box<dyn IngressDecoder + Send> {
        Box::new(responses::decoder::ResponsesDecoder)
    }
    fn make_encoder(&self) -> Box<dyn EgressEncoder + Send> {
        Box::new(responses::encoder::ResponsesEncoder)
    }
    fn make_response_parser(&self) -> Box<dyn ResponseParser> {
        Box::new(responses::parser::ResponsesResponseParser)
    }
    fn make_response_formatter(&self) -> Box<dyn ResponseFormatter> {
        Box::new(responses::formatter::ResponsesResponseFormatter)
    }
    fn make_stream_parser(&self) -> Box<dyn StreamParser> {
        Box::new(responses::parser::ResponsesStreamParser::new())
    }
    fn make_stream_formatter(&self) -> Box<dyn StreamFormatter> {
        Box::new(responses::stream::ResponsesStreamFormatter::new())
    }
}

inventory::submit! {
    ProtocolRegistration { make: || Box::new(OpenAIResponsesV1) }
}
