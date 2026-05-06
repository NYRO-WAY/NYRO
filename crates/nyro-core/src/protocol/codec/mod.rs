//! Wire-level codec implementations. Each submodule owns a single API
//! family's request/response/stream codecs **and** the thin
//! `ProtocolHandler` registration shell for that dialect.

pub mod openai;
pub mod anthropic;
pub mod google;
pub mod reasoning;
pub mod tool_correlation;
