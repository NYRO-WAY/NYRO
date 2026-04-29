//! OpenAI Chat Completions API (`POST /v1/chat/completions`).
//!
//! Wraps the existing `protocol::codec::openai::{decoder,encoder,stream}` codecs
//! into a `ProtocolHandler` and registers it via `inventory::submit!`.

use crate::protocol::codec::openai;
use crate::protocol::ids::{OPENAI_CHAT_V1, ProtocolCapabilities, ProtocolId};
use crate::protocol::registry::ProtocolRegistration;
use crate::protocol::traits::*;

pub struct OpenAIChatV1;

const CAPS: ProtocolCapabilities = ProtocolCapabilities {
    streaming: true,
    tools: true,
    reasoning: true,
    embeddings: false,
    force_upstream_stream: false,
    override_model_in_body: false,
    ingress_routes: &[
        ("POST", "/v1/chat/completions"),
        ("POST", "/chat/completions"),
    ],
};

impl ProtocolHandler for OpenAIChatV1 {
    fn id(&self) -> ProtocolId {
        OPENAI_CHAT_V1
    }
    fn capabilities(&self) -> &'static ProtocolCapabilities {
        &CAPS
    }
    fn make_decoder(&self) -> Box<dyn IngressDecoder + Send> {
        Box::new(openai::decoder::OpenAIDecoder)
    }
    fn make_encoder(&self) -> Box<dyn EgressEncoder + Send> {
        Box::new(openai::encoder::OpenAIEncoder)
    }
    fn make_response_parser(&self) -> Box<dyn ResponseParser> {
        Box::new(openai::stream::OpenAIResponseParser)
    }
    fn make_response_formatter(&self) -> Box<dyn ResponseFormatter> {
        Box::new(openai::stream::OpenAIResponseFormatter)
    }
    fn make_stream_parser(&self) -> Box<dyn StreamParser> {
        Box::new(openai::stream::OpenAIStreamParser::new())
    }
    fn make_stream_formatter(&self) -> Box<dyn StreamFormatter> {
        Box::new(openai::stream::OpenAIStreamFormatter::new())
    }
}

inventory::submit! {
    ProtocolRegistration { make: || Box::new(OpenAIChatV1) }
}
