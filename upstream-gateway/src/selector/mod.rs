use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use chrono::{Datelike, FixedOffset, TimeZone, Timelike};
use dashmap::DashMap;
use parking_lot::Mutex;

use crate::config::{
    DailyResetConfig, EnabledKeySlot, GatewayKeyConfig, GatewayProviderRateLimitConfig,
    ModelRateLimitRule, TpmMode,
};
use crate::errors::RateLimitError;
use crate::runtime::{
    KeyModelRuntimeSnapshot, KeySelectionInput, ProviderModelRuntimeSnapshot, RateLimitLease,
    RateLimitMetricSnapshot, SelectedUpstreamKey, SettlementUsage,
    UpstreamRateLimitRuntimeSnapshot,
};

pub trait KeySelector: Send + Sync {
    fn select_key(
        &self,
        config: &GatewayProviderRateLimitConfig,
        input: &KeySelectionInput<'_>,
    ) -> Result<SelectedUpstreamKey, RateLimitError>;
}

#[derive(Debug)]
pub struct DefaultKeySelector {
    state: Arc<RuntimeState>,
}

impl DefaultKeySelector {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn with_state(state: Arc<RuntimeState>) -> Self {
        Self { state }
    }

    pub fn select_key_at(
        &self,
        config: &GatewayProviderRateLimitConfig,
        input: &KeySelectionInput<'_>,
        now_ms: i64,
    ) -> Result<SelectedUpstreamKey, RateLimitError> {
        let enabled_keys = &config.enabled_key_slots_cache;
        let total_slots = config.total_weight_slots_cache;
        if total_slots == 0 {
            return Err(RateLimitError::NoAvailableKey {
                provider_name: input.provider_name.to_string(),
                model: input.actual_model.to_string(),
                reason: ": no enabled keys in key pool".to_string(),
            });
        }

        let rule = config.matching_rule(input.actual_model);
        let tpm_mode = rule
            .and_then(|value| value.tpm_mode)
            .unwrap_or(TpmMode::InputOnly);
        let reserved_output_tokens = if matches!(tpm_mode, TpmMode::InputAndOutput) {
            input.request_output_reservation
        } else {
            0
        };
        let reserved_total_tokens = input
            .request_input_tokens
            .saturating_add(reserved_output_tokens);

        let provider_model_key = ProviderModelCursorKey {
            provider_id: input.provider_id.to_string(),
            model: input.actual_model.to_string(),
        };
        let provider_model_state = self.state.provider_model_state(&provider_model_key);
        let start_slot = {
            let mut cursor = provider_model_state.cursor.lock();
            let start_slot = (*cursor) % total_slots;
            *cursor = (*cursor + 1) % total_slots;
            start_slot
        };
        let Some(start_index) = key_index_for_slot(&enabled_keys, start_slot) else {
            return Err(RateLimitError::NoAvailableKey {
                provider_name: input.provider_name.to_string(),
                model: input.actual_model.to_string(),
                reason: ": failed to map weighted cursor to enabled key".to_string(),
            });
        };
        let mut first_reason: Option<String> = None;

        for offset in 0..enabled_keys.len() {
            let idx = (start_index + offset) % enabled_keys.len();
            let weighted_key = &enabled_keys[idx];
            let key = &config.key_pool[weighted_key.key_index];
            let key_state_handle = provider_model_state.key_state(&key.id);
            let mut key_state = key_state_handle.lock();
            key_state.cleanup_sliding_windows(now_ms);
            key_state.refresh_rpd_window(now_ms, &config.daily_reset)?;

            if let Some(limit) = rule.and_then(|value| value.rpm) {
                if key_state.current_rpm_count() >= limit {
                    first_reason
                        .get_or_insert_with(|| format!("rpm quota exceeded on key '{}'", key.id));
                    continue;
                }
            }
            if let Some(limit) = rule.and_then(|value| value.rpd) {
                if key_state.rpd_count >= limit {
                    first_reason
                        .get_or_insert_with(|| format!("rpd quota exceeded on key '{}'", key.id));
                    continue;
                }
            }
            if let Some(limit) = rule.and_then(|value| value.tpm) {
                if key_state
                    .current_window_tokens()
                    .saturating_add(reserved_total_tokens)
                    > limit
                {
                    first_reason
                        .get_or_insert_with(|| format!("tpm quota exceeded on key '{}'", key.id));
                    continue;
                }
            }

            let lease_id = uuid::Uuid::new_v4().to_string();
            let request_started_at_ms = now_ms;
            let lease = RateLimitLease {
                lease_id: lease_id.clone(),
                provider_id: input.provider_id.to_string(),
                key_id: key.id.clone(),
                model: input.actual_model.to_string(),
                reserved_input_tokens: input.request_input_tokens,
                reserved_output_tokens,
                tpm_mode,
                request_started_at_ms,
            };

            if rule.and_then(|value| value.rpm).is_some() {
                key_state.rpm_events.push_back(RequestEvent {
                    lease_id: lease_id.clone(),
                    timestamp_ms: now_ms,
                });
                key_state.record_rpm_reservation(&lease_id);
            }
            if rule.and_then(|value| value.rpd).is_some() {
                key_state.rpd_count = key_state.rpd_count.saturating_add(1);
            }
            if rule.and_then(|value| value.tpm).is_some() {
                key_state.tpm_events.push_back(TokenEvent {
                    lease_id: lease_id.clone(),
                    timestamp_ms: now_ms,
                    tokens: reserved_total_tokens,
                });
                key_state.record_tpm_reservation(&lease_id, reserved_total_tokens);
            }
            key_state.active_lease_count = key_state.active_lease_count.saturating_add(1);
            let rpd_window_id = key_state.rpd_window_id.clone();
            drop(key_state);

            self.state.active_leases.insert(
                lease_id.clone(),
                ActiveLeaseState {
                    provider_model_key: provider_model_key.clone(),
                    provider_model_state: provider_model_state.clone(),
                    key_id: key.id.clone(),
                    key_state: key_state_handle,
                    rpd_window_id,
                },
            );
            let selected_slot = if idx == start_index {
                start_slot
            } else {
                weighted_key.start_slot
            };
            {
                let mut cursor = provider_model_state.cursor.lock();
                *cursor = (selected_slot + 1) % total_slots;
            }

            return Ok(SelectedUpstreamKey {
                key_id: key.id.clone(),
                api_key: key.api_key.clone(),
                lease,
            });
        }

        Err(RateLimitError::NoAvailableKey {
            provider_name: input.provider_name.to_string(),
            model: input.actual_model.to_string(),
            reason: first_reason
                .map(|reason| format!(": {reason}"))
                .unwrap_or_default(),
        })
    }

