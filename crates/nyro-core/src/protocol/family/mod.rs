//! Family handlers — one module per protocol family, with one submodule per
//! dialect that emits an `inventory::submit!` registration.
//!
//! Adding a new dialect = create `family/<family>/<dialect>.rs`, implement
//! `ProtocolHandler`, append one `inventory::submit!`, and reference the
//! module from `family/<family>/mod.rs`. No `match` updates anywhere.

pub mod openai;
pub mod anthropic;
pub mod google;
