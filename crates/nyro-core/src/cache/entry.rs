use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::ir::AiResponse;
use crate::protocol::types::TokenUsage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub payload: Value,
    pub status_code: u16,
    pub provider_name: String,
    #[serde(default)]
    pub actual_model: Option<String>,
    pub usage: TokenUsage,
    pub created_at_epoch_ms: i64,
    #[serde(default)]
    pub internal_response: Option<AiResponse>,
}