    pub fn runtime_snapshot(
        &self,
        provider_id: &str,
        config: &GatewayProviderRateLimitConfig,
    ) -> Result<UpstreamRateLimitRuntimeSnapshot, RateLimitError> {
        self.runtime_snapshot_at(provider_id, config, chrono::Utc::now().timestamp_millis())
    }

    pub fn runtime_snapshot_at(
        &self,
        provider_id: &str,
        config: &GatewayProviderRateLimitConfig,
        now_ms: i64,
    ) -> Result<UpstreamRateLimitRuntimeSnapshot, RateLimitError> {
        let models = self.runtime_models_for_provider(provider_id, config);
        let mut snapshots = Vec::with_capacity(models.len());

        for model in models {
            snapshots.push(self.model_runtime_snapshot_at(provider_id, &model, config, now_ms)?);
        }

        Ok(UpstreamRateLimitRuntimeSnapshot {
            captured_at_ms: now_ms,
            models: snapshots,
        })
    }

    pub fn settle_at(
        &self,
        lease: &RateLimitLease,
        usage: SettlementUsage,
        _now_ms: i64,
    ) -> Result<(), RateLimitError> {
        let (_, active) = self
            .state
            .active_leases
            .remove(&lease.lease_id)
            .ok_or_else(|| RateLimitError::LeaseNotFound {
                lease_id: lease.lease_id.clone(),
            })?;
        let mut key_state = active.key_state.lock();
        let actual_input = if usage.input_tokens == 0 {
            lease.reserved_input_tokens
        } else {
            usage.input_tokens
        };
        let actual_output = if matches!(lease.tpm_mode, TpmMode::InputAndOutput) {
            usage.output_tokens
        } else {
            0
        };
        let actual_total_tokens = if matches!(lease.tpm_mode, TpmMode::InputAndOutput) {
            actual_input.saturating_add(actual_output)
        } else {
            actual_input
        };
        key_state.record_tpm_settlement(
            &lease.lease_id,
            reserved_total_tokens_for_lease(lease),
            actual_total_tokens,
        );
        if key_state.active_lease_count > 0 {
            key_state.active_lease_count -= 1;
        }
        let should_cleanup = key_state.is_prunable();
        drop(key_state);
        if should_cleanup {
            self.state.cleanup_idle_key_state(&active);
        }
        Ok(())
    }

