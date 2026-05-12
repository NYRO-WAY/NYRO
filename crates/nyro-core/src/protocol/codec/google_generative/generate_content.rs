//! Google Generative AI (`POST /v1beta/models/:model:generateContent`).
//!
//! Wire version `v1beta` matches Google's URL versioning.
//!
//! `override_model_in_body` is true: the encoder embeds the actual model name
//! in the request body / URL path rather than a top-level `model` field.

use crate::protocol::ids::{EndpointCapabilities, GOOGLE_GENERATE_CONTENT_V1BETA, ProtocolEndpoint};
use crate::protocol::registry::EndpointRegistration;
use crate::protocol::traits::*;

pub struct GoogleGenerateContentV1Beta;

const CAPS: EndpointCapabilities = EndpointCapabilities {
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
    ..EndpointCapabilities::CHAT_STANDARD
};

impl EndpointHandler for GoogleGenerateContentV1Beta {
    fn id(&self) -> ProtocolEndpoint {
        GOOGLE_GENERATE_CONTENT_V1BETA
    }
    fn capabilities(&self) -> &'static EndpointCapabilities {
        &CAPS
    }
    fn make_decoder(&self) -> Box<dyn IngressDecoder + Send> {
        Box::new(super::decoder::GoogleDecoder)
    }
    fn make_encoder(&self) -> Box<dyn EgressEncoder + Send> {
        Box::new(super::encoder::GoogleEncoder)
    }
    fn make_response_parser(&self) -> Box<dyn ResponseParser> {
        Box::new(super::stream::GoogleResponseParser)
    }
    fn make_response_formatter(&self) -> Box<dyn ResponseFormatter> {
        Box::new(super::stream::GoogleResponseFormatter)
    }
    fn make_stream_parser(&self) -> Box<dyn StreamParser> {
        Box::new(super::stream::GoogleStreamParser::new())
    }
    fn make_stream_formatter(&self) -> Box<dyn StreamFormatter> {
        Box::new(super::stream::GoogleStreamFormatter::new())
    }
}

inventory::submit! {
    EndpointRegistration { make: || Box::new(GoogleGenerateContentV1Beta) }
}
