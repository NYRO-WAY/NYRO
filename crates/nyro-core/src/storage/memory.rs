use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::db::models::{
    CreateProvider, CreateRoute, LogPage, LogQuery, ModelStats, OAuthCredential, Provider,
    ProviderStats, RequestLog, Route, StatsHourly, StatsOverview, UpdateProvider, UpdateRoute,
    UpsertOAuthCredential,
};
use crate::logging::LogEntry;

use super::traits::{
    ApiKeyStore, AuthAccessStore, LogStore, OAuthCredentialStore, ProviderStore,
    ProviderTestResult, RouteSnapshotStore, RouteStore, RouteTargetStore, SettingsStore, Storage,
    StorageBackend, StorageBootstrap, StorageHealth,
};

use std::sync::Arc;

#[derive(Clone)]
pub struct MemoryStorage {
    providers: Arc<RwLock<Vec<Provider>>>,
    routes: Arc<RwLock<Vec<Route>>>,
    settings: Arc<RwLock<Vec<(String, String)>>>,
    oauth_credentials: Arc<MemoryOAuthCredentialStore>,
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
            oauth_credentials: Arc::new(MemoryOAuthCredentialStore {
                credentials: RwLock::new(std::collections::HashMap::new()),
            }),
        }
    }
}

pub struct MemoryOAuthCredentialStore {
    credentials: RwLock<std::collections::HashMap<String, OAuthCredential>>,
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
    fn oauth_credentials(&self) -> &dyn OAuthCredentialStore {
        self.oauth_credentials.as_ref()
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
        Ok(self.providers.read().await.iter().find(|p| p.id == id).cloned())
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
        Ok(providers.iter().any(|p| {
            p.name == name && exclude_id.map_or(true, |eid| p.id != eid)
        }))
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
        Ok(self.routes.read().await.iter().find(|r| r.id == id).cloned())
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
        Ok(routes.iter().any(|r| {
            r.name == name && exclude_id.map_or(true, |eid| r.id != eid)
        }))
    }

    async fn exists_by_virtual_model(
        &self,
        virtual_model: &str,
        exclude_id: Option<&str>,
    ) -> anyhow::Result<bool> {
        let routes = self.routes.read().await;
        Ok(routes.iter().any(|r| {
            r.virtual_model == virtual_model
                && exclude_id.map_or(true, |eid| r.id != eid)
        }))
    }
}

#[async_trait]
impl RouteSnapshotStore for MemoryStorage {
    async fn load_active_snapshot(&self) -> anyhow::Result<Vec<Route>> {
        let routes = self.routes.read().await;
        Ok(routes.iter().filter(|r| r.is_enabled).cloned().collect())
    }
}

#[async_trait]
impl SettingsStore for MemoryStorage {
    async fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        let settings = self.settings.read().await;
        Ok(settings.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()))
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

    async fn find_by_id(&self, _id: &str) -> anyhow::Result<Option<RequestLog>> {
        Ok(None)
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

fn now_rfc3339() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

#[async_trait]
impl OAuthCredentialStore for MemoryOAuthCredentialStore {
    async fn get(&self, provider_id: &str) -> anyhow::Result<Option<OAuthCredential>> {
        Ok(self.credentials.read().await.get(provider_id).cloned())
    }

    async fn upsert(
        &self,
        provider_id: &str,
        input: UpsertOAuthCredential,
    ) -> anyhow::Result<OAuthCredential> {
        let now = now_rfc3339();
        let mut map = self.credentials.write().await;
        let version = map.get(provider_id).map(|c| c.status_version + 1).unwrap_or(0);
        let cred = OAuthCredential {
            provider_id: provider_id.to_string(),
            driver_key: input.driver_key,
            scheme: input.scheme,
            access_token: input.access_token,
            refresh_token: input.refresh_token,
            expires_at: input.expires_at,
            resource_url: input.resource_url,
            subject_id: input.subject_id,
            scopes: input.scopes.unwrap_or_else(|| "[]".to_string()),
            meta: input.meta.unwrap_or_else(|| "{}".to_string()),
            status: "connected".to_string(),
            status_version: version,
            last_error: None,
            last_refresh_at: map.get(provider_id).and_then(|c| c.last_refresh_at.clone()),
            created_at: map.get(provider_id).map(|c| c.created_at.clone()).unwrap_or_else(|| now.clone()),
            updated_at: now,
        };
        map.insert(provider_id.to_string(), cred.clone());
        Ok(cred)
    }

    async fn delete(&self, provider_id: &str) -> anyhow::Result<()> {
        self.credentials.write().await.remove(provider_id);
        Ok(())
    }

    async fn try_begin_refresh(
        &self,
        provider_id: &str,
        expected_version: i32,
    ) -> anyhow::Result<Option<OAuthCredential>> {
        let mut map = self.credentials.write().await;
        let Some(cred) = map.get_mut(provider_id) else {
            return Ok(None);
        };
        if cred.status != "connected" || cred.status_version != expected_version {
            return Ok(None);
        }
        cred.status = "refreshing".to_string();
        cred.status_version += 1;
        cred.updated_at = now_rfc3339();
        Ok(Some(cred.clone()))
    }

    async fn complete_refresh(
        &self,
        provider_id: &str,
        input: UpsertOAuthCredential,
    ) -> anyhow::Result<OAuthCredential> {
        let mut map = self.credentials.write().await;
        let cred = map.get_mut(provider_id).context("credential not found")?;
        let now = now_rfc3339();
        cred.driver_key = input.driver_key;
        cred.scheme = input.scheme;
        cred.access_token = input.access_token;
        cred.refresh_token = input.refresh_token;
        cred.expires_at = input.expires_at;
        cred.resource_url = input.resource_url;
        cred.subject_id = input.subject_id;
        if let Some(scopes) = input.scopes {
            cred.scopes = scopes;
        }
        if let Some(meta) = input.meta {
            cred.meta = meta;
        }
        cred.status = "connected".to_string();
        cred.status_version += 1;
        cred.last_error = None;
        cred.last_refresh_at = Some(now.clone());
        cred.updated_at = now;
        Ok(cred.clone())
    }

    async fn fail_refresh(
        &self,
        provider_id: &str,
        error_message: &str,
    ) -> anyhow::Result<()> {
        let mut map = self.credentials.write().await;
        if let Some(cred) = map.get_mut(provider_id) {
            cred.status = "error".to_string();
            cred.last_error = Some(error_message.to_string());
            cred.status_version += 1;
            cred.updated_at = now_rfc3339();
        }
        Ok(())
    }

    async fn list_expiring(&self, _before: Duration) -> anyhow::Result<Vec<OAuthCredential>> {
        // Memory store: return all connected credentials (no real time comparison)
        let map = self.credentials.read().await;
        Ok(map.values().filter(|c| c.status == "connected").cloned().collect())
    }

    async fn recover_stale_refreshing(&self, _timeout: Duration) -> anyhow::Result<u64> {
        let mut map = self.credentials.write().await;
        let mut count = 0u64;
        for cred in map.values_mut() {
            if cred.status == "refreshing" {
                cred.status = "error".to_string();
                cred.last_error = Some("refresh timeout".to_string());
                cred.status_version += 1;
                cred.updated_at = now_rfc3339();
                count += 1;
            }
        }
        Ok(count)
    }
}