    pub fn rollback_at(&self, lease: &RateLimitLease, _now_ms: i64) -> Result<(), RateLimitError> {
        let (_, active) = self
            .state
            .active_leases
            .remove(&lease.lease_id)
            .ok_or_else(|| RateLimitError::LeaseNotFound {
                lease_id: lease.lease_id.clone(),
            })?;
        let mut key_state = active.key_state.lock();
        key_state.rollback_rpm_lease(&lease.lease_id);
        key_state.rollback_tpm_lease(&lease.lease_id, reserved_total_tokens_for_lease(lease));
        if active.rpd_window_id.is_some()
            && active.rpd_window_id == key_state.rpd_window_id
            && key_state.rpd_count > 0
        {
            key_state.rpd_count -= 1;
        }
        if key_state.active_lease_count > 0 {
            key_state.active_lease_count -= 1;
        }
        let should_cleanup = key_state.is_prunable();
        drop(key_state);
        if should_cleanup {
            self.state.cleanup_idle_key_state(&active);
        }
        Ok(())
    }

    fn runtime_models_for_provider(
        &self,
        provider_id: &str,
        config: &GatewayProviderRateLimitConfig,
    ) -> Vec<String> {
        let mut models = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for rule in &config.models {
            if seen.insert(rule.model.clone()) {
                models.push(rule.model.clone());
            }
        }

        let mut observed_models = self
            .state
            .provider_models
            .iter()
            .filter(|entry| entry.key().provider_id == provider_id)
            .map(|entry| entry.key().model.clone())
            .collect::<Vec<_>>();
        observed_models.sort();

        for model in observed_models {
            if seen.insert(model.clone()) {
                models.push(model);
            }
        }

        models
    }

