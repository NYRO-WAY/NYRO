//! Aggregated `EndpointHandler` trait.
//!
//! A handler bundles a `ProtocolEndpoint`, static `EndpointCapabilities`, and factory
//! methods for the six codec components (request decoder / request encoder /
//! response decoder / response encoder / stream response decoder / stream response encoder).
//!
//! Codec traits remain defined in `super` (i.e. `protocol/mod.rs`) for backward
//! compatibility with existing `use crate::protocol::RequestDecoder` call sites;
//! this module re-exports them so new code can `use crate::protocol::traits::*`
//! to pull in everything needed to implement a handler.

pub use super::{
    RequestDecoder, RequestEncoder, ResponseDecoder, ResponseEncoder, StreamResponseDecoder,
    StreamResponseEncoder,
};

use crate::protocol::ids::{EndpointCapabilities, Protocol, ProtocolEndpoint};

/// Single trait that aggregates the six codec components plus identity and
/// capabilities. Each registered handler is constructed once via
/// `EndpointRegistration::make` and stored in `ProtocolRegistry`.
///
/// Stream parsers/formatters are stateful, so factory methods return owned
/// `Box<dyn ...>`. Stateless decoders/encoders/parsers/formatters also use
/// `Box` for uniformity (the registry stores `Arc<dyn EndpointHandler>` so
/// the cost is one allocation per request, matching the legacy factory
/// functions in `protocol/mod.rs`).
pub trait EndpointHandler: Send + Sync + 'static {
    fn id(&self) -> ProtocolEndpoint;
    fn capabilities(&self) -> &'static EndpointCapabilities;

    /// The protocol suite this handler belongs to.  Derived from `id()` by default.
    fn protocol(&self) -> Protocol {
        self.id().protocol
    }

    fn make_request_decoder(&self) -> Box<dyn RequestDecoder + Send>;
    fn make_request_encoder(&self) -> Box<dyn RequestEncoder + Send>;
    fn make_response_decoder(&self) -> Box<dyn ResponseDecoder>;
    fn make_response_encoder(&self) -> Box<dyn ResponseEncoder>;
    fn make_stream_response_decoder(&self) -> Box<dyn StreamResponseDecoder>;
    fn make_stream_response_encoder(&self) -> Box<dyn StreamResponseEncoder>;
}
