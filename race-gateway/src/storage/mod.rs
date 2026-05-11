mod bootstrap;
mod sqlite;

use std::sync::Arc;

use async_trait::async_trait;

use crate::domain::{
    BootstrapData, RaceGroup, RaceGroupSummary, RaceKeyPool, RaceKeyPoolSummary,
    RaceModelDescriptor, RaceModelSummary, RaceSettings,
};

pub use bootstrap::load_bootstrap_data;
pub use sqlite::SqliteRaceConfigStore;

pub type SharedRaceConfigStore = Arc<dyn RaceConfigStore>;

#[async_trait]
pub trait RaceConfigStore: Send + Sync {
    async fn list_models(&self) -> anyhow::Result<Vec<RaceModelSummary>>;
    async fn get_model(&self, model_id: &str) -> anyhow::Result<Option<RaceModelDescriptor>>;
    async fn put_model(
        &self,
        previous_model_id: Option<&str>,
        model: RaceModelDescriptor,
    ) -> anyhow::Result<RaceModelDescriptor>;
    async fn delete_model(&self, model_id: &str) -> anyhow::Result<bool>;
    async fn load_models_full(&self) -> anyhow::Result<Vec<RaceModelDescriptor>> {
        let mut models = Vec::new();
        for summary in self.list_models().await? {
            if let Some(model) = self.get_model(&summary.id).await? {
                models.push(model);
            }
        }
        Ok(models)
    }

    async fn list_groups(&self) -> anyhow::Result<Vec<RaceGroupSummary>>;
    async fn get_group(&self, group_id: &str) -> anyhow::Result<Option<RaceGroup>>;
    async fn put_group(
        &self,
        previous_group_id: Option<&str>,
        group: RaceGroup,
    ) -> anyhow::Result<RaceGroup>;
    async fn delete_group(&self, group_id: &str) -> anyhow::Result<bool>;
    async fn load_groups_full(&self) -> anyhow::Result<Vec<RaceGroup>> {
        let mut groups = Vec::new();
        for summary in self.list_groups().await? {
            if let Some(group) = self.get_group(&summary.id).await? {
                groups.push(group);
            }
        }
        Ok(groups)
    }

    async fn list_key_pools(&self) -> anyhow::Result<Vec<RaceKeyPoolSummary>>;
    async fn get_key_pool(&self, key_pool_id: &str) -> anyhow::Result<Option<RaceKeyPool>>;
    async fn put_key_pool(
        &self,
        previous_key_pool_id: Option<&str>,
        pool: RaceKeyPool,
    ) -> anyhow::Result<RaceKeyPool>;
    async fn delete_key_pool(&self, key_pool_id: &str) -> anyhow::Result<bool>;
    async fn load_key_pools_full(&self) -> anyhow::Result<Vec<RaceKeyPool>> {
        let mut key_pools = Vec::new();
        for summary in self.list_key_pools().await? {
            if let Some(key_pool) = self.get_key_pool(&summary.id).await? {
                key_pools.push(key_pool);
            }
        }
        Ok(key_pools)
    }

    async fn get_settings(&self) -> anyhow::Result<RaceSettings>;
    async fn put_settings(&self, settings: RaceSettings) -> anyhow::Result<RaceSettings>;
}

pub fn normalize_bootstrap_data(mut data: BootstrapData) -> BootstrapData {
    for model in &mut data.models {
        model.id = model.id.trim().to_string();
        model.display_name = model.display_name.trim().to_string();
        model.upstream_model = model.upstream_model.trim().to_string();
        for endpoint in &mut model.endpoints {
            endpoint.base_url = endpoint.base_url.trim().trim_end_matches('/').to_string();
            endpoint.key_pool_id = endpoint.key_pool_id.trim().to_string();
        }
    }

    for group in &mut data.groups {
        group.id = group.id.trim().to_string();
        group.display_name = group.display_name.trim().to_string();
        for candidate in &mut group.candidates {
            candidate.id = candidate.id.trim().to_string();
            candidate.group_id = candidate.group_id.trim().to_string();
            candidate.name = candidate.name.trim().to_string();
            candidate.model_id = candidate
                .model_id
                .take()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            candidate.upstream_model = candidate.upstream_model.trim().to_string();
            for endpoint in &mut candidate.inline_endpoint_overrides {
                endpoint.base_url = endpoint.base_url.trim().trim_end_matches('/').to_string();
                endpoint.key_pool_id = endpoint.key_pool_id.trim().to_string();
            }
        }
    }

    for pool in &mut data.key_pools {
        pool.id = pool.id.trim().to_string();
        pool.display_name = pool.display_name.trim().to_string();
        for key in &mut pool.keys {
            key.id = key.id.trim().to_string();
            key.key_pool_id = key.key_pool_id.trim().to_string();
            key.secret = key.secret.trim().to_string();
        }
    }

    data
}