    fn model_runtime_snapshot_at(
        &self,
        provider_id: &str,
        model: &str,
        config: &GatewayProviderRateLimitConfig,
        now_ms: i64,
    ) -> Result<ProviderModelRuntimeSnapshot, RateLimitError> {
        let provider_model_key = ProviderModelCursorKey {
            provider_id: provider_id.to_string(),
            model: model.to_string(),
        };
        let provider_model_state = self
            .state
            .provider_models
            .get(&provider_model_key)
            .map(|entry| entry.clone());
        let next_cursor = provider_model_state
            .as_ref()
            .map(|state| *state.cursor.lock())
            .unwrap_or(0);
        let rule = if model == "*" {
            config
                .models
                .iter()
                .find(|candidate| candidate.model == "*")
        } else {
            config.matching_rule(model)
        };
        let enabled_key_count = config.key_pool.iter().filter(|key| key.enabled).count() as u32;
        let mut available_key_count = 0u32;
        let mut keys = Vec::with_capacity(config.key_pool.len());

        for key in &config.key_pool {
            let key_state_handle = provider_model_state
                .as_ref()
                .and_then(|state| state.key_models.get(&key.id).map(|entry| entry.clone()));
            let key_snapshot =
                self.key_runtime_snapshot_at(key, key_state_handle.as_ref(), config, rule, now_ms)?;
            if key_snapshot.available {
                available_key_count = available_key_count.saturating_add(1);
            }
            keys.push(key_snapshot);
        }

        Ok(ProviderModelRuntimeSnapshot {
            model: model.to_string(),
            matched_rule_model: rule.map(|value| value.model.clone()),
            tpm_mode: rule.and_then(|value| value.tpm_mode),
            rpm_limit: rule.and_then(|value| value.rpm),
            rpd_limit: rule.and_then(|value| value.rpd),
            tpm_limit: rule.and_then(|value| value.tpm),
            key_count: config.key_pool.len() as u32,
            enabled_key_count,
            available_key_count,
            next_cursor,
            keys,
        })
    }

    fn key_runtime_snapshot_at(
        &self,
        key: &GatewayKeyConfig,
        key_state_handle: Option<&Arc<Mutex<KeyModelState>>>,
        config: &GatewayProviderRateLimitConfig,
        rule: Option<&ModelRateLimitRule>,
        now_ms: i64,
    ) -> Result<KeyModelRuntimeSnapshot, RateLimitError> {
        let (rpm_used, rpd_used, tpm_used, active_lease_count, rpd_window_id) =
            if let Some(handle) = key_state_handle {
                let mut key_state = handle.lock();
                key_state.cleanup_sliding_windows(now_ms);
                key_state.refresh_rpd_window(now_ms, &config.daily_reset)?;
                (
                    key_state.current_rpm_count(),
                    key_state.rpd_count,
                    key_state.current_window_tokens(),
                    key_state.active_lease_count,
                    key_state.rpd_window_id.clone(),
                )
            } else {
                (0, 0, 0, 0, None)
            };

        let rpm_limit = rule.and_then(|value| value.rpm);
        let rpd_limit = rule.and_then(|value| value.rpd);
        let tpm_limit = rule.and_then(|value| value.tpm);
        let blocked_reason = availability_reason(
            key.enabled,
            rpm_used,
            rpm_limit,
            rpd_used,
            rpd_limit,
            tpm_used,
            tpm_limit,
        );

        Ok(KeyModelRuntimeSnapshot {
            key_id: key.id.clone(),
            enabled: key.enabled,
            available: blocked_reason.is_none(),
            blocked_reason,
            active_lease_count,
            rpm: metric_snapshot(rpm_used, rpm_limit),
            rpd: metric_snapshot(rpd_used, rpd_limit),
            tpm: metric_snapshot(tpm_used, tpm_limit),
            rpd_window_id,
        })
    }
}

impl Default for DefaultKeySelector {
    fn default() -> Self {
        Self {
            state: Arc::new(RuntimeState::default()),
        }
    }
}

