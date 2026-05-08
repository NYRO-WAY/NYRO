//! Cross-vendor shared adapter utilities.
//!
//! # Extension guidelines
//!
//! - Currently only `openai.rs` exists here because OpenAI-compatible
//!   auth/URL logic is shared by 10+ vendors.
//! - Add `anthropic.rs` / `google.rs` etc. only when ≥3 real vendors
//!   need the same logic; do not create empty placeholder files.
//! - For lightweight code reuse that doesn't reach the 3-vendor
//!   threshold, prefer a `shared.rs` helper inside the vendor's own
//!   directory (e.g. `provider/my_family/shared.rs`).

pub mod openai;
pub mod pipeline;
