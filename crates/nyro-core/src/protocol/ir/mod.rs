//! New Internal Representation (IR) for the Nyro AI Gateway.
//!
//! # Design principles
//!
//! 1. **No silent drops** — every field from every supported protocol has
//!    an explicit home: `known field`, `vendor.known_specific`, or
//!    `vendor.passthrough_safe`.
//! 2. **Lossless envelope** — `RawEnvelope` keeps a snapshot of the original
//!    request body + headers for pass-through and audit.
//! 3. **Parallel deployment** — old `InternalRequest`/`InternalResponse` in
//!    `protocol/types.rs` stay as `#[deprecated]` shims via `From` impls in
//!    `compat.rs`.  No breaking change until all codec PRs (08-12) are done.
//! 4. **Repair, not validate** — `repair.rs` performs single-direction
//!    mutations (fill missing `tool_call_id`, fix orphaned refs, patch broken
//!    conversation structure) without rejecting the request.

pub mod compat;
pub mod envelope;
pub mod repair;
pub mod request;
pub mod response;
pub mod stream;
pub mod vendor_ext;

pub use envelope::RawEnvelope;
pub use request::{
    AiRequest, GenerationConfig, Message, MessageContent, ReasoningConfig, RequestMetadata,
    ResponseFormat, Role, SafetySettings, StreamConfig, ToolChoice, ToolSpec,
};
pub use response::{AiResponse, ResponseItem};
pub use stream::StreamDelta as AiStreamDelta;
pub use vendor_ext::VendorExtensions;