impl KeySelector for DefaultKeySelector {
    fn select_key(
        &self,
        config: &GatewayProviderRateLimitConfig,
        input: &KeySelectionInput<'_>,
    ) -> Result<SelectedUpstreamKey, RateLimitError> {
        self.select_key_at(config, input, chrono::Utc::now().timestamp_millis())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ProviderModelCursorKey {
    pub provider_id: String,
    pub model: String,
}

#[derive(Debug, Default)]
pub struct RuntimeState {
    provider_models: DashMap<ProviderModelCursorKey, Arc<ProviderModelState>>,
    active_leases: DashMap<String, ActiveLeaseState>,
}

impl RuntimeState {
    fn provider_model_state(&self, key: &ProviderModelCursorKey) -> Arc<ProviderModelState> {
        self.provider_models
            .entry(key.clone())
            .or_insert_with(|| Arc::new(ProviderModelState::default()))
            .clone()
    }

    fn cleanup_idle_key_state(&self, active: &ActiveLeaseState) {
        let removed = active
            .provider_model_state
            .key_models
            .remove_if(&active.key_id, |_, value| {
                Arc::ptr_eq(value, &active.key_state)
            });
        if removed.is_none() {
            return;
        }

        let _ = self
            .provider_models
            .remove_if(&active.provider_model_key, |_, value| {
                Arc::ptr_eq(value, &active.provider_model_state) && value.key_models.is_empty()
            });
    }
}

#[derive(Debug, Default)]
pub(crate) struct ProviderModelState {
    cursor: Mutex<usize>,
    key_models: DashMap<String, Arc<Mutex<KeyModelState>>>,
}

impl ProviderModelState {
    fn key_state(&self, key_id: &str) -> Arc<Mutex<KeyModelState>> {
        self.key_models
            .entry(key_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(KeyModelState::default())))
            .clone()
    }
}

#[derive(Debug, Default)]
pub(crate) struct KeyModelState {
    rpm_events: VecDeque<RequestEvent>,
    tpm_events: VecDeque<TokenEvent>,
    rpm_count: u32,
    tpm_tokens_in_window: u32,
    active_rpm_leases: HashSet<String>,
    active_tpm_leases: HashSet<String>,
    rolled_back_rpm_leases: HashSet<String>,
    rolled_back_tpm_leases: HashSet<String>,
    settled_tpm_overrides: HashMap<String, u32>,
    rpd_window_id: Option<String>,
    rpd_count: u32,
    active_lease_count: u32,
}

impl KeyModelState {
    fn cleanup_sliding_windows(&mut self, now_ms: i64) {
        let cutoff = now_ms.saturating_sub(60_000);
        while matches!(
            self.rpm_events.front(),
            Some(event) if event.timestamp_ms <= cutoff
        ) {
            if let Some(event) = self.rpm_events.pop_front() {
                if self.rolled_back_rpm_leases.remove(&event.lease_id) {
                    continue;
                }
                if self.active_rpm_leases.remove(&event.lease_id) && self.rpm_count > 0 {
                    self.rpm_count -= 1;
                }
            }
        }
        while matches!(
            self.tpm_events.front(),
            Some(event) if event.timestamp_ms <= cutoff
        ) {
            if let Some(event) = self.tpm_events.pop_front() {
                if self.rolled_back_tpm_leases.remove(&event.lease_id) {
                    self.settled_tpm_overrides.remove(&event.lease_id);
                    continue;
                }
                let tokens = self
                    .settled_tpm_overrides
                    .remove(&event.lease_id)
                    .unwrap_or(event.tokens);
                self.active_tpm_leases.remove(&event.lease_id);
                self.tpm_tokens_in_window = self.tpm_tokens_in_window.saturating_sub(tokens);
            }
        }
    }

    fn refresh_rpd_window(
        &mut self,
        now_ms: i64,
        reset: &DailyResetConfig,
    ) -> Result<(), RateLimitError> {
        let next_window = daily_window_id(now_ms, reset)?;
        if self.rpd_window_id.as_deref() != Some(next_window.as_str()) {
            self.rpd_window_id = Some(next_window);
            self.rpd_count = 0;
        }
        Ok(())
    }

    fn current_window_tokens(&self) -> u32 {
        self.tpm_tokens_in_window
    }

    fn current_rpm_count(&self) -> u32 {
        self.rpm_count
    }

    fn record_rpm_reservation(&mut self, lease_id: &str) {
        self.active_rpm_leases.insert(lease_id.to_string());
        self.rpm_count = self.rpm_count.saturating_add(1);
    }

    fn record_tpm_reservation(&mut self, lease_id: &str, reserved_tokens: u32) {
        self.active_tpm_leases.insert(lease_id.to_string());
        self.tpm_tokens_in_window = self.tpm_tokens_in_window.saturating_add(reserved_tokens);
    }

