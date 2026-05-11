mod bootstrap;
mod sqlite;

use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use dashmap::DashMap;

use crate::provider::{GatewayProviderBundle, GatewayProviderSummary};

pub use bootstrap::load_provider_bundles;
pub use sqlite::SqliteGatewayConfigStore;

pub type SharedGatewayConfigStore = Arc<dyn GatewayConfigStore>;
pub type SharedProviderBundle = Arc<GatewayProviderBundle>;

#[async_trait]
pub trait GatewayConfigStore: Send + Sync {
    async fn get_provider_bundle(
        &self,
        provider_id: &str,
    ) -> anyhow::Result<Option<GatewayProviderBundle>>;

    async fn get_provider_bundle_shared(
        &self,
        provider_id: &str,
    ) -> anyhow::Result<Option<SharedProviderBundle>> {
        Ok(self.get_provider_bundle(provider_id).await?.map(Arc::new))
    }

    async fn list_provider_bundles(&self) -> anyhow::Result<Vec<GatewayProviderBundle>> {
        let summaries = self.list_provider_summaries().await?;
        let mut bundles = Vec::with_capacity(summaries.len());
        for summary in summaries {
            if let Some(bundle) = self.get_provider_bundle(&summary.id).await? {
                bundles.push(bundle);
            }
        }
        Ok(bundles)
    }

    async fn list_provider_bundles_shared(&self) -> anyhow::Result<Vec<SharedProviderBundle>> {
        Ok(self
            .list_provider_bundles()
            .await?
            .into_iter()
            .map(Arc::new)
            .collect())
    }

    async fn list_provider_summaries(&self) -> anyhow::Result<Vec<GatewayProviderSummary>>;

    async fn put_provider_bundle(
        &self,
        bundle: GatewayProviderBundle,
    ) -> anyhow::Result<GatewayProviderBundle>;

    async fn delete_provider(&self, provider_id: &str) -> anyhow::Result<bool>;
}

#[derive(Debug, Default)]
pub struct InMemoryGatewayConfigStore {
    providers: DashMap<String, SharedProviderBundle>,
}

impl InMemoryGatewayConfigStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, bundle: GatewayProviderBundle) -> anyhow::Result<()> {
        let provider_id = bundle.provider.id.trim().to_string();
        if provider_id.is_empty() {
            anyhow::bail!("provider id cannot be empty");
        }
        let normalized = normalize_bundle(bundle)?;
        self.providers.insert(provider_id, Arc::new(normalized));
        Ok(())
    }
}

#[async_trait]
impl GatewayConfigStore for InMemoryGatewayConfigStore {
    async fn get_provider_bundle(
        &self,
        provider_id: &str,
    ) -> anyhow::Result<Option<GatewayProviderBundle>> {
        Ok(self
            .providers
            .get(provider_id)
            .map(|entry| entry.as_ref().clone()))
    }

    async fn get_provider_bundle_shared(
        &self,
        provider_id: &str,
    ) -> anyhow::Result<Option<SharedProviderBundle>> {
        Ok(self.providers.get(provider_id).map(|entry| entry.clone()))
    }

    async fn list_provider_summaries(&self) -> anyhow::Result<Vec<GatewayProviderSummary>> {
        let mut items = self
            .providers
            .iter()
            .map(|entry| entry.as_ref().summary())
            .collect::<Vec<_>>();
        items.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(items)
    }

    async fn list_provider_bundles(&self) -> anyhow::Result<Vec<GatewayProviderBundle>> {
        let mut items = self
            .providers
            .iter()
            .map(|entry| entry.as_ref().clone())
            .collect::<Vec<_>>();
        items.sort_by(|left, right| left.provider.id.cmp(&right.provider.id));
        Ok(items)
    }

    async fn list_provider_bundles_shared(&self) -> anyhow::Result<Vec<SharedProviderBundle>> {
        let mut items = self
            .providers
            .iter()
            .map(|entry| entry.clone())
            .collect::<Vec<_>>();
        items.sort_by(|left, right| left.provider.id.cmp(&right.provider.id));
        Ok(items)
    }

    async fn put_provider_bundle(
        &self,
        bundle: GatewayProviderBundle,
    ) -> anyhow::Result<GatewayProviderBundle> {
        let normalized = normalize_bundle(bundle)?;
        self.providers
            .insert(normalized.provider.id.clone(), Arc::new(normalized.clone()));
        Ok(normalized)
    }

    async fn delete_provider(&self, provider_id: &str) -> anyhow::Result<bool> {
        Ok(self.providers.remove(provider_id).is_some())
    }
}

pub(crate) fn normalize_bundle(
    mut bundle: GatewayProviderBundle,
) -> anyhow::Result<GatewayProviderBundle> {
    bundle.provider.id = bundle.provider.id.trim().to_string();
    bundle.provider.name = bundle.provider.name.trim().to_string();
    bundle.provider.base_url = bundle
        .provider
        .base_url
        .trim()
        .trim_end_matches('/')
        .to_string();

    if bundle.provider.id.is_empty() {
        anyhow::bail!("provider id cannot be empty");
    }
    if bundle.provider.name.is_empty() {
        anyhow::bail!("provider name cannot be empty");
    }
    if bundle.provider.base_url.is_empty() {
        anyhow::bail!("provider base_url cannot be empty");
    }

    for key in &mut bundle.keys {
        key.id = key.id.trim().to_string();
        key.provider_id = key.provider_id.trim().to_string();
        key.api_key = key.api_key.trim().to_string();
        key.display_name = key
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        if key.provider_id.is_empty() {
            key.provider_id = bundle.provider.id.clone();
        }
        if key.provider_id != bundle.provider.id {
            anyhow::bail!(
                "key '{}' belongs to provider '{}' instead of '{}'",
                key.id,
                key.provider_id,
                bundle.provider.id
            );
        }
    }

    for rule in &mut bundle.model_rules {
        rule.provider_id = rule.provider_id.trim().to_string();
        if rule.provider_id.is_empty() {
            rule.provider_id = bundle.provider.id.clone();
        }
        if rule.provider_id != bundle.provider.id {
            anyhow::bail!(
                "model rule '{}' belongs to provider '{}' instead of '{}'",
                rule.rule.model,
                rule.provider_id,
                bundle.provider.id
            );
        }
    }

    bundle.refresh_rate_limit_config_cache().with_context(|| {
        format!(
            "provider '{}' rate_limit config is invalid",
            bundle.provider.id
        )
    })?;

    Ok(bundle)
}
