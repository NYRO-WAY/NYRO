//! Wire-level codec implementations. Each submodule owns a single Protocol's
//! request/response/stream codecs **and** the thin `EndpointHandler`
//! registration shell for every endpoint.

pub mod anthropic_messages;
pub mod google_generative;
pub mod openai_compatible;
pub mod openai_responses;
pub mod reasoning;
pub mod tool_correlation;