    fn record_tpm_settlement(&mut self, lease_id: &str, reserved_tokens: u32, actual_tokens: u32) {
        if !self.active_tpm_leases.contains(lease_id) {
            self.settled_tpm_overrides.remove(lease_id);
            return;
        }

        self.tpm_tokens_in_window = self
            .tpm_tokens_in_window
            .saturating_sub(reserved_tokens)
            .saturating_add(actual_tokens);
        if actual_tokens == reserved_tokens {
            self.settled_tpm_overrides.remove(lease_id);
        } else {
            self.settled_tpm_overrides
                .insert(lease_id.to_string(), actual_tokens);
        }
    }

    fn rollback_rpm_lease(&mut self, lease_id: &str) {
        if self.active_rpm_leases.remove(lease_id) {
            self.rpm_count = self.rpm_count.saturating_sub(1);
            self.rolled_back_rpm_leases.insert(lease_id.to_string());
        }
    }

    fn rollback_tpm_lease(&mut self, lease_id: &str, reserved_tokens: u32) {
        if self.active_tpm_leases.remove(lease_id) {
            self.tpm_tokens_in_window = self.tpm_tokens_in_window.saturating_sub(reserved_tokens);
            self.rolled_back_tpm_leases.insert(lease_id.to_string());
        }
        self.settled_tpm_overrides.remove(lease_id);
    }

