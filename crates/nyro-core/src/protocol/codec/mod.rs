//! Wire-level codec implementations. Each submodule owns a single API
//! family's request/response/stream codecs as plain types; the
//! [`handler`](super::handler) layer wraps them into `ProtocolHandler`
//! registrations.

pub mod openai;
pub mod anthropic;
pub mod google;
