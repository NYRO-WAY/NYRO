//! Anthropic Messages API (`POST /v1/messages`).
//!
//! Wire version is the schema date `2023-06-01` (the `anthropic-version` header
//! the API requires), not the URL prefix `v1`.

use crate::protocol::codec::anthropic;
use crate::protocol::ids::{ANTHROPIC_MESSAGES_2023_06_01, ProtocolCapabilities, ProtocolId};
use crate::protocol::registry::ProtocolRegistration;
use crate::protocol::traits::*;

pub struct AnthropicMessages2023;

const CAPS: ProtocolCapabilities = ProtocolCapabilities {
    streaming: true,
    tools: true,
    reasoning: true,
    embeddings: false,
    force_upstream_stream: false,
    override_model_in_body: false,
    ingress_routes: &[("POST", "/v1/messages"), ("POST", "/messages")],
};

impl ProtocolHandler for AnthropicMessages2023 {
    fn id(&self) -> ProtocolId {
        ANTHROPIC_MESSAGES_2023_06_01
    }
    fn capabilities(&self) -> &'static ProtocolCapabilities {
        &CAPS
    }
    fn make_decoder(&self) -> Box<dyn IngressDecoder + Send> {
        Box::new(anthropic::decoder::AnthropicDecoder)
    }
    fn make_encoder(&self) -> Box<dyn EgressEncoder + Send> {
        Box::new(anthropic::encoder::AnthropicEncoder)
    }
    fn make_response_parser(&self) -> Box<dyn ResponseParser> {
        Box::new(anthropic::stream::AnthropicResponseParser)
    }
    fn make_response_formatter(&self) -> Box<dyn ResponseFormatter> {
        Box::new(anthropic::stream::AnthropicResponseFormatter)
    }
    fn make_stream_parser(&self) -> Box<dyn StreamParser> {
        Box::new(anthropic::stream::AnthropicStreamParser::new())
    }
    fn make_stream_formatter(&self) -> Box<dyn StreamFormatter> {
        Box::new(anthropic::stream::AnthropicStreamFormatter::new())
    }
}

inventory::submit! {
    ProtocolRegistration { make: || Box::new(AnthropicMessages2023) }
}
