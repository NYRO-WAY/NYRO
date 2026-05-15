//! Integration hooks — open extension points for the proxy pipeline.
//!
//! # Architecture
//!
//! Two traits cover the two principal injection points:
//!
//! - [`RequestHook`]: fires after auth, before upstream. Can mutate or reject.
//! - [`ResponseHook`]: fires after a successful upstream response, before
//!   the response is serialized and sent to the client. Observe-only by
//!   convention (errors are logged and ignored).
//!
//! # Registration (compile-time)
//!
//! Use `inventory::submit!` to register a hook at startup:
//!
//! ```rust,ignore
//! use nyro_core::integrations::hooks::{
//!     HookContext, RequestHook, RequestHookRegistration,
//!     ResponseHook, ResponseHookRegistration,
//! };
//! use nyro_core::error::GatewayError;
//! use nyro_core::protocol::ir::{AiRequest, AiResponse};
//! use async_trait::async_trait;
//!
//! struct MyAuditHook;
//!
//! #[async_trait]
//! impl ResponseHook for MyAuditHook {
//!     fn name(&self) -> &'static str { "my-audit" }
//!     async fn on_response(&self, ctx: &HookContext, resp: &mut InternalResponse, latency_ms: u64) {
//!         tracing::info!(provider = %ctx.provider_name, model = %ctx.model,
//!                        latency_ms, "response received");
//!     }
//! }
//!
//! inventory::submit! { ResponseHookRegistration { make: || Box::new(MyAuditHook) } }
//! ```

pub mod hooks;

pub use hooks::{
    HookContext, HookRegistry, RequestHook, RequestHookRegistration, ResponseHook,
    ResponseHookRegistration,
};
