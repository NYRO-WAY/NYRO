use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub vendor: Option<String>,
    pub protocol: String,
    pub base_url: String,
    #[serde(default)]
    pub default_protocol: String,
    /// JSON map of protocol -> endpoint config.
    /// e.g. `{"openai":{"base_url":"https://..."},"anthropic":{"base_url":"https://..."}}`
    #[serde(default)]
    pub protocol_endpoints: String,
    pub preset_key: Option<String>,
    pub channel: Option<String>,
    #[serde(alias = "modelsEndpoint")]
    pub models_source: Option<String>,
    #[serde(alias = "capabilitiesSource")]
    pub capabilities_source: Option<String>,
    pub static_models: Option<String>,
    pub api_key: String,
    #[serde(default)]
    pub use_proxy: bool,
    pub last_test_success: Option<bool>,
    pub last_test_at: Option<String>,
    pub is_enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Route {
    pub id: String,
    pub name: String,
    #[serde(alias = "vmodel")]
    pub virtual_model: String,
    pub strategy: String,
    pub target_provider: String,
    pub target_model: String,
    pub access_control: bool,
    #[serde(default)]
    #[serde(alias = "type")]
    #[sqlx(default)]
    pub route_type: String,
    #[serde(default)]
    #[sqlx(default)]
    pub cache_exact_ttl: Option<i64>,
    #[serde(default)]
    #[sqlx(default)]
    pub cache_semantic_ttl: Option<i64>,
    #[serde(default)]
    #[sqlx(default)]
    pub cache_semantic_threshold: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[sqlx(skip)]
    pub cache: Option<RouteCacheConfig>,
    pub is_enabled: bool,
    pub created_at: String,
    #[serde(default)]
    #[sqlx(skip)]
    pub targets: Vec<RouteTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct RouteTarget {
    pub id: String,
    pub route_id: String,
    pub provider_id: String,
    pub model: String,
    pub weight: i32,
    pub priority: i32,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RouteStrategy {
    Weighted,
    Priority,
}

impl Default for RouteStrategy {
    fn default() -> Self {
        Self::Weighted
    }
}

impl RouteStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Weighted => "weighted",
            Self::Priority => "priority",
        }
    }
}