    fn is_prunable(&self) -> bool {
        self.active_lease_count == 0
            && self.rpm_events.is_empty()
            && self.tpm_events.is_empty()
            && self.rpd_count == 0
    }
}

#[derive(Debug)]
struct ActiveLeaseState {
    provider_model_key: ProviderModelCursorKey,
    provider_model_state: Arc<ProviderModelState>,
    key_id: String,
    key_state: Arc<Mutex<KeyModelState>>,
    rpd_window_id: Option<String>,
}

#[derive(Debug)]
struct RequestEvent {
    lease_id: String,
    timestamp_ms: i64,
}

#[derive(Debug)]
struct TokenEvent {
    lease_id: String,
    timestamp_ms: i64,
    tokens: u32,
}

fn reserved_total_tokens_for_lease(lease: &RateLimitLease) -> u32 {
    if matches!(lease.tpm_mode, TpmMode::InputAndOutput) {
        lease
            .reserved_input_tokens
            .saturating_add(lease.reserved_output_tokens)
    } else {
        lease.reserved_input_tokens
    }
}

fn key_index_for_slot(keys: &[EnabledKeySlot], slot: usize) -> Option<usize> {
    if keys.is_empty() {
        return None;
    }

    let idx = keys.partition_point(|entry| entry.end_slot_exclusive <= slot);
    if idx >= keys.len() {
        return None;
    }

    let entry = &keys[idx];
    if slot >= entry.start_slot && slot < entry.end_slot_exclusive {
        Some(idx)
    } else {
        None
    }
}

fn metric_snapshot(used: u32, limit: Option<u32>) -> RateLimitMetricSnapshot {
    RateLimitMetricSnapshot {
        used,
        limit,
        ratio: limit.map(|value| used as f64 / value as f64),
    }
}

fn availability_reason(
    enabled: bool,
    rpm_used: u32,
    rpm_limit: Option<u32>,
    rpd_used: u32,
    rpd_limit: Option<u32>,
    tpm_used: u32,
    tpm_limit: Option<u32>,
) -> Option<String> {
    if !enabled {
        return Some("key disabled".to_string());
    }
    if let Some(limit) = rpm_limit {
        if rpm_used >= limit {
            return Some("rpm quota exceeded".to_string());
        }
    }
    if let Some(limit) = rpd_limit {
        if rpd_used >= limit {
            return Some("rpd quota exceeded".to_string());
        }
    }
    if let Some(limit) = tpm_limit {
        if tpm_used >= limit {
            return Some("tpm quota exceeded".to_string());
        }
    }
    None
}

fn daily_window_id(now_ms: i64, reset: &DailyResetConfig) -> Result<String, RateLimitError> {
    let offset = FixedOffset::east_opt(reset.offset_seconds()?)
        .ok_or_else(|| RateLimitError::invalid_config("invalid daily_reset.timezone offset"))?;
    let now = offset
        .timestamp_millis_opt(now_ms)
        .single()
        .ok_or_else(|| {
            RateLimitError::invalid_config("invalid current timestamp for daily reset")
        })?;
    let start = offset
        .with_ymd_and_hms(
            now.year(),
            now.month(),
            now.day(),
            reset.hour as u32,
            reset.minute as u32,
            0,
        )
        .single()
        .ok_or_else(|| RateLimitError::invalid_config("invalid daily reset wall clock"))?;
    let window_start = if (now.hour(), now.minute()) < (reset.hour as u32, reset.minute as u32) {
        start - chrono::Duration::days(1)
    } else {
        start
    };
    Ok(window_start.timestamp_millis().to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{TimeZone, Utc};

    use crate::config::{
        DailyResetConfig, GatewayKeyConfig, GatewayProviderRateLimitConfig, ModelRateLimitRule,
    };

    use super::*;

    fn sample_config() -> GatewayProviderRateLimitConfig {
        GatewayProviderRateLimitConfig {
            key_pool: vec![GatewayKeyConfig {
                id: "key-a".to_string(),
                api_key: "sk-a".to_string(),
                enabled: true,
                weight: 1,
            }],
            daily_reset: DailyResetConfig {
                timezone: "+00:00".to_string(),
                hour: 0,
                minute: 0,
            },
            models: vec![ModelRateLimitRule {
                model: "*".to_string(),
                rpm: Some(10),
                rpd: Some(10),
                tpm: Some(10_000),
                tpm_mode: Some(TpmMode::InputOnly),
                tokenizer_encoding: None,
                tokenizer_model: None,
            }],
            enabled_key_slots_cache: Vec::new(),
            total_weight_slots_cache: 0,
        }
        .normalized()
        .unwrap()
    }

    #[test]
    fn different_provider_model_shards_do_not_share_cursor_lock() {
        let state = Arc::new(RuntimeState::default());
        let blocked_key = ProviderModelCursorKey {
            provider_id: "provider-1".to_string(),
            model: "gpt-4o".to_string(),
        };
        let blocked_state = state.provider_model_state(&blocked_key);
        let _blocked_cursor = blocked_state.cursor.lock();

        let selector = DefaultKeySelector::with_state(state);
        let config = sample_config();
        let now = Utc
            .with_ymd_and_hms(2026, 5, 5, 12, 0, 0)
            .unwrap()
            .timestamp_millis();
        let input = KeySelectionInput {
            provider_id: "provider-2",
            provider_name: "Provider 2",
            actual_model: "gpt-4o",
            request_input_tokens: 32,
            request_output_reservation: 0,
        };

        let selected = selector.select_key_at(&config, &input, now).unwrap();
        assert_eq!(selected.key_id, "key-a");
    }

    #[test]
    fn idle_key_state_and_provider_model_are_pruned_after_settle() {
        let state = Arc::new(RuntimeState::default());
        let selector = DefaultKeySelector::with_state(state.clone());
        let config = GatewayProviderRateLimitConfig {
            key_pool: vec![GatewayKeyConfig {
                id: "key-a".to_string(),
                api_key: "sk-a".to_string(),
                enabled: true,
                weight: 1,
            }],
            daily_reset: DailyResetConfig {
                timezone: "+00:00".to_string(),
                hour: 0,
                minute: 0,
            },
            models: vec![],
            enabled_key_slots_cache: Vec::new(),
            total_weight_slots_cache: 0,
        }
        .normalized()
        .unwrap();
        let now = Utc
            .with_ymd_and_hms(2026, 5, 5, 12, 0, 0)
            .unwrap()
            .timestamp_millis();
        let input = KeySelectionInput {
            provider_id: "provider-9",
            provider_name: "Provider 9",
            actual_model: "gpt-4o",
            request_input_tokens: 8,
            request_output_reservation: 0,
        };

        let selected = selector.select_key_at(&config, &input, now).unwrap();
        let key = ProviderModelCursorKey {
            provider_id: "provider-9".to_string(),
            model: "gpt-4o".to_string(),
        };
        assert!(state.provider_models.contains_key(&key));

        selector
            .settle_at(
                &selected.lease,
                SettlementUsage {
                    input_tokens: 8,
                    output_tokens: 0,
                },
                now,
            )
            .unwrap();

        assert!(!state.provider_models.contains_key(&key));
    }

    #[test]
    fn runtime_snapshot_reports_usage_and_block_reason() {
        let state = Arc::new(RuntimeState::default());
        let selector = DefaultKeySelector::with_state(state);
        let config = GatewayProviderRateLimitConfig {
            key_pool: vec![
                GatewayKeyConfig {
                    id: "key-a".to_string(),
                    api_key: "sk-a".to_string(),
                    enabled: true,
                    weight: 1,
                },
                GatewayKeyConfig {
                    id: "key-b".to_string(),
                    api_key: "sk-b".to_string(),
                    enabled: false,
                    weight: 1,
                },
            ],
            daily_reset: DailyResetConfig {
                timezone: "+00:00".to_string(),
                hour: 0,
                minute: 0,
            },
            models: vec![ModelRateLimitRule {
                model: "*".to_string(),
                rpm: Some(1),
                rpd: Some(3),
                tpm: Some(100),
                tpm_mode: Some(TpmMode::InputAndOutput),
                tokenizer_encoding: None,
                tokenizer_model: None,
            }],
            enabled_key_slots_cache: Vec::new(),
            total_weight_slots_cache: 0,
        }
        .normalized()
        .unwrap();
        let now = Utc
            .with_ymd_and_hms(2026, 5, 5, 12, 0, 0)
            .unwrap()
            .timestamp_millis();
        let input = KeySelectionInput {
            provider_id: "provider-7",
            provider_name: "Provider 7",
            actual_model: "gpt-4o",
            request_input_tokens: 40,
            request_output_reservation: 50,
        };

        let selected = selector.select_key_at(&config, &input, now).unwrap();
        selector
            .settle_at(
                &selected.lease,
                SettlementUsage {
                    input_tokens: 40,
                    output_tokens: 50,
                },
                now,
            )
            .unwrap();

        let snapshot = selector
            .runtime_snapshot_at("provider-7", &config, now)
            .unwrap();
        assert_eq!(snapshot.models.len(), 2);

        let wildcard = snapshot
            .models
            .iter()
            .find(|model| model.model == "*")
            .unwrap();
        assert_eq!(wildcard.matched_rule_model.as_deref(), Some("*"));

        let actual = snapshot
            .models
            .iter()
            .find(|model| model.model == "gpt-4o")
            .unwrap();
        assert_eq!(actual.matched_rule_model.as_deref(), Some("*"));
        assert_eq!(actual.available_key_count, 0);

        let key_a = actual
            .keys
            .iter()
            .find(|key| key.key_id == "key-a")
            .unwrap();
        assert!(!key_a.available);
        assert_eq!(key_a.blocked_reason.as_deref(), Some("rpm quota exceeded"));
        assert_eq!(key_a.rpm.used, 1);
        assert_eq!(key_a.rpm.limit, Some(1));
        assert_eq!(key_a.tpm.used, 90);
        assert_eq!(key_a.tpm.limit, Some(100));

        let key_b = actual
            .keys
            .iter()
            .find(|key| key.key_id == "key-b")
            .unwrap();
        assert!(!key_b.available);
        assert_eq!(key_b.blocked_reason.as_deref(), Some("key disabled"));
    }
}
