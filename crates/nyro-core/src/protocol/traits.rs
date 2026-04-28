//! Aggregated `ProtocolHandler` trait.
//!
//! A handler bundles a `ProtocolId`, static `ProtocolCapabilities`, and factory
//! methods for the six codec components (decoder / encoder / response parser /
//! response formatter / streaming parser / streaming formatter).
//!
//! Codec traits remain defined in `super` (i.e. `protocol/mod.rs`) for backward
//! compatibility with existing `use crate::protocol::IngressDecoder` call sites;
//! this module re-exports them so new code can `use crate::protocol::traits::*`
//! to pull in everything needed to implement a handler.

pub use super::{
    EgressEncoder, IngressDecoder, ResponseFormatter, ResponseParser, StreamFormatter,
    StreamParser,
};

use crate::protocol::ids::{ProtocolCapabilities, ProtocolId};

/// Single trait that aggregates the six codec components plus identity and
/// capabilities. Each registered handler is constructed once via
/// `ProtocolRegistration::make` and stored in `ProtocolRegistry`.
///
/// Stream parsers/formatters are stateful, so factory methods return owned
/// `Box<dyn ...>`. Stateless decoders/encoders/parsers/formatters also use
/// `Box` for uniformity (the registry stores `Arc<dyn ProtocolHandler>` so
/// the cost is one allocation per request, matching the legacy factory
/// functions in `protocol/mod.rs`).
pub trait ProtocolHandler: Send + Sync + 'static {
    fn id(&self) -> ProtocolId;
    fn capabilities(&self) -> &'static ProtocolCapabilities;

    fn make_decoder(&self) -> Box<dyn IngressDecoder + Send>;
    fn make_encoder(&self) -> Box<dyn EgressEncoder + Send>;
    fn make_response_parser(&self) -> Box<dyn ResponseParser>;
    fn make_response_formatter(&self) -> Box<dyn ResponseFormatter>;
    fn make_stream_parser(&self) -> Box<dyn StreamParser>;
    fn make_stream_formatter(&self) -> Box<dyn StreamFormatter>;
}