impl std::str::FromStr for RouteStrategy {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "weighted" => Ok(Self::Weighted),
            "priority" => Ok(Self::Priority),
            other => anyhow::bail!("unsupported route strategy: {other}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiKey {
    pub id: String,
    pub key: String,
    pub name: String,
    pub rpm: Option<i32>,
    pub rpd: Option<i32>,
    pub tpm: Option<i32>,
    pub tpd: Option<i32>,
    pub is_enabled: bool,
    pub expires_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyWithBindings {
    pub id: String,
    pub key: String,
    pub name: String,
    pub rpm: Option<i32>,
    pub rpd: Option<i32>,
    pub tpm: Option<i32>,
    pub tpd: Option<i32>,
    pub is_enabled: bool,
    pub expires_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub route_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct RequestLog {
    pub id: String,
    pub created_at: String,
    pub api_key_id: Option<String>,
    pub ingress_protocol: Option<String>,
    pub egress_protocol: Option<String>,
    pub request_model: Option<String>,
    pub actual_model: Option<String>,
    pub provider_name: Option<String>,
    pub status_code: Option<i32>,
    pub duration_ms: Option<f64>,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub is_stream: bool,
    pub is_tool_call: bool,
    pub error_message: Option<String>,
    pub response_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProvider {
    pub name: String,
    pub vendor: Option<String>,
    pub protocol: String,
    pub base_url: String,
    pub default_protocol: Option<String>,
    /// JSON map: `{"openai":{"base_url":"..."}}`
    pub protocol_endpoints: Option<String>,
    pub preset_key: Option<String>,
    pub channel: Option<String>,
    #[serde(alias = "modelsSource")]
    pub models_source: Option<String>,
    #[serde(alias = "capabilitiesSource")]
    pub capabilities_source: Option<String>,
    pub static_models: Option<String>,
    pub api_key: String,
    #[serde(default)]
    pub use_proxy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateProvider {
    pub name: Option<String>,
    pub vendor: Option<String>,
    pub protocol: Option<String>,
    pub base_url: Option<String>,
    pub default_protocol: Option<String>,
    pub protocol_endpoints: Option<String>,
    pub preset_key: Option<String>,
    pub channel: Option<String>,
    #[serde(alias = "modelsSource")]
    pub models_source: Option<String>,
    #[serde(alias = "capabilitiesSource")]
    pub capabilities_source: Option<String>,
    pub static_models: Option<String>,
    pub api_key: Option<String>,
    pub use_proxy: Option<bool>,
    pub is_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRoute {
    pub name: Option<String>,
    #[serde(alias = "vmodel")]
    pub virtual_model: Option<String>,
    pub strategy: Option<String>,
    pub target_provider: Option<String>,
    pub target_model: Option<String>,
    #[serde(default)]
    pub targets: Option<Vec<UpsertRouteTarget>>,
    pub access_control: Option<bool>,
    #[serde(alias = "type")]
    pub route_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<RouteCacheConfig>,
    #[serde(skip)]
    pub cache_exact_ttl: Option<i64>,
    #[serde(skip)]
    pub cache_semantic_ttl: Option<i64>,
    #[serde(skip)]
    pub cache_semantic_threshold: Option<f64>,
    pub is_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRoute {
    pub name: String,
    #[serde(alias = "vmodel")]
    pub virtual_model: String,
    pub strategy: Option<String>,
    pub target_provider: String,
    pub target_model: String,
    #[serde(default)]
    pub targets: Vec<CreateRouteTarget>,
    pub access_control: Option<bool>,
    #[serde(alias = "type")]
    pub route_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<RouteCacheConfig>,
    #[serde(skip)]
    pub cache_exact_ttl: Option<i64>,
    #[serde(skip)]
    pub cache_semantic_ttl: Option<i64>,
    #[serde(skip)]
    pub cache_semantic_threshold: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RouteCacheConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact: Option<RouteExactCacheConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic: Option<RouteSemanticCacheConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteExactCacheConfig {
    pub ttl: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteSemanticCacheConfig {
    pub ttl: Option<i64>,
    pub threshold: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRouteTarget {
    pub provider_id: String,
    pub model: String,
    pub weight: Option<i32>,
    pub priority: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertRouteTarget {
    pub id: Option<String>,
    pub provider_id: String,
    pub model: String,
    pub weight: Option<i32>,
    pub priority: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateApiKey {
    pub name: String,
    pub rpm: Option<i32>,
    pub rpd: Option<i32>,
    pub tpm: Option<i32>,
    pub tpd: Option<i32>,
    pub expires_at: Option<String>,
    #[serde(default)]
    pub route_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateApiKey {
    pub name: Option<String>,
    pub rpm: Option<i32>,
    pub rpd: Option<i32>,
    pub tpm: Option<i32>,
    pub tpd: Option<i32>,
    pub is_enabled: Option<bool>,
    pub expires_at: Option<String>,
    pub route_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LogQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub status_min: Option<i32>,
    pub status_max: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogPage {
    pub items: Vec<RequestLog>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, FromRow)]
pub struct StatsOverview {
    pub total_requests: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub avg_duration_ms: f64,
    pub error_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct StatsHourly {
    pub hour: String,
    pub request_count: i64,
    pub error_count: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub avg_duration_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ModelStats {
    pub model: String,
    pub request_count: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub avg_duration_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProviderStats {
    pub provider: String,
    pub request_count: i64,
    pub error_count: i64,
    pub avg_duration_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub success: bool,
    pub latency_ms: u64,
    pub model: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub provider: String,
    pub model_id: String,
    pub context_window: u64,
    pub embedding_length: Option<u64>,
    pub output_max_tokens: Option<u64>,
    pub tool_call: bool,
    pub reasoning: bool,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub input_cost: Option<f64>,
    pub output_cost: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportData {
    pub version: u32,
    pub providers: Vec<ExportProvider>,
    pub routes: Vec<ExportRoute>,
    pub settings: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportProvider {
    pub name: String,
    pub vendor: Option<String>,
    pub protocol: String,
    pub base_url: String,
    #[serde(default)]
    pub default_protocol: String,
    #[serde(default)]
    pub protocol_endpoints: String,
    pub preset_key: Option<String>,
    pub channel: Option<String>,
    #[serde(alias = "modelsEndpoint")]
    pub models_source: Option<String>,
    #[serde(alias = "capabilitiesSource")]
    pub capabilities_source: Option<String>,
    pub static_models: Option<String>,
    pub api_key: String,
    #[serde(default)]
    pub use_proxy: bool,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportRoute {
    pub name: String,
    pub virtual_model: String,
    pub target_model: String,
    #[serde(default)]
    pub access_control: bool,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub providers_imported: u32,
    pub routes_imported: u32,
    pub settings_imported: u32,
}

impl Provider {
    pub fn effective_models_source(&self) -> Option<&str> {
        self.models_source
            .as_deref()
            .filter(|v| !v.trim().is_empty())
    }

    /// Resolve effective default_protocol: new field > legacy `protocol`.
    pub fn effective_default_protocol(&self) -> &str {
        let dp = self.default_protocol.trim();
        if dp.is_empty() {
            self.protocol.trim()
        } else {
            dp
        }
    }

    /// Parse `protocol_endpoints` JSON into a map.
    /// Falls back to building a single-entry map from legacy `protocol`/`base_url`.
    pub fn parsed_protocol_endpoints(&self) -> HashMap<String, ProtocolEndpointEntry> {
        if !self.protocol_endpoints.trim().is_empty() && self.protocol_endpoints.trim() != "{}" {
            if let Ok(map) = serde_json::from_str::<HashMap<String, ProtocolEndpointEntry>>(&self.protocol_endpoints) {
                if !map.is_empty() {
                    return map;
                }
            }
        }
        let mut map = HashMap::new();
        if !self.protocol.trim().is_empty() && !self.base_url.trim().is_empty() {
            map.insert(
                self.protocol.trim().to_string(),
                ProtocolEndpointEntry {
                    base_url: self.base_url.trim().to_string(),
                },
            );
        }
        map
    }
}

impl Route {
    pub fn normalized_route_type(&self) -> &str {
        if self.route_type.trim().eq_ignore_ascii_case("embedding") {
            "embedding"
        } else {
            "chat"
        }
    }

    pub fn is_embedding_route(&self) -> bool {
        self.normalized_route_type() == "embedding"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolEndpointEntry {
    pub base_url: String,
}

impl CreateProvider {
    pub fn effective_models_source(&self) -> Option<&str> {
        self.models_source
            .as_deref()
            .filter(|v| !v.trim().is_empty())
    }
}

impl UpdateProvider {
    pub fn effective_models_source(&self) -> Option<&str> {
        self.models_source
            .as_deref()
            .filter(|v| !v.trim().is_empty())
    }
}
