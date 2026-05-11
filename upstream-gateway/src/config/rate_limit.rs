use serde::{Deserialize, Serialize};

use crate::errors::RateLimitError;
use crate::runtime::UpstreamRateLimitSummary;

fn default_true() -> bool {
    true
}

fn default_weight() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayProviderRateLimitConfig {
    #[serde(default)]
    pub key_pool: Vec<GatewayKeyConfig>,
    pub daily_reset: DailyResetConfig,
    #[serde(default)]
    pub models: Vec<ModelRateLimitRule>,
    #[serde(skip, default)]
    #[doc(hidden)]
    pub enabled_key_slots_cache: Vec<EnabledKeySlot>,
    #[serde(skip, default)]
    #[doc(hidden)]
    pub total_weight_slots_cache: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayKeyConfig {
    pub id: String,
    pub api_key: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_weight")]
    pub weight: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnabledKeySlot {
    pub key_index: usize,
    pub start_slot: usize,
    pub end_slot_exclusive: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyResetConfig {
    pub timezone: String,
    pub hour: u8,
    pub minute: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRateLimitRule {
    pub model: String,
    pub rpm: Option<u32>,
    pub rpd: Option<u32>,
    pub tpm: Option<u32>,
    pub tpm_mode: Option<TpmMode>,
    pub tokenizer_encoding: Option<OpenAITokenizerEncoding>,
    pub tokenizer_model: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TpmMode {
    InputOnly,
    InputAndOutput,
}

impl Default for TpmMode {
    fn default() -> Self {
        Self::InputOnly
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpenAITokenizerEncoding {
    O200kHarmony,
    O200kBase,
    Cl100kBase,
}

impl OpenAITokenizerEncoding {
    pub fn representative_model(self) -> &'static str {
        match self {
            Self::O200kHarmony => "gpt-oss-120b",
            Self::O200kBase => "gpt-4o",
            Self::Cl100kBase => "gpt-4",
        }
    }

    pub fn bpe(self) -> Result<&'static tiktoken_rs::CoreBPE, RateLimitError> {
        let bpe = match self {
            Self::O200kHarmony | Self::O200kBase => tiktoken_rs::o200k_base_singleton(),
            Self::Cl100kBase => tiktoken_rs::cl100k_base_singleton(),
        };
        Ok(bpe)
    }

    pub fn fallback_candidates() -> &'static [Self] {
        &[Self::O200kHarmony, Self::O200kBase, Self::Cl100kBase]
    }
}

impl GatewayProviderRateLimitConfig {
    pub fn normalized(mut self) -> Result<Self, RateLimitError> {
        self.daily_reset.validate()?;
        if self.key_pool.is_empty() {
            return Err(RateLimitError::invalid_config(
                "key_pool cannot be empty when upstream_rate_limit is configured",
            ));
        }

        let mut seen_key_ids = std::collections::HashSet::new();
        for key in &mut self.key_pool {
            key.id = key.id.trim().to_string();
            key.api_key = key.api_key.trim().to_string();
            if key.id.is_empty() {
                return Err(RateLimitError::invalid_config(
                    "key_pool[].id cannot be empty",
                ));
            }
            if key.api_key.is_empty() {
                return Err(RateLimitError::invalid_config(format!(
                    "key_pool[{}].api_key cannot be empty",
                    key.id
                )));
            }
            if !seen_key_ids.insert(key.id.clone()) {
                return Err(RateLimitError::invalid_config(format!(
                    "duplicated key_pool id: {}",
                    key.id
                )));
            }
            if key.weight == 0 {
                key.weight = 1;
            }
        }

        let mut seen_models = std::collections::HashSet::new();
        let mut seen_wildcard = false;
        for rule in &mut self.models {
            rule.model = rule.model.trim().to_string();
            rule.tokenizer_model = rule
                .tokenizer_model
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string);
            if rule.model.is_empty() {
                return Err(RateLimitError::invalid_config(
                    "models[].model cannot be empty",
                ));
            }
            rule.rpm = normalize_limit(rule.rpm);
            rule.rpd = normalize_limit(rule.rpd);
            rule.tpm = normalize_limit(rule.tpm);
            if rule.tpm.is_none() {
                rule.tpm_mode = None;
            }
            if rule.model == "*" {
                if seen_wildcard {
                    return Err(RateLimitError::invalid_config(
                        "models contains duplicated wildcard rule '*'",
                    ));
                }
                seen_wildcard = true;
            } else if !seen_models.insert(rule.model.clone()) {
                return Err(RateLimitError::invalid_config(format!(
                    "duplicated model rule: {}",
                    rule.model
                )));
            }
        }

        self.refresh_enabled_key_slots_cache();

        Ok(self)
    }

    pub fn matching_rule(&self, actual_model: &str) -> Option<&ModelRateLimitRule> {
        self.models
            .iter()
            .find(|rule| rule.model == actual_model)
            .or_else(|| self.models.iter().find(|rule| rule.model == "*"))
    }

    pub fn needs_tpm_estimation(&self, actual_model: &str) -> bool {
        self.matching_rule(actual_model)
            .and_then(|rule| rule.tpm)
            .is_some()
    }

    pub fn summary(&self) -> UpstreamRateLimitSummary {
        UpstreamRateLimitSummary {
            key_count: self.key_pool.len() as u32,
            enabled_key_count: self.key_pool.iter().filter(|key| key.enabled).count() as u32,
            model_rule_count: self.models.len() as u32,
            has_tpm: self.models.iter().any(|rule| rule.tpm.unwrap_or(0) > 0),
            has_rpm: self.models.iter().any(|rule| rule.rpm.unwrap_or(0) > 0),
            has_rpd: self.models.iter().any(|rule| rule.rpd.unwrap_or(0) > 0),
        }
    }

    fn refresh_enabled_key_slots_cache(&mut self) {
        self.enabled_key_slots_cache.clear();
        self.enabled_key_slots_cache.reserve(self.key_pool.len());

        let mut next_slot = 0usize;
        for (key_index, key) in self.key_pool.iter().enumerate() {
            if !key.enabled {
                continue;
            }

            let weight = usize::try_from(key.weight.max(1)).unwrap_or(usize::MAX);
            let end_slot_exclusive = next_slot.saturating_add(weight);
            self.enabled_key_slots_cache.push(EnabledKeySlot {
                key_index,
                start_slot: next_slot,
                end_slot_exclusive,
            });
            next_slot = end_slot_exclusive;
        }

        self.total_weight_slots_cache = next_slot;
    }
}

impl DailyResetConfig {
    pub fn validate(&self) -> Result<(), RateLimitError> {
        let tz = self.timezone.trim();
        if tz.is_empty() {
            return Err(RateLimitError::invalid_config(
                "daily_reset.timezone cannot be empty",
            ));
        }
        parse_fixed_offset_seconds(tz)?;
        if self.hour > 23 {
            return Err(RateLimitError::invalid_config(
                "daily_reset.hour must be between 0 and 23",
            ));
        }
        if self.minute > 59 {
            return Err(RateLimitError::invalid_config(
                "daily_reset.minute must be between 0 and 59",
            ));
        }
        Ok(())
    }

    pub fn offset_seconds(&self) -> Result<i32, RateLimitError> {
        parse_fixed_offset_seconds(self.timezone.trim())
    }
}

fn normalize_limit(limit: Option<u32>) -> Option<u32> {
    match limit {
        Some(0) | None => None,
        Some(value) => Some(value),
    }
}

fn parse_fixed_offset_seconds(raw: &str) -> Result<i32, RateLimitError> {
    if raw.eq_ignore_ascii_case("utc") || raw == "Z" || raw == "+00:00" || raw == "-00:00" {
        return Ok(0);
    }
    let sign = match raw.as_bytes().first().copied() {
        Some(b'+') => 1,
        Some(b'-') => -1,
        _ => {
            return Err(RateLimitError::invalid_config(format!(
                "unsupported daily_reset.timezone: {raw}"
            )));
        }
    };
    let mut parts = raw[1..].split(':');
    let hour = parts
        .next()
        .ok_or_else(|| {
            RateLimitError::invalid_config(format!("unsupported daily_reset.timezone: {raw}"))
        })?
        .parse::<i32>()
        .map_err(|_| {
            RateLimitError::invalid_config(format!("unsupported daily_reset.timezone: {raw}"))
        })?;
    let minute = parts
        .next()
        .ok_or_else(|| {
            RateLimitError::invalid_config(format!("unsupported daily_reset.timezone: {raw}"))
        })?
        .parse::<i32>()
        .map_err(|_| {
            RateLimitError::invalid_config(format!("unsupported daily_reset.timezone: {raw}"))
        })?;
    if parts.next().is_some() || hour > 23 || minute > 59 {
        return Err(RateLimitError::invalid_config(format!(
            "unsupported daily_reset.timezone: {raw}"
        )));
    }
    Ok(sign * (hour * 3600 + minute * 60))
}
