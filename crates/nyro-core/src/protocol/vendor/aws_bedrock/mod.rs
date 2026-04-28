//! Placeholder for the AWS Bedrock vendor.
//!
//! Not registered with `inventory` yet. Bedrock requires SigV4
//! request signing (see `sigv4.rs`) and a small response wrapper layer
//! (see `wrapper.rs`) on top of the Anthropic / Cohere / Meta dialects
//! it forwards. The full implementation will arrive in a follow-up PR.

pub mod sigv4;
pub mod wrapper;

// TODO: METADATA + VendorExtension impl wiring auth_headers to SigV4
// signing and post_parse to the Bedrock response wrapper.
