use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ProtocolFamily {
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "google")]
    Google,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuthStrategy {
    Bearer,
    HeaderApiKey { header_name: String },
    QueryApiKey { parameter_name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum KeySelectionStrategy {
    #[default]
    Random,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownstreamRouteKind {
    OpenAiChatCompletions,
    OpenAiResponses,
    AnthropicMessages,
    GoogleV1BetaModels,
    GoogleV1Models,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RaceTargetEndpoint {
    pub protocol_family: ProtocolFamily,
    pub base_url: String,
    pub auth_strategy: AuthStrategy,
    pub key_pool_id: String,
    #[serde(default)]
    pub request_timeout_ms: Option<u64>,
    #[serde(default)]
    pub extra_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub extra_query: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RaceModelDescriptor {
    pub id: String,
    pub display_name: String,
    pub upstream_model: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub endpoints: Vec<RaceTargetEndpoint>,
    #[serde(default = "default_metadata")]
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RaceCandidate {
    pub id: String,
    pub group_id: String,
    pub name: String,
    #[serde(default)]
    pub model_id: Option<String>,
    pub upstream_model: String,
    #[serde(default)]
    pub inline_endpoint_overrides: Vec<RaceTargetEndpoint>,
    pub initial_weight: f64,
    pub response_protection_timeout_ms: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_metadata")]
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RaceGroup {
    pub id: String,
    pub display_name: String,
    pub fallback_ratio: f64,
    pub decay_factor: f64,
    pub penalty_rate: f64,
    pub recovery_rate: f64,
    #[serde(default)]
    pub race_max_wait_time_ms: Option<u64>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub candidates: Vec<RaceCandidate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RaceKeyPool {
    pub id: String,
    pub display_name: String,
    pub auth_strategy: AuthStrategy,
    #[serde(default)]
    pub selection_strategy: KeySelectionStrategy,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub keys: Vec<RaceKey>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RaceKey {
    pub id: String,
    pub key_pool_id: String,
    pub secret: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_metadata")]
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RaceSettings {
    #[serde(default)]
    pub enable_race_diagnostics_header: bool,
    #[serde(default = "default_max_buffer_events")]
    pub max_buffer_events: usize,
    #[serde(default = "default_buffer_backpressure_timeout_ms")]
    pub buffer_backpressure_timeout_ms: u64,
}

impl Default for RaceSettings {
    fn default() -> Self {
        Self {
            enable_race_diagnostics_header: false,
            max_buffer_events: default_max_buffer_events(),
            buffer_backpressure_timeout_ms: default_buffer_backpressure_timeout_ms(),
        }
    }
}

impl RaceSettings {
    pub fn normalized(self) -> Self {
        Self {
            enable_race_diagnostics_header: self.enable_race_diagnostics_header,
            max_buffer_events: self.max_buffer_events.max(1),
            buffer_backpressure_timeout_ms: self.buffer_backpressure_timeout_ms.max(1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RaceModelSummary {
    pub id: String,
    pub display_name: String,
    pub upstream_model: String,
    pub enabled: bool,
    #[serde(default)]
    pub protocol_families: Vec<ProtocolFamily>,
    #[serde(default)]
    pub key_pool_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RaceGroupSummary {
    pub id: String,
    pub display_name: String,
    pub enabled: bool,
    pub candidate_count: usize,
    pub enabled_candidate_count: usize,
    #[serde(default)]
    pub protocol_families: Vec<ProtocolFamily>,
    #[serde(default)]
    pub candidate_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RaceKeyPoolSummary {
    pub id: String,
    pub display_name: String,
    pub enabled: bool,
    pub auth_strategy: AuthStrategy,
    pub selection_strategy: KeySelectionStrategy,
    pub total_keys: usize,
    pub enabled_keys: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub field: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationErrorResponse {
    pub valid: bool,
    #[serde(default)]
    pub issues: Vec<ValidationIssue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedCandidateTarget {
    pub candidate: RaceCandidate,
    pub endpoint: RaceTargetEndpoint,
    pub endpoint_source: ResolvedEndpointSource,
    pub key_pool_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedEndpointSource {
    CandidateInlineOverride,
    ModelDescriptor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BootstrapData {
    #[serde(default)]
    pub models: Vec<RaceModelDescriptor>,
    #[serde(default)]
    pub groups: Vec<RaceGroup>,
    #[serde(default)]
    pub key_pools: Vec<RaceKeyPool>,
    #[serde(default)]
    pub settings: Option<RaceSettings>,
}

impl Default for BootstrapData {
    fn default() -> Self {
        Self {
            models: Vec::new(),
            groups: Vec::new(),
            key_pools: Vec::new(),
            settings: Some(RaceSettings::default()),
        }
    }
}

impl RaceModelDescriptor {
    pub fn summary(&self) -> RaceModelSummary {
        let mut protocol_families = self
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.enabled)
            .map(|endpoint| endpoint.protocol_family)
            .collect::<Vec<_>>();
        protocol_families.sort();
        protocol_families.dedup();

        let mut key_pool_ids = self
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.enabled)
            .map(|endpoint| endpoint.key_pool_id.clone())
            .collect::<Vec<_>>();
        key_pool_ids.sort();
        key_pool_ids.dedup();

        RaceModelSummary {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            upstream_model: self.upstream_model.clone(),
            enabled: self.enabled,
            protocol_families,
            key_pool_ids,
        }
    }
}

impl RaceGroup {
    pub fn summary(&self) -> RaceGroupSummary {
        let enabled_candidate_count = self
            .candidates
            .iter()
            .filter(|candidate| candidate.enabled)
            .count();
        let mut protocol_families = self
            .candidates
            .iter()
            .flat_map(|candidate| {
                candidate
                    .inline_endpoint_overrides
                    .iter()
                    .filter(|endpoint| endpoint.enabled)
                    .map(|endpoint| endpoint.protocol_family)
            })
            .collect::<Vec<_>>();
        protocol_families.sort();
        protocol_families.dedup();

        RaceGroupSummary {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            enabled: self.enabled,
            candidate_count: self.candidates.len(),
            enabled_candidate_count,
            protocol_families,
            candidate_names: self
                .candidates
                .iter()
                .map(|candidate| candidate.name.clone())
                .collect(),
        }
    }
}

impl RaceKeyPool {
    pub fn summary(&self) -> RaceKeyPoolSummary {
        RaceKeyPoolSummary {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            enabled: self.enabled,
            auth_strategy: self.auth_strategy.clone(),
            selection_strategy: self.selection_strategy,
            total_keys: self.keys.len(),
            enabled_keys: self.keys.iter().filter(|key| key.enabled).count(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_metadata() -> Value {
    Value::Object(Default::default())
}

fn default_max_buffer_events() -> usize {
    100_000
}

fn default_buffer_backpressure_timeout_ms() -> u64 {
    100
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_descriptor_round_trip() {
        let model = RaceModelDescriptor {
            id: "model-a".to_string(),
            display_name: "Model A".to_string(),
            upstream_model: "vendor/model-a".to_string(),
            description: String::new(),
            enabled: true,
            endpoints: vec![RaceTargetEndpoint {
                protocol_family: ProtocolFamily::OpenAi,
                base_url: "https://example.com/v1".to_string(),
                auth_strategy: AuthStrategy::Bearer,
                key_pool_id: "pool-a".to_string(),
                request_timeout_ms: Some(30_000),
                extra_headers: BTreeMap::new(),
                extra_query: BTreeMap::new(),
                enabled: true,
            }],
            metadata: Value::Object(Default::default()),
        };

        let json = serde_json::to_string(&model).expect("serialize");
        let restored: RaceModelDescriptor = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, model);
    }
}
