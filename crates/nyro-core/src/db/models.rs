use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub base_url: String,
    pub preset_key: Option<String>,
    #[serde(alias = "region")]
    pub channel: Option<String>,
    pub models_endpoint: Option<String>,
    pub static_models: Option<String>,
    #[serde(skip_serializing)]
    pub api_key: String,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Route {
    pub id: String,
    pub name: String,
    pub ingress_protocol: String,
    pub virtual_model: String,
    pub target_provider: String,
    pub target_model: String,
    pub access_control: bool,
    pub is_active: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiKey {
    pub id: String,
    pub key: String,
    pub name: String,
    pub rpm: Option<i32>,
    pub tpm: Option<i32>,
    pub tpd: Option<i32>,
    pub status: String,
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
    pub tpm: Option<i32>,
    pub tpd: Option<i32>,
    pub status: String,
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
    pub request_preview: Option<String>,
    pub response_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProvider {
    pub name: String,
    pub protocol: String,
    pub base_url: String,
    pub preset_key: Option<String>,
    #[serde(alias = "region")]
    pub channel: Option<String>,
    pub models_endpoint: Option<String>,
    pub static_models: Option<String>,
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProvider {
    pub name: Option<String>,
    pub protocol: Option<String>,
    pub base_url: Option<String>,
    pub preset_key: Option<String>,
    #[serde(alias = "region")]
    pub channel: Option<String>,
    pub models_endpoint: Option<String>,
    pub static_models: Option<String>,
    pub api_key: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRoute {
    pub name: Option<String>,
    pub ingress_protocol: Option<String>,
    pub virtual_model: Option<String>,
    pub target_provider: Option<String>,
    pub target_model: Option<String>,
    pub access_control: Option<bool>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRoute {
    pub name: String,
    pub ingress_protocol: String,
    pub virtual_model: String,
    pub target_provider: String,
    pub target_model: String,
    pub access_control: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateApiKey {
    pub name: String,
    pub rpm: Option<i32>,
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
    pub tpm: Option<i32>,
    pub tpd: Option<i32>,
    pub status: Option<String>,
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
pub struct ExportData {
    pub version: u32,
    pub providers: Vec<ExportProvider>,
    pub routes: Vec<ExportRoute>,
    pub settings: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportProvider {
    pub name: String,
    pub protocol: String,
    pub base_url: String,
    pub preset_key: Option<String>,
    #[serde(alias = "region")]
    pub channel: Option<String>,
    pub models_endpoint: Option<String>,
    pub static_models: Option<String>,
    pub api_key: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportRoute {
    pub name: String,
    #[serde(default = "default_ingress_protocol")]
    pub ingress_protocol: String,
    #[serde(alias = "match_pattern")]
    pub virtual_model: String,
    pub target_provider_name: String,
    pub target_model: String,
    #[serde(default)]
    pub access_control: bool,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub providers_imported: u32,
    pub routes_imported: u32,
    pub settings_imported: u32,
}

fn default_ingress_protocol() -> String {
    "openai".to_string()
}
