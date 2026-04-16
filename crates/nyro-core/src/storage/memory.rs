use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::db::models::{
    CreateProvider, CreateRoute, LogPage, LogQuery, ModelStats, Provider, ProviderStats, Route,
    StatsHourly, StatsOverview, UpdateProvider, UpdateRoute,
};
use crate::logging::LogEntry;

use super::traits::{
    ApiKeyStore, AuthAccessStore, LogStore, ProviderStore, ProviderTestResult, RouteSnapshotStore,
    RouteStore, RouteTargetStore, SettingsStore, Storage, StorageBackend, StorageBootstrap,
    StorageHealth,
};

use std::sync::Arc;

#[derive(Clone)]
pub struct MemoryStorage {
    providers: Arc<RwLock<Vec<Provider>>>,
    routes: Arc<RwLock<Vec<Route>>>,
    settings: Arc<RwLock<Vec<(String, String)>>>,
}

impl MemoryStorage {
    pub fn new(
        providers: Vec<Provider>,
        routes: Vec<Route>,
        settings: Vec<(String, String)>,
    ) -> Self {
        Self {
            providers: Arc::new(RwLock::new(providers)),
            routes: Arc::new(RwLock::new(routes)),
            settings: Arc::new(RwLock::new(settings)),
        }
    }
}

impl Storage for MemoryStorage {
    fn providers(&self) -> &dyn ProviderStore {
        self
    }
    fn routes(&self) -> &dyn RouteStore {
        self
    }
    fn snapshots(&self) -> &dyn RouteSnapshotStore {
        self
    }
    fn route_targets(&self) -> Option<&dyn RouteTargetStore> {
        None
    }
    fn settings(&self) -> &dyn SettingsStore {
        self
    }
    fn api_keys(&self) -> Option<&dyn ApiKeyStore> {
        None
    }
    fn auth(&self) -> Option<&dyn AuthAccessStore> {
        None
    }
    fn logs(&self) -> &dyn LogStore {
        self
    }
    fn bootstrap(&self) -> &dyn StorageBootstrap {
        self
    }
}

#[async_trait]
impl ProviderStore for MemoryStorage {
    async fn list(&self) -> anyhow::Result<Vec<Provider>> {
        Ok(self.providers.read().await.clone())
    }

    async fn get(&self, id: &str) -> anyhow::Result<Option<Provider>> {
        Ok(self
            .providers
            .read()
            .await
            .iter()
            .find(|p| p.id == id)
            .cloned())
    }

    async fn create(&self, _input: CreateProvider) -> anyhow::Result<Provider> {
        anyhow::bail!("create not supported in standalone (YAML) mode")
    }

    async fn update(&self, _id: &str, _input: UpdateProvider) -> anyhow::Result<Provider> {
        anyhow::bail!("update not supported in standalone (YAML) mode")
    }

    async fn delete(&self, _id: &str) -> anyhow::Result<()> {
        anyhow::bail!("delete not supported in standalone (YAML) mode")
    }

    async fn exists_by_name(&self, name: &str, exclude_id: Option<&str>) -> anyhow::Result<bool> {
        let providers = self.providers.read().await;
        Ok(providers
            .iter()
            .any(|p| p.name == name && exclude_id.map_or(true, |eid| p.id != eid)))
    }

    async fn record_test_result(
        &self,
        _provider_id: &str,
        _result: ProviderTestResult,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl RouteStore for MemoryStorage {
    async fn list(&self) -> anyhow::Result<Vec<Route>> {
        Ok(self.routes.read().await.clone())
    }

    async fn get(&self, id: &str) -> anyhow::Result<Option<Route>> {
        Ok(self
            .routes
            .read()
            .await
            .iter()
            .find(|r| r.id == id)
            .cloned())
    }

    async fn create(&self, _input: CreateRoute) -> anyhow::Result<Route> {
        anyhow::bail!("create not supported in standalone (YAML) mode")
    }

    async fn update(&self, _id: &str, _input: UpdateRoute) -> anyhow::Result<Route> {
        anyhow::bail!("update not supported in standalone (YAML) mode")
    }

    async fn delete(&self, _id: &str) -> anyhow::Result<()> {
        anyhow::bail!("delete not supported in standalone (YAML) mode")
    }

    async fn exists_by_name(&self, name: &str, exclude_id: Option<&str>) -> anyhow::Result<bool> {
        let routes = self.routes.read().await;
        Ok(routes
            .iter()
            .any(|r| r.name == name && exclude_id.map_or(true, |eid| r.id != eid)))
    }

    async fn exists_by_virtual_model(
        &self,
        virtual_model: &str,
        exclude_id: Option<&str>,
    ) -> anyhow::Result<bool> {
        let routes = self.routes.read().await;
        Ok(routes.iter().any(|r| {
            r.virtual_model == virtual_model && exclude_id.map_or(true, |eid| r.id != eid)
        }))
    }
}

#[async_trait]
impl RouteSnapshotStore for MemoryStorage {
    async fn load_active_snapshot(&self) -> anyhow::Result<Vec<Route>> {
        let routes = self.routes.read().await;
        Ok(routes.iter().filter(|r| r.is_active).cloned().collect())
    }
}

#[async_trait]
impl SettingsStore for MemoryStorage {
    async fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        let settings = self.settings.read().await;
        Ok(settings
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone()))
    }

    async fn set(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let mut settings = self.settings.write().await;
        if let Some(entry) = settings.iter_mut().find(|(k, _)| k == key) {
            entry.1 = value.to_string();
        } else {
            settings.push((key.to_string(), value.to_string()));
        }
        Ok(())
    }

    async fn list_all(&self) -> anyhow::Result<Vec<(String, String)>> {
        Ok(self.settings.read().await.clone())
    }
}

#[async_trait]
impl LogStore for MemoryStorage {
    async fn append_batch(&self, _entries: Vec<LogEntry>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn query(&self, _query: LogQuery) -> anyhow::Result<LogPage> {
        Ok(LogPage {
            items: vec![],
            total: 0,
        })
    }

    async fn cleanup_before(&self, _cutoff: &str) -> anyhow::Result<u64> {
        Ok(0)
    }

    async fn stats_overview(&self, _hours: Option<i64>) -> anyhow::Result<StatsOverview> {
        Ok(StatsOverview::default())
    }

    async fn stats_hourly(&self, _hours: i64) -> anyhow::Result<Vec<StatsHourly>> {
        Ok(vec![])
    }

    async fn stats_by_model(&self, _hours: Option<i64>) -> anyhow::Result<Vec<ModelStats>> {
        Ok(vec![])
    }

    async fn stats_by_provider(&self, _hours: Option<i64>) -> anyhow::Result<Vec<ProviderStats>> {
        Ok(vec![])
    }
}

#[async_trait]
impl StorageBootstrap for MemoryStorage {
    async fn init(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn migrate(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn health(&self) -> anyhow::Result<StorageHealth> {
        Ok(StorageHealth {
            backend: StorageBackend::Sqlite,
            can_connect: true,
            schema_compatible: true,
            writable: false,
        })
    }
}
