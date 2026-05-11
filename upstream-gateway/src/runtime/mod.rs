mod types;

use std::sync::Arc;

use async_trait::async_trait;

use crate::config::GatewayProviderRateLimitConfig;
use crate::errors::RateLimitError;
use crate::provider::GatewayProvider;
use crate::selector::{DefaultKeySelector, KeySelector, RuntimeState};

pub use types::{
    KeyModelRuntimeSnapshot, KeySelectionInput, ProviderModelRuntimeSnapshot, RateLimitLease,
    RateLimitMetricSnapshot, SelectedUpstreamKey, SettlementUsage,
    UpstreamRateLimitRuntimeSnapshot, UpstreamRateLimitSummary,
};

pub type SharedRateLimiter = Arc<dyn UpstreamRateLimiter>;

#[async_trait]
pub trait UpstreamRateLimiter: Send + Sync {
    async fn acquire(
        &self,
        provider: &GatewayProvider,
        config: &GatewayProviderRateLimitConfig,
        actual_model: &str,
        request_input_tokens: u32,
        request_output_reservation: u32,
    ) -> Result<SelectedUpstreamKey, RateLimitError>;

    async fn settle(
        &self,
        lease: &RateLimitLease,
        usage: SettlementUsage,
    ) -> Result<(), RateLimitError>;

    async fn rollback(&self, lease: &RateLimitLease) -> Result<(), RateLimitError>;

    fn summarize(&self, config: &GatewayProviderRateLimitConfig) -> UpstreamRateLimitSummary;

    fn runtime_snapshot(
        &self,
        provider: &GatewayProvider,
        config: &GatewayProviderRateLimitConfig,
    ) -> Result<UpstreamRateLimitRuntimeSnapshot, RateLimitError>;
}

#[derive(Debug)]
pub struct InMemoryUpstreamRateLimiter {
    selector: DefaultKeySelector,
}

impl Default for InMemoryUpstreamRateLimiter {
    fn default() -> Self {
        Self {
            selector: DefaultKeySelector::with_state(Arc::new(RuntimeState::default())),
        }
    }
}

impl InMemoryUpstreamRateLimiter {
    pub fn selector(&self) -> &DefaultKeySelector {
        &self.selector
    }
}

#[async_trait]
impl UpstreamRateLimiter for InMemoryUpstreamRateLimiter {
    async fn acquire(
        &self,
        provider: &GatewayProvider,
        config: &GatewayProviderRateLimitConfig,
        actual_model: &str,
        request_input_tokens: u32,
        request_output_reservation: u32,
    ) -> Result<SelectedUpstreamKey, RateLimitError> {
        let input = KeySelectionInput {
            provider_id: &provider.id,
            provider_name: &provider.name,
            actual_model,
            request_input_tokens,
            request_output_reservation,
        };
        self.selector.select_key(config, &input)
    }

    async fn settle(
        &self,
        lease: &RateLimitLease,
        usage: SettlementUsage,
    ) -> Result<(), RateLimitError> {
        self.selector
            .settle_at(lease, usage, chrono::Utc::now().timestamp_millis())
    }

    async fn rollback(&self, lease: &RateLimitLease) -> Result<(), RateLimitError> {
        self.selector
            .rollback_at(lease, chrono::Utc::now().timestamp_millis())
    }

    fn summarize(&self, config: &GatewayProviderRateLimitConfig) -> UpstreamRateLimitSummary {
        config.summary()
    }

    fn runtime_snapshot(
        &self,
        provider: &GatewayProvider,
        config: &GatewayProviderRateLimitConfig,
    ) -> Result<UpstreamRateLimitRuntimeSnapshot, RateLimitError> {
        self.selector.runtime_snapshot(&provider.id, config)
    }
}
