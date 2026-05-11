use serde::{Deserialize, Serialize};

use crate::config::TpmMode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamRateLimitSummary {
    pub key_count: u32,
    pub enabled_key_count: u32,
    pub model_rule_count: u32,
    pub has_tpm: bool,
    pub has_rpm: bool,
    pub has_rpd: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitMetricSnapshot {
    pub used: u32,
    pub limit: Option<u32>,
    pub ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamRateLimitRuntimeSnapshot {
    pub captured_at_ms: i64,
    #[serde(default)]
    pub models: Vec<ProviderModelRuntimeSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderModelRuntimeSnapshot {
    pub model: String,
    pub matched_rule_model: Option<String>,
    pub tpm_mode: Option<TpmMode>,
    pub rpm_limit: Option<u32>,
    pub rpd_limit: Option<u32>,
    pub tpm_limit: Option<u32>,
    pub key_count: u32,
    pub enabled_key_count: u32,
    pub available_key_count: u32,
    pub next_cursor: usize,
    #[serde(default)]
    pub keys: Vec<KeyModelRuntimeSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyModelRuntimeSnapshot {
    pub key_id: String,
    pub enabled: bool,
    pub available: bool,
    pub blocked_reason: Option<String>,
    pub active_lease_count: u32,
    pub rpm: RateLimitMetricSnapshot,
    pub rpd: RateLimitMetricSnapshot,
    pub tpm: RateLimitMetricSnapshot,
    pub rpd_window_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct KeySelectionInput<'a> {
    pub provider_id: &'a str,
    pub provider_name: &'a str,
    pub actual_model: &'a str,
    pub request_input_tokens: u32,
    pub request_output_reservation: u32,
}

#[derive(Debug, Clone)]
pub struct SelectedUpstreamKey {
    pub key_id: String,
    pub api_key: String,
    pub lease: RateLimitLease,
}

#[derive(Debug, Clone)]
pub struct RateLimitLease {
    pub lease_id: String,
    pub provider_id: String,
    pub key_id: String,
    pub model: String,
    pub reserved_input_tokens: u32,
    pub reserved_output_tokens: u32,
    pub tpm_mode: TpmMode,
    pub request_started_at_ms: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SettlementUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}
