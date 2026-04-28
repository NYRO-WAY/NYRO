//! Placeholder for the Google Vertex AI vendor.
//!
//! Not registered with `inventory` yet. Vertex requires Google Cloud
//! service-account or ADC auth (see `auth.rs`) and project/region URL
//! construction (see `url.rs`). The full implementation will arrive
//! in a follow-up PR.

pub mod auth;
pub mod url;

// TODO: METADATA + VendorExtension impl wiring auth_headers to GCP
// access tokens and build_url to projects/{project}/locations/{region}.
