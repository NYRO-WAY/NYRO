//! Provider layer — vendor adapters, metadata, and orchestration traits.
//!
//! # Architecture (PR-15)
//!
//! ```text
//! provider/
//! ├── adapter.rs          — ProviderAdapter trait + ProviderCtx
//! ├── vendor_ext.rs       — VendorExtension trait + VendorCtx (hooks)
//! ├── registry.rs         — VendorRegistry + ProviderAdapterRegistry
//! ├── metadata.rs         — VendorMetadata types (moved from protocol/vendor/types.rs)
//! ├── outbound.rs         — OutboundRequest (wire-format outbound)
//! ├── inbound.rs          — InboundResponse (wire-format inbound)
//! ├── stream.rs           — ProviderStreamParser trait
//! ├── common/
//! │   └── openai.rs       — shared Bearer auth / URL helpers + pipeline helpers
//! └── <vendor>/mod.rs     — per-vendor VendorExtension + ProviderAdapter impls
//! ```

pub mod adapter;
pub mod common;
pub mod inbound;
pub mod metadata;
pub mod outbound;
pub mod registry;
pub mod stream;
pub mod vendor_ext;

// ── Known vendors (each registers itself via inventory::submit!) ──────────────
pub mod anthropic;
pub mod custom;
pub mod deepseek;
pub mod google;
pub mod minimax;
pub mod moonshotai;
pub mod nvidia;
pub mod ollama;
pub mod openai;
pub mod openrouter;
pub mod xai;
pub mod zai;
pub mod zhipuai;

// ── Flat re-exports for convenient import paths ───────────────────────────────

pub use adapter::{ProviderAdapter, ProviderCtx};
pub use inbound::InboundResponse;
pub use metadata::{
    AuthMode, ChannelDef, Label, OAuthConfig, ProtocolBaseUrl, RuntimeConfig, VendorMetadata,
};
pub use outbound::OutboundRequest;
pub use registry::{
    ProviderAdapterRegistration, ProviderAdapterRegistry, VendorRegistration, VendorRegistry,
    VendorScope,
};
pub use stream::{LegacyStreamParserAdapter, ProviderStreamParser};
pub use vendor_ext::{VendorCtx, VendorExtension};
