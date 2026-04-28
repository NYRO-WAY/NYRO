//! Placeholder for the Azure AI Foundry vendor.
//!
//! Not registered with `inventory` yet. Foundry exposes an
//! OpenAI-compatible surface plus its own `model-deployment`
//! conventions; the implementation will arrive in a follow-up PR
//! together with its bearer-token / Entra ID auth driver.
//!
//! Note: per design discussion we will *only* support the Foundry
//! flavour and rely on Foundry's compat layer for Azure OpenAI; the
//! legacy `deployment_map.rs` shape is not preserved.

// TODO: METADATA + VendorExtension impl + bearer / Entra ID auth driver.
