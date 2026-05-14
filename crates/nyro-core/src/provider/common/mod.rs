//! Cross-vendor shared adapter utilities.
//!
//! # Extension guidelines
//!
//! - `openai_compat.rs` covers Bearer-auth, URL construction, error mapping,
//!   and the `openai_compat_vendor!` macro — shared by 10+ vendors.
//! - Add `anthropic_compat.rs` / `google_compat.rs` etc. only when ≥3 real
//!   vendors need the same logic; do not create empty placeholder files.
//! - For lightweight code reuse below the 3-vendor threshold, prefer a
//!   `shared.rs` helper inside the vendor's own directory.

pub mod openai_compat;
pub mod pipeline;

/// Backward-compatible alias: `use crate::provider::common::openai::*` still works.
pub use openai_compat as openai;
