use mongodb::bson::DateTime;
use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

fn default_active() -> String {
    "active".to_string()
}

fn default_openai() -> String {
    "openai".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderDocument {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub name_key: String,
    pub vendor: Option<String>,
    pub protocol: String,
    pub base_url: String,
    pub preset_key: Option<String>,
    #[serde(alias = "region", default)]
    pub channel: Option<String>,
    #[serde(alias = "modelsEndpoint", default)]
    pub models_endpoint: Option<String>,
    #[serde(alias = "modelsSource", default)]
    pub models_source: Option<String>,
    #[serde(alias = "capabilitiesSource", default)]
    pub capabilities_source: Option<String>,
    #[serde(default)]
    pub static_models: Option<String>,
    pub api_key: String,
    #[serde(default)]
    pub last_test_success: Option<bool>,
    #[serde(default)]
    pub last_test_at: Option<DateTime>,
    #[serde(default = "default_true")]
    pub is_active: bool,
    #[serde(default = "DateTime::now")]
    pub created_at: DateTime,
    #[serde(default = "DateTime::now")]
    pub updated_at: DateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteDocument {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub name_key: String,
    #[serde(default = "default_openai")]
    pub ingress_protocol: String,
    #[serde(alias = "match_pattern", default)]
    pub virtual_model: String,
    #[serde(default)]
    pub route_key: String,
    pub target_provider: String,
    pub target_model: String,
    #[serde(default)]
    pub access_control: bool,
    #[serde(default = "default_true")]
    pub is_active: bool,
    #[serde(default = "DateTime::now")]
    pub created_at: DateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyDocument {
    pub id: String,
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub name_key: String,
    #[serde(default)]
    pub rpm: Option<i32>,
    #[serde(default)]
    pub rpd: Option<i32>,
    #[serde(default)]
    pub tpm: Option<i32>,
    #[serde(default)]
    pub tpd: Option<i32>,
    #[serde(default = "default_active")]
    pub status: String,
    #[serde(default)]
    pub expires_at: Option<DateTime>,
    #[serde(default = "DateTime::now")]
    pub created_at: DateTime,
    #[serde(default = "DateTime::now")]
    pub updated_at: DateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyRouteBindingDocument {
    pub api_key_id: String,
    pub route_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLogDocument {
    pub id: String,
    #[serde(default = "DateTime::now")]
    pub created_at: DateTime,
    #[serde(default)]
    pub api_key_id: Option<String>,
    #[serde(default)]
    pub ingress_protocol: Option<String>,
    #[serde(default)]
    pub egress_protocol: Option<String>,
    #[serde(default)]
    pub request_model: Option<String>,
    #[serde(default)]
    pub actual_model: Option<String>,
    #[serde(default)]
    pub provider_name: Option<String>,
    #[serde(default)]
    pub status_code: Option<i32>,
    #[serde(default)]
    pub duration_ms: Option<f64>,
    #[serde(default)]
    pub input_tokens: i32,
    #[serde(default)]
    pub output_tokens: i32,
    #[serde(default)]
    pub is_stream: bool,
    #[serde(default)]
    pub is_tool_call: bool,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub request_preview: Option<String>,
    #[serde(default)]
    pub response_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingDocument {
    pub key: String,
    pub value: String,
    #[serde(default = "DateTime::now")]
    pub updated_at: DateTime,
}
