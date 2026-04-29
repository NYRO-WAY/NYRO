//! `ProtocolHandler` registration shells. One file per dialect; each
//! wraps a [`codec`](super::codec) implementation, declares its
//! [`ProtocolCapabilities`](super::ids::ProtocolCapabilities), and
//! registers via `inventory::submit!`.
//!
//! Adding a new dialect = create `handler/<family>/<dialect>.rs` plus
//! one `inventory::submit!`. The `family/dialect/version` identity in
//! [`ids`](super::ids) is unchanged by this layout.

pub mod openai;
pub mod anthropic;
pub mod google;
