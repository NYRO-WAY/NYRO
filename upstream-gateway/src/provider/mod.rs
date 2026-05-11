use serde::{Deserialize, Serialize};

use crate::config::{GatewayKeyConfig, GatewayProviderRateLimitConfig, ModelRateLimitRule};
use crate::errors::RateLimitError;
use crate::runtime::UpstreamRateLimitSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderVendor {
    OpenAI,
    Anthropic,
    Gemini,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamProtocol {
    OpenAIChatCompletions,
    OpenAIResponses,
    OpenAIEmbeddings,
    AnthropicMessages,
    GoogleGenerateContent,
    GoogleStreamGenerateContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GatewayAuthStrategy {
    Bearer,
    HeaderApiKey { header_name: String },
    QueryApiKey { parameter_name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayProvider {
    pub id: String,
    pub name: String,
    pub vendor: ProviderVendor,
    pub base_url: String,
    pub auth_strategy: GatewayAuthStrategy,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayKey {
    pub id: String,
    pub provider_id: String,
    pub display_name: Option<String>,
    pub api_key: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub weight: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayModelRule {
    pub provider_id: String,
    #[serde(flatten)]
    pub rule: ModelRateLimitRule,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayProviderBundle {
    pub provider: GatewayProvider,
    #[serde(default)]
    pub keys: Vec<GatewayKey>,
    #[serde(default)]
    pub model_rules: Vec<GatewayModelRule>,
    pub daily_reset: crate::config::DailyResetConfig,
    #[serde(skip, default)]
    #[doc(hidden)]
    pub normalized_rate_limit_config_cache: Option<GatewayProviderRateLimitConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayProviderSummary {
    pub id: String,
    pub name: String,
    pub vendor: ProviderVendor,
    pub enabled: bool,
    pub base_url: String,
    pub rate_limit: UpstreamRateLimitSummary,
}

impl GatewayProviderBundle {
    pub fn normalized_rate_limit_config(
        &self,
    ) -> Result<&GatewayProviderRateLimitConfig, RateLimitError> {
        self.normalized_rate_limit_config_cache
            .as_ref()
            .ok_or_else(|| {
                RateLimitError::invalid_config(
                    "normalized rate limit cache is missing; bundle must be normalized before use",
                )
            })
    }

    pub fn rate_limit_config(&self) -> GatewayProviderRateLimitConfig {
        GatewayProviderRateLimitConfig {
            key_pool: self
                .keys
                .iter()
                .map(|key| GatewayKeyConfig {
                    id: key.id.clone(),
                    api_key: key.api_key.clone(),
                    enabled: key.enabled,
                    weight: key.weight.unwrap_or(1),
                })
                .collect(),
            daily_reset: self.daily_reset.clone(),
            models: self
                .model_rules
                .iter()
                .map(|rule| rule.rule.clone())
                .collect(),
            enabled_key_slots_cache: Vec::new(),
            total_weight_slots_cache: 0,
        }
    }

    pub fn normalized_rate_limit_config_cloned(
        &self,
    ) -> Result<GatewayProviderRateLimitConfig, RateLimitError> {
        self.normalized_rate_limit_config().cloned()
    }

    pub fn refresh_rate_limit_config_cache(&mut self) -> Result<(), RateLimitError> {
        self.normalized_rate_limit_config_cache = Some(self.rate_limit_config().normalized()?);
        Ok(())
    }

    pub fn summary(&self) -> GatewayProviderSummary {
        let rate_limit = self
            .normalized_rate_limit_config()
            .map(|config| config.summary())
            .unwrap_or_else(|_| self.rate_limit_config().summary());
        GatewayProviderSummary {
            id: self.provider.id.clone(),
            name: self.provider.name.clone(),
            vendor: self.provider.vendor,
            enabled: self.provider.enabled,
            base_url: self.provider.base_url.clone(),
            rate_limit,
        }
    }
}

fn default_true() -> bool {
    true
}
