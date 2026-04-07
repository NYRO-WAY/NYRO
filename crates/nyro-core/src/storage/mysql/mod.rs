use std::sync::Arc;

use anyhow::{bail, Context};
use async_trait::async_trait;
use sqlx::{MySql, Pool};
use std::time::Duration;

use crate::db::models::{
    ApiKey, ApiKeyWithBindings, CreateApiKey, CreateProvider, CreateRoute, CreateRouteTarget,
    LogPage, LogQuery, ModelStats, Provider, ProviderStats, RequestLog, Route, RouteTarget,
    StatsHourly, StatsOverview, UpdateApiKey, UpdateProvider, UpdateRoute,
};
use crate::logging::LogEntry;
use crate::storage::sql::config::SqlBackendConfig;
use crate::storage::sql::dialect::SqlDialect;
use crate::storage::sql::pool::RelationalPool;
use crate::storage::traits::{
    ApiKeyAccessRecord, ApiKeyStore, AuthAccessStore, CacheStore, DynStorage, LogStore,
    ProviderStore, ProviderTestResult, RouteSnapshotStore, RouteStore, RouteTargetStore,
    SettingsStore, Storage, StorageBackend, StorageBootstrap, StorageHealth, UsageWindow,
};

#[derive(Clone)]
pub struct MySqlAdapter {
    pool: Pool<MySql>,
    config: SqlBackendConfig,
}

#[derive(Debug, Clone)]
pub struct MySqlHealth {
    pub can_connect: bool,
    pub schema_compatible: bool,
}

impl MySqlAdapter {
    pub async fn connect(config: SqlBackendConfig) -> anyhow::Result<Self> {
        let pool = RelationalPool::connect(crate::storage::sql::config::SqlBackendKind::MySql, &config)
            .await
            .context("connect mysql adapter")?;
        let pool = pool
            .as_mysql()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("relational pool kind mismatch: expected mysql"))?;
        Ok(Self { pool, config })
    }

    pub fn dialect(&self) -> SqlDialect {
        SqlDialect::MySql
    }

    pub fn config(&self) -> &SqlBackendConfig {
        &self.config
    }

    pub fn pool(&self) -> &Pool<MySql> {
        &self.pool
    }

    pub async fn ping(&self) -> anyhow::Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn health(&self) -> MySqlHealth {
        let can_connect = self.ping().await.is_ok();
        MySqlHealth {
            can_connect,
            schema_compatible: can_connect,
        }
    }
}

#[derive(Clone)]
pub struct MySqlStorage {
    provider_store: Arc<MySqlProviderStore>,
    route_store: Arc<MySqlRouteStore>,
    route_target_store: Arc<MySqlRouteTargetStore>,
    settings_store: Arc<MySqlSettingsStore>,
    api_key_store: Arc<MySqlApiKeyStore>,
    auth_store: Arc<MySqlAuthAccessStore>,
    log_store: Arc<MySqlLogStore>,
    bootstrap: Arc<MySqlBootstrap>,
}

impl MySqlStorage {
    pub async fn connect(config: SqlBackendConfig, _fallback: DynStorage) -> anyhow::Result<Self> {
        let adapter = MySqlAdapter::connect(config).await?;
        let provider_store = Arc::new(MySqlProviderStore {
            pool: adapter.pool().clone(),
        });
        let route_store = Arc::new(MySqlRouteStore {
            pool: adapter.pool().clone(),
        });
        let route_target_store = Arc::new(MySqlRouteTargetStore {
            pool: adapter.pool().clone(),
        });
        let settings_store = Arc::new(MySqlSettingsStore {
            pool: adapter.pool().clone(),
        });
        let api_key_store = Arc::new(MySqlApiKeyStore {
            pool: adapter.pool().clone(),
        });
        let auth_store = Arc::new(MySqlAuthAccessStore {
            pool: adapter.pool().clone(),
        });
        let log_store = Arc::new(MySqlLogStore {
            pool: adapter.pool().clone(),
        });
        let bootstrap = Arc::new(MySqlBootstrap { adapter });
        Ok(Self {
            provider_store,
            route_store,
            route_target_store,
            settings_store,
            api_key_store,
            auth_store,
            log_store,
            bootstrap,
        })
    }
}

impl Storage for MySqlStorage {
    fn providers(&self) -> &dyn ProviderStore {
        self.provider_store.as_ref()
    }

    fn routes(&self) -> &dyn RouteStore {
        self.route_store.as_ref()
    }

    fn snapshots(&self) -> &dyn RouteSnapshotStore {
        self.route_store.as_ref()
    }

    fn settings(&self) -> &dyn SettingsStore {
        self.settings_store.as_ref()
    }

    fn route_targets(&self) -> Option<&dyn RouteTargetStore> {
        Some(self.route_target_store.as_ref())
    }

    fn api_keys(&self) -> Option<&dyn ApiKeyStore> {
        Some(self.api_key_store.as_ref())
    }

    fn auth(&self) -> Option<&dyn AuthAccessStore> {
        Some(self.auth_store.as_ref())
    }

    fn logs(&self) -> &dyn LogStore {
        self.log_store.as_ref()
    }

    fn cache(&self) -> Option<&dyn CacheStore> {
        Some(self.log_store.as_ref())
    }

    fn bootstrap(&self) -> &dyn StorageBootstrap {
        self.bootstrap.as_ref()
    }
}

#[derive(Clone)]
struct MySqlProviderStore {
    pool: Pool<MySql>,
}

#[async_trait]
impl ProviderStore for MySqlProviderStore {
    async fn list(&self) -> anyhow::Result<Vec<Provider>> {
        Ok(sqlx::query_as::<_, Provider>(&provider_select(None))
            .fetch_all(&self.pool)
            .await?)
    }

    async fn get(&self, id: &str) -> anyhow::Result<Option<Provider>> {
        Ok(sqlx::query_as::<_, Provider>(&provider_select(Some("WHERE id = ?")))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }

    async fn create(&self, input: CreateProvider) -> anyhow::Result<Provider> {
        let id = uuid::Uuid::new_v4().to_string();
        let vendor = normalize_provider_vendor(input.vendor.as_deref());
        let models_source = input.effective_models_source().map(ToString::to_string);
        let default_protocol = input
            .default_protocol
            .as_deref()
            .unwrap_or(input.protocol.as_str());
        let protocol_endpoints = input.protocol_endpoints.as_deref().unwrap_or("{}");
        sqlx::query(
            "INSERT INTO providers (id, name, vendor, protocol, base_url, default_protocol, protocol_endpoints, preset_key, channel, models_source, capabilities_source, static_models, api_key, use_proxy, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, UTC_TIMESTAMP(), UTC_TIMESTAMP())",
        )
        .bind(&id)
        .bind(input.name.trim())
        .bind(vendor)
        .bind(input.protocol.trim())
        .bind(input.base_url.trim())
        .bind(default_protocol)
        .bind(protocol_endpoints)
        .bind(input.preset_key)
        .bind(input.channel)
        .bind(models_source)
        .bind(input.capabilities_source)
        .bind(input.static_models)
        .bind(input.api_key)
        .bind(input.use_proxy)
        .execute(&self.pool)
        .await?;
        self.get(&id).await?.context("provider missing after create")
    }

    async fn update(&self, id: &str, input: UpdateProvider) -> anyhow::Result<Provider> {
        let current = self.get(id).await?.context("provider not found for update")?;
        let models_source_input = input.effective_models_source().map(ToString::to_string);
        let name = input.name.unwrap_or(current.name);
        let vendor = if input.vendor.is_some() {
            normalize_provider_vendor(input.vendor.as_deref())
        } else {
            normalize_provider_vendor(current.vendor.as_deref())
        };
        let models_source = models_source_input
            .or_else(|| current.models_source.clone());
        let protocol = input.protocol.unwrap_or(current.protocol.clone());
        let base_url = input.base_url.unwrap_or(current.base_url);
        let default_protocol = input
            .default_protocol
            .unwrap_or(current.default_protocol);
        let protocol_endpoints = input
            .protocol_endpoints
            .unwrap_or(current.protocol_endpoints);
        let preset_key = input.preset_key.or(current.preset_key);
        let channel = input.channel.or(current.channel);
        let capabilities_source = input.capabilities_source.or(current.capabilities_source);
        let static_models = input.static_models.or(current.static_models);
        let api_key = input.api_key.unwrap_or(current.api_key);
        let use_proxy = input.use_proxy.unwrap_or(current.use_proxy);
        let is_active = input.is_active.unwrap_or(current.is_active);

        sqlx::query(
            "UPDATE providers SET name=?, vendor=?, protocol=?, base_url=?, default_protocol=?, protocol_endpoints=?, preset_key=?, channel=?, models_source=?, capabilities_source=?, static_models=?, api_key=?, use_proxy=?, is_active=?, updated_at=UTC_TIMESTAMP() WHERE id=?",
        )
        .bind(name.trim())
        .bind(vendor)
        .bind(protocol.trim())
        .bind(base_url.trim())
        .bind(default_protocol)
        .bind(protocol_endpoints)
        .bind(preset_key)
        .bind(channel)
        .bind(models_source)
        .bind(capabilities_source)
        .bind(static_models)
        .bind(api_key)
        .bind(use_proxy)
        .bind(is_active)
        .bind(id)
        .execute(&self.pool)
        .await?;
        self.get(id).await?.context("provider missing after update")
    }

    async fn delete(&self, id: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM providers WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn exists_by_name(&self, name: &str, exclude_id: Option<&str>) -> anyhow::Result<bool> {
        let row = if let Some(exclude_id) = exclude_id {
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM providers WHERE lower(trim(name)) = lower(trim(?)) AND id != ? LIMIT 1",
            )
            .bind(name)
            .bind(exclude_id)
            .fetch_optional(&self.pool)
            .await?
        } else {
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM providers WHERE lower(trim(name)) = lower(trim(?)) LIMIT 1",
            )
            .bind(name)
            .fetch_optional(&self.pool)
            .await?
        };
        Ok(row.is_some())
    }

    async fn record_test_result(&self, provider_id: &str, result: ProviderTestResult) -> anyhow::Result<()> {
        let _ = result.tested_at;
        sqlx::query(
            "UPDATE providers SET last_test_success = ?, last_test_at = UTC_TIMESTAMP() WHERE id = ?",
        )
        .bind(result.success)
        .bind(provider_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[derive(Clone)]
struct MySqlRouteStore {
    pool: Pool<MySql>,
}

#[async_trait]
impl RouteStore for MySqlRouteStore {
    async fn list(&self) -> anyhow::Result<Vec<Route>> {
        Ok(sqlx::query_as::<_, Route>(&route_select(Some("ORDER BY created_at DESC")))
            .fetch_all(&self.pool)
            .await?)
    }

    async fn get(&self, id: &str) -> anyhow::Result<Option<Route>> {
        let sql = format!("{} WHERE id = ?", route_select(None));
        Ok(sqlx::query_as::<_, Route>(&sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }

    async fn create(&self, input: CreateRoute) -> anyhow::Result<Route> {
        let id = uuid::Uuid::new_v4().to_string();
        let virtual_model = input.virtual_model.trim().to_string();
        sqlx::query(
            "INSERT INTO routes (id, name, virtual_model, strategy, target_provider, target_model, access_control, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, UTC_TIMESTAMP())",
        )
        .bind(&id)
        .bind(input.name.trim())
        .bind(&virtual_model)
        .bind(input.strategy.unwrap_or_else(|| "weighted".to_string()))
        .bind(input.target_provider.trim())
        .bind(input.target_model.trim())
        .bind(input.access_control.unwrap_or(false))
        .execute(&self.pool)
        .await?;
        self.get(&id).await?.context("route missing after create")
    }

    async fn update(&self, id: &str, input: UpdateRoute) -> anyhow::Result<Route> {
        let current = self.get(id).await?.context("route not found for update")?;
        let name = input.name.unwrap_or(current.name);
        let virtual_model = input
            .virtual_model
            .unwrap_or(current.virtual_model)
            .trim()
            .to_string();
        let strategy = input.strategy.unwrap_or(current.strategy);
        let target_provider = input.target_provider.unwrap_or(current.target_provider);
        let target_model = input.target_model.unwrap_or(current.target_model);
        let access_control = input.access_control.unwrap_or(current.access_control);
        let is_active = input.is_active.unwrap_or(current.is_active);

        sqlx::query(
            "UPDATE routes SET name=?, virtual_model=?, strategy=?, target_provider=?, target_model=?, access_control=?, is_active=? WHERE id=?",
        )
        .bind(name.trim())
        .bind(&virtual_model)
        .bind(strategy.trim().to_lowercase())
        .bind(target_provider.trim())
        .bind(target_model.trim())
        .bind(access_control)
        .bind(is_active)
        .bind(id)
        .execute(&self.pool)
        .await?;
        self.get(id).await?.context("route missing after update")
    }

    async fn delete(&self, id: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM routes WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn exists_by_name(&self, name: &str, exclude_id: Option<&str>) -> anyhow::Result<bool> {
        let row = if let Some(exclude_id) = exclude_id {
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM routes WHERE lower(trim(name)) = lower(trim(?)) AND id != ? LIMIT 1",
            )
            .bind(name)
            .bind(exclude_id)
            .fetch_optional(&self.pool)
            .await?
        } else {
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM routes WHERE lower(trim(name)) = lower(trim(?)) LIMIT 1",
            )
            .bind(name)
            .fetch_optional(&self.pool)
            .await?
        };
        Ok(row.is_some())
    }

    async fn exists_by_virtual_model(
        &self,
        virtual_model: &str,
        exclude_id: Option<&str>,
    ) -> anyhow::Result<bool> {
        let normalized_model = virtual_model.trim();
        let row = if let Some(exclude_id) = exclude_id {
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM routes WHERE virtual_model = ? AND id != ? LIMIT 1",
            )
            .bind(normalized_model)
            .bind(exclude_id)
            .fetch_optional(&self.pool)
            .await?
        } else {
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM routes WHERE virtual_model = ? LIMIT 1",
            )
            .bind(normalized_model)
            .fetch_optional(&self.pool)
            .await?
        };
        Ok(row.is_some())
    }

}

#[async_trait]
impl RouteSnapshotStore for MySqlRouteStore {
    async fn load_active_snapshot(&self) -> anyhow::Result<Vec<Route>> {
        let sql = format!("{} WHERE is_active = TRUE", route_select(None));
        Ok(sqlx::query_as::<_, Route>(&sql)
            .fetch_all(&self.pool)
            .await?)
    }
}

#[derive(Clone)]
struct MySqlRouteTargetStore {
    pool: Pool<MySql>,
}

#[async_trait]
impl RouteTargetStore for MySqlRouteTargetStore {
    async fn list_targets_by_route(&self, route_id: &str) -> anyhow::Result<Vec<RouteTarget>> {
        Ok(sqlx::query_as::<_, RouteTarget>(
            "SELECT id, route_id, provider_id, model, weight, priority, DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s') AS created_at FROM route_targets WHERE route_id = ? ORDER BY priority ASC, created_at ASC",
        )
        .bind(route_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn set_targets(
        &self,
        route_id: &str,
        targets: &[CreateRouteTarget],
    ) -> anyhow::Result<Vec<RouteTarget>> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM route_targets WHERE route_id = ?")
            .bind(route_id)
            .execute(&mut *tx)
            .await?;

        for target in targets {
            let id = uuid::Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO route_targets (id, route_id, provider_id, model, weight, priority, created_at) VALUES (?, ?, ?, ?, ?, ?, UTC_TIMESTAMP())",
            )
            .bind(id)
            .bind(route_id)
            .bind(target.provider_id.trim())
            .bind(target.model.trim())
            .bind(target.weight.unwrap_or(100).max(0))
            .bind(target.priority.unwrap_or(1).max(1))
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        self.list_targets_by_route(route_id).await
    }

    async fn delete_targets_by_route(&self, route_id: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM route_targets WHERE route_id = ?")
            .bind(route_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[derive(Clone)]
struct MySqlSettingsStore {
    pool: Pool<MySql>,
}

#[async_trait]
impl SettingsStore for MySqlSettingsStore {
    async fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE `key` = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.0))
    }

    async fn set(&self, key: &str, value: &str) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO settings (`key`, value, updated_at) VALUES (?, ?, UTC_TIMESTAMP()) ON DUPLICATE KEY UPDATE value = VALUES(value), updated_at = VALUES(updated_at)",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_all(&self) -> anyhow::Result<Vec<(String, String)>> {
        Ok(sqlx::query_as::<_, (String, String)>("SELECT `key`, value FROM settings")
            .fetch_all(&self.pool)
            .await?)
    }
}

#[derive(Clone)]
struct MySqlApiKeyStore {
    pool: Pool<MySql>,
}

#[async_trait]
impl ApiKeyStore for MySqlApiKeyStore {
    async fn list(&self) -> anyhow::Result<Vec<ApiKeyWithBindings>> {
        let rows = sqlx::query_as::<_, ApiKey>(&api_key_select(None))
            .fetch_all(&self.pool)
            .await?;
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let route_ids = list_api_key_route_ids(&self.pool, &row.id).await?;
            items.push(api_key_with_bindings(row, route_ids));
        }
        Ok(items)
    }

    async fn get(&self, id: &str) -> anyhow::Result<Option<ApiKeyWithBindings>> {
        let row = sqlx::query_as::<_, ApiKey>(&api_key_select(Some("WHERE id = ?")))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let route_ids = list_api_key_route_ids(&self.pool, id).await?;
        Ok(Some(api_key_with_bindings(row, route_ids)))
    }

    async fn create(&self, input: CreateApiKey) -> anyhow::Result<ApiKeyWithBindings> {
        let id = uuid::Uuid::new_v4().to_string();
        let key = format!("sk-{}", uuid::Uuid::new_v4().simple());
        sqlx::query(
            "INSERT INTO api_keys (id, `key`, name, rpm, rpd, tpm, tpd, status, expires_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, 'active', ?, UTC_TIMESTAMP(), UTC_TIMESTAMP())",
        )
        .bind(&id)
        .bind(&key)
        .bind(input.name.trim())
        .bind(input.rpm)
        .bind(input.rpd)
        .bind(input.tpm)
        .bind(input.tpd)
        .bind(normalize_optional_datetime(input.expires_at.as_deref()))
        .execute(&self.pool)
        .await?;
        replace_api_key_routes(&self.pool, &id, &input.route_ids).await?;
        self.get(&id).await?.context("api key missing after create")
    }

    async fn update(&self, id: &str, input: UpdateApiKey) -> anyhow::Result<ApiKeyWithBindings> {
        let current = sqlx::query_as::<_, ApiKey>(&api_key_select(Some("WHERE id = ?")))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .context("api key not found for update")?;
        let name = input.name.unwrap_or(current.name);
        let rpm = input.rpm.or(current.rpm);
        let rpd = input.rpd.or(current.rpd);
        let tpm = input.tpm.or(current.tpm);
        let tpd = input.tpd.or(current.tpd);
        let status = input.status.unwrap_or(current.status);
        let expires_at = input.expires_at.or(current.expires_at);

        sqlx::query(
            "UPDATE api_keys SET name=?, rpm=?, rpd=?, tpm=?, tpd=?, status=?, expires_at=?, updated_at=UTC_TIMESTAMP() WHERE id=?",
        )
        .bind(name.trim())
        .bind(rpm)
        .bind(rpd)
        .bind(tpm)
        .bind(tpd)
        .bind(status)
        .bind(normalize_optional_datetime(expires_at.as_deref()))
        .bind(id)
        .execute(&self.pool)
        .await?;

        if let Some(route_ids) = input.route_ids {
            replace_api_key_routes(&self.pool, id, &route_ids).await?;
        }
        self.get(id).await?.context("api key missing after update")
    }

    async fn delete(&self, id: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM api_keys WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn exists_by_name(&self, name: &str, exclude_id: Option<&str>) -> anyhow::Result<bool> {
        let row = if let Some(exclude_id) = exclude_id {
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM api_keys WHERE lower(trim(name)) = lower(trim(?)) AND id != ? LIMIT 1",
            )
            .bind(name)
            .bind(exclude_id)
            .fetch_optional(&self.pool)
            .await?
        } else {
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM api_keys WHERE lower(trim(name)) = lower(trim(?)) LIMIT 1",
            )
            .bind(name)
            .fetch_optional(&self.pool)
            .await?
        };
        Ok(row.is_some())
    }
}

#[derive(Clone)]
struct MySqlAuthAccessStore {
    pool: Pool<MySql>,
}

#[async_trait]
impl AuthAccessStore for MySqlAuthAccessStore {
    async fn find_api_key(&self, raw_key: &str) -> anyhow::Result<Option<ApiKeyAccessRecord>> {
        let row = sqlx::query_as::<
            _,
            (
                String,
                String,
                Option<String>,
                Option<i32>,
                Option<i32>,
                Option<i32>,
                Option<i32>,
            ),
        >(
            "SELECT id, status, DATE_FORMAT(expires_at, '%Y-%m-%d %H:%i:%s') AS expires_at, rpm, rpd, tpm, tpd FROM api_keys WHERE `key` = ?",
        )
        .bind(raw_key)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(id, status, expires_at, rpm, rpd, tpm, tpd)| ApiKeyAccessRecord {
            id,
            status,
            expires_at,
            rpm,
            rpd,
            tpm,
            tpd,
        }))
    }

    async fn route_binding_exists(&self, api_key_id: &str, route_id: &str) -> anyhow::Result<bool> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM api_key_routes WHERE api_key_id = ? AND route_id = ?",
        )
        .bind(api_key_id)
        .bind(route_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    async fn list_bound_route_ids(&self, api_key_id: &str) -> anyhow::Result<Vec<String>> {
        list_api_key_route_ids(&self.pool, api_key_id).await
    }

    async fn request_count_since(&self, api_key_id: &str, window: UsageWindow) -> anyhow::Result<i64> {
        let sql = format!(
            "SELECT COUNT(*) FROM request_logs WHERE api_key_id = ? AND created_at >= DATE_SUB(UTC_TIMESTAMP(), {})",
            usage_window_interval(window)
        );
        Ok(sqlx::query_scalar::<_, i64>(&sql)
            .bind(api_key_id)
            .fetch_one(&self.pool)
            .await?)
    }

    async fn token_count_since(&self, api_key_id: &str, window: UsageWindow) -> anyhow::Result<i64> {
        let sql = format!(
            "SELECT COALESCE(SUM(input_tokens + output_tokens), 0) FROM request_logs WHERE api_key_id = ? AND created_at >= DATE_SUB(UTC_TIMESTAMP(), {})",
            usage_window_interval(window)
        );
        Ok(sqlx::query_scalar::<_, i64>(&sql)
            .bind(api_key_id)
            .fetch_one(&self.pool)
            .await?)
    }
}

#[derive(Clone)]
struct MySqlLogStore {
    pool: Pool<MySql>,
}

#[async_trait]
impl LogStore for MySqlLogStore {
    async fn append_batch(&self, entries: Vec<LogEntry>) -> anyhow::Result<()> {
        for entry in entries {
            let id = uuid::Uuid::new_v4().to_string();
            sqlx::query(
                r#"INSERT INTO request_logs
                    (id, created_at, api_key_id, ingress_protocol, egress_protocol, request_model, actual_model,
                     provider_name, status_code, duration_ms, input_tokens, output_tokens,
                     is_stream, is_tool_call, error_message, response_preview)
                VALUES (?, UTC_TIMESTAMP(), ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&id)
            .bind(&entry.api_key_id)
            .bind(&entry.ingress_protocol)
            .bind(&entry.egress_protocol)
            .bind(&entry.request_model)
            .bind(&entry.actual_model)
            .bind(&entry.provider_name)
            .bind(entry.status_code)
            .bind(entry.duration_ms)
            .bind(entry.usage.input_tokens as i32)
            .bind(entry.usage.output_tokens as i32)
            .bind(entry.is_stream)
            .bind(entry.is_tool_call)
            .bind(&entry.error_message)
            .bind(&entry.response_preview)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn query(&self, query: LogQuery) -> anyhow::Result<LogPage> {
        let mut count_sql = String::from("SELECT COUNT(*) AS total FROM request_logs WHERE 1=1");
        let mut data_sql = String::from(
            "SELECT id, DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s') AS created_at, api_key_id, ingress_protocol, egress_protocol, request_model, actual_model, provider_name, status_code, duration_ms, input_tokens, output_tokens, is_stream, is_tool_call, error_message, response_preview FROM request_logs WHERE 1=1",
        );
        let mut bind_values: Vec<String> = Vec::new();

        if let Some(provider) = query.provider.filter(|v| !v.is_empty()) {
            count_sql.push_str(" AND provider_name = ?");
            data_sql.push_str(" AND provider_name = ?");
            bind_values.push(provider);
        }
        if let Some(model) = query.model.filter(|v| !v.is_empty()) {
            count_sql.push_str(" AND actual_model = ?");
            data_sql.push_str(" AND actual_model = ?");
            bind_values.push(model);
        }
        if let Some(status_min) = query.status_min {
            count_sql.push_str(" AND status_code >= ?");
            data_sql.push_str(" AND status_code >= ?");
            bind_values.push(status_min.to_string());
        }
        if let Some(status_max) = query.status_max {
            count_sql.push_str(" AND status_code <= ?");
            data_sql.push_str(" AND status_code <= ?");
            bind_values.push(status_max.to_string());
        }

        data_sql.push_str(" ORDER BY created_at DESC LIMIT ? OFFSET ?");

        let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql);
        let mut data_query = sqlx::query_as::<_, RequestLog>(&data_sql);
        for value in &bind_values {
            count_query = count_query.bind(value);
            data_query = data_query.bind(value);
        }

        let total = count_query.fetch_one(&self.pool).await?;
        let items = data_query
            .bind(query.limit.unwrap_or(50))
            .bind(query.offset.unwrap_or(0))
            .fetch_all(&self.pool)
            .await?;
        Ok(LogPage { items, total })
    }

    async fn cleanup_before(&self, cutoff_expression: &str) -> anyhow::Result<u64> {
        let interval = parse_mysql_interval(cutoff_expression)?;
        let sql = format!(
            "DELETE FROM request_logs WHERE created_at < DATE_SUB(UTC_TIMESTAMP(), {interval})"
        );
        let result = sqlx::query(&sql).execute(&self.pool).await?;
        Ok(result.rows_affected())
    }

    async fn stats_overview(&self, hours: Option<i64>) -> anyhow::Result<StatsOverview> {
        let sql = if let Some(hours) = hours {
            format!(
                "SELECT COUNT(*) AS total_requests, CAST(COALESCE(SUM(input_tokens), 0) AS SIGNED) AS total_input_tokens, CAST(COALESCE(SUM(output_tokens), 0) AS SIGNED) AS total_output_tokens, COALESCE(AVG(duration_ms), 0.0) AS avg_duration_ms, CAST(COALESCE(SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END), 0) AS SIGNED) AS error_count FROM request_logs WHERE created_at >= DATE_SUB(UTC_TIMESTAMP(), INTERVAL {hours} HOUR)"
            )
        } else {
            "SELECT COUNT(*) AS total_requests, CAST(COALESCE(SUM(input_tokens), 0) AS SIGNED) AS total_input_tokens, CAST(COALESCE(SUM(output_tokens), 0) AS SIGNED) AS total_output_tokens, COALESCE(AVG(duration_ms), 0.0) AS avg_duration_ms, CAST(COALESCE(SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END), 0) AS SIGNED) AS error_count FROM request_logs".to_string()
        };
        Ok(sqlx::query_as::<_, StatsOverview>(&sql)
            .fetch_one(&self.pool)
            .await?)
    }

    async fn stats_hourly(&self, hours: i64) -> anyhow::Result<Vec<StatsHourly>> {
        let sql = format!(
            "SELECT DATE_FORMAT(DATE_SUB(created_at, INTERVAL MINUTE(created_at) MINUTE) - INTERVAL SECOND(created_at) SECOND, '%Y-%m-%d %H:00:00') AS hour, COUNT(*) AS request_count, CAST(COALESCE(SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END), 0) AS SIGNED) AS error_count, CAST(COALESCE(SUM(input_tokens), 0) AS SIGNED) AS total_input_tokens, CAST(COALESCE(SUM(output_tokens), 0) AS SIGNED) AS total_output_tokens, COALESCE(AVG(duration_ms), 0.0) AS avg_duration_ms FROM request_logs WHERE created_at >= DATE_SUB(UTC_TIMESTAMP(), INTERVAL {hours} HOUR) GROUP BY hour ORDER BY hour ASC"
        );
        Ok(sqlx::query_as::<_, StatsHourly>(&sql)
            .fetch_all(&self.pool)
            .await?)
    }

    async fn stats_by_model(&self, hours: Option<i64>) -> anyhow::Result<Vec<ModelStats>> {
        let sql = if let Some(hours) = hours {
            format!(
                "SELECT actual_model AS model, COUNT(*) AS request_count, CAST(COALESCE(SUM(input_tokens), 0) AS SIGNED) AS total_input_tokens, CAST(COALESCE(SUM(output_tokens), 0) AS SIGNED) AS total_output_tokens, COALESCE(AVG(duration_ms), 0.0) AS avg_duration_ms FROM request_logs WHERE created_at >= DATE_SUB(UTC_TIMESTAMP(), INTERVAL {hours} HOUR) GROUP BY actual_model ORDER BY request_count DESC"
            )
        } else {
            "SELECT actual_model AS model, COUNT(*) AS request_count, CAST(COALESCE(SUM(input_tokens), 0) AS SIGNED) AS total_input_tokens, CAST(COALESCE(SUM(output_tokens), 0) AS SIGNED) AS total_output_tokens, COALESCE(AVG(duration_ms), 0.0) AS avg_duration_ms FROM request_logs GROUP BY actual_model ORDER BY request_count DESC".to_string()
        };
        Ok(sqlx::query_as::<_, ModelStats>(&sql)
            .fetch_all(&self.pool)
            .await?)
    }

    async fn stats_by_provider(&self, hours: Option<i64>) -> anyhow::Result<Vec<ProviderStats>> {
        let sql = if let Some(hours) = hours {
            format!(
                "SELECT provider_name AS provider, COUNT(*) AS request_count, CAST(COALESCE(SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END), 0) AS SIGNED) AS error_count, COALESCE(AVG(duration_ms), 0.0) AS avg_duration_ms FROM request_logs WHERE created_at >= DATE_SUB(UTC_TIMESTAMP(), INTERVAL {hours} HOUR) GROUP BY provider_name ORDER BY request_count DESC"
            )
        } else {
            "SELECT provider_name AS provider, COUNT(*) AS request_count, CAST(COALESCE(SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END), 0) AS SIGNED) AS error_count, COALESCE(AVG(duration_ms), 0.0) AS avg_duration_ms FROM request_logs GROUP BY provider_name ORDER BY request_count DESC".to_string()
        };
        Ok(sqlx::query_as::<_, ProviderStats>(&sql)
            .fetch_all(&self.pool)
            .await?)
    }
}

#[async_trait]
impl CacheStore for MySqlLogStore {
    async fn get(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let row = sqlx::query_as::<_, (Vec<u8>,)>(
            "SELECT data FROM cache_entries WHERE `key` = ? AND expires_at > UTC_TIMESTAMP()",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|v| v.0))
    }

    async fn set(&self, key: &str, data: &[u8], ttl: Option<Duration>) -> anyhow::Result<()> {
        let ttl_secs = ttl.unwrap_or_else(|| Duration::from_secs(3600)).as_secs() as i64;
        sqlx::query(
            "INSERT INTO cache_entries (`key`, data, expires_at, created_at) VALUES (?, ?, DATE_ADD(UTC_TIMESTAMP(), INTERVAL ? SECOND), UTC_TIMESTAMP()) \
             ON DUPLICATE KEY UPDATE data = VALUES(data), expires_at = VALUES(expires_at)",
        )
        .bind(key)
        .bind(data)
        .bind(ttl_secs)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM cache_entries WHERE `key` = ?")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn flush(&self) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM cache_entries")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn cleanup_expired(&self) -> anyhow::Result<u64> {
        let result = sqlx::query("DELETE FROM cache_entries WHERE expires_at <= UTC_TIMESTAMP()")
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}

#[derive(Clone)]
struct MySqlBootstrap {
    adapter: MySqlAdapter,
}

#[async_trait]
impl StorageBootstrap for MySqlBootstrap {
    async fn init(&self) -> anyhow::Result<()> {
        self.adapter.ping().await
    }

    async fn migrate(&self) -> anyhow::Result<()> {
        for statement in MYSQL_INIT_SQL.split(';') {
            let statement = statement.trim();
            if statement.is_empty() {
                continue;
            }
            if let Err(error) = sqlx::query(statement).execute(self.adapter.pool()).await {
                if is_ignorable_mysql_migration_error(&error) {
                    continue;
                }
                return Err(error.into());
            }
        }
        let _ = sqlx::query("ALTER TABLE routes ADD COLUMN strategy VARCHAR(32) NULL")
            .execute(self.adapter.pool())
            .await;
        let _ = sqlx::query("ALTER TABLE providers ADD COLUMN use_proxy BOOLEAN NOT NULL DEFAULT FALSE")
            .execute(self.adapter.pool())
            .await;
        let _ = sqlx::query("ALTER TABLE providers ADD COLUMN default_protocol VARCHAR(64) NOT NULL DEFAULT ''")
            .execute(self.adapter.pool())
            .await;
        let _ = sqlx::query("ALTER TABLE providers ADD COLUMN protocol_endpoints LONGTEXT NOT NULL DEFAULT '{}'")
            .execute(self.adapter.pool())
            .await;
        let _ = sqlx::query(
            "UPDATE providers SET default_protocol = protocol WHERE (default_protocol IS NULL OR TRIM(default_protocol) = '') AND protocol IS NOT NULL AND TRIM(protocol) != ''",
        )
        .execute(self.adapter.pool())
        .await;
        let _ = sqlx::query(
            "UPDATE providers SET protocol_endpoints = CONCAT('{\"', TRIM(protocol), '\":{\"base_url\":\"', TRIM(base_url), '\"}}') WHERE (protocol_endpoints IS NULL OR TRIM(protocol_endpoints) = '' OR TRIM(protocol_endpoints) = '{}') AND protocol IS NOT NULL AND TRIM(protocol) != '' AND base_url IS NOT NULL AND TRIM(base_url) != ''",
        )
        .execute(self.adapter.pool())
        .await;
        sqlx::query("UPDATE routes SET strategy = 'weighted' WHERE strategy IS NULL OR TRIM(strategy) = ''")
            .execute(self.adapter.pool())
            .await?;
        sqlx::query(
            r#"
            INSERT INTO route_targets (id, route_id, provider_id, model, weight, priority, created_at)
            SELECT LOWER(REPLACE(UUID(), '-', '')), r.id, r.target_provider, r.target_model, 100, 1, UTC_TIMESTAMP()
            FROM routes r
            WHERE r.target_provider IS NOT NULL
              AND TRIM(r.target_provider) != ''
              AND NOT EXISTS (SELECT 1 FROM route_targets rt WHERE rt.route_id = r.id)
            "#,
        )
        .execute(self.adapter.pool())
        .await?;
        Ok(())
    }

    async fn health(&self) -> anyhow::Result<StorageHealth> {
        let health = self.adapter.health().await;
        Ok(StorageHealth {
            backend: StorageBackend::MySql,
            can_connect: health.can_connect,
            schema_compatible: health.schema_compatible,
            writable: health.can_connect,
        })
    }

}

fn provider_select(suffix: Option<&str>) -> String {
    let mut sql = String::from(
        "SELECT id, name, vendor, protocol, base_url, COALESCE(default_protocol, protocol) AS default_protocol, COALESCE(protocol_endpoints, '{}') AS protocol_endpoints, preset_key, channel, models_source, capabilities_source, static_models, api_key, COALESCE(use_proxy, FALSE) AS use_proxy, last_test_success, DATE_FORMAT(last_test_at, '%Y-%m-%d %H:%i:%s') AS last_test_at, is_active, DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s') AS created_at, DATE_FORMAT(updated_at, '%Y-%m-%d %H:%i:%s') AS updated_at FROM providers",
    );
    if let Some(suffix) = suffix {
        sql.push(' ');
        sql.push_str(suffix);
    } else {
        sql.push_str(" ORDER BY created_at DESC");
    }
    sql
}

fn route_select(suffix: Option<&str>) -> String {
    let mut sql = String::from(
        "SELECT id, name, virtual_model, COALESCE(strategy, 'weighted') AS strategy, target_provider, target_model, COALESCE(access_control, FALSE) AS access_control, is_active, DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s') AS created_at FROM routes",
    );
    if let Some(suffix) = suffix {
        sql.push(' ');
        sql.push_str(suffix);
    }
    sql
}

fn api_key_select(suffix: Option<&str>) -> String {
    let mut sql = String::from(
        "SELECT id, `key`, name, rpm, rpd, tpm, tpd, status, DATE_FORMAT(expires_at, '%Y-%m-%d %H:%i:%s') AS expires_at, DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s') AS created_at, DATE_FORMAT(updated_at, '%Y-%m-%d %H:%i:%s') AS updated_at FROM api_keys",
    );
    if let Some(suffix) = suffix {
        sql.push(' ');
        sql.push_str(suffix);
    } else {
        sql.push_str(" ORDER BY created_at DESC");
    }
    sql
}

fn api_key_with_bindings(row: ApiKey, route_ids: Vec<String>) -> ApiKeyWithBindings {
    ApiKeyWithBindings {
        id: row.id,
        key: row.key,
        name: row.name,
        rpm: row.rpm,
        rpd: row.rpd,
        tpm: row.tpm,
        tpd: row.tpd,
        status: row.status,
        expires_at: row.expires_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
        route_ids,
    }
}

fn normalize_provider_vendor(vendor: Option<&str>) -> Option<String> {
    vendor
        .map(str::trim)
        .filter(|v| !v.is_empty() && *v != "custom")
        .map(|v| v.to_lowercase())
}

fn normalize_optional_datetime(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

fn usage_window_interval(window: UsageWindow) -> &'static str {
    match window {
        UsageWindow::Minute => "INTERVAL 1 MINUTE",
        UsageWindow::Day => "INTERVAL 1 DAY",
    }
}

fn parse_mysql_interval(expression: &str) -> anyhow::Result<String> {
    let normalized = expression.trim().trim_start_matches('-').trim();
    let mut parts = normalized.split_whitespace();
    let Some(amount) = parts.next() else {
        bail!("missing mysql interval amount");
    };
    let Some(unit) = parts.next() else {
        bail!("missing mysql interval unit");
    };
    if parts.next().is_some() {
        bail!("unsupported mysql interval expression: {expression}");
    }
    let amount: u64 = amount.parse().context("invalid mysql interval amount")?;
    let unit = match unit.to_ascii_lowercase().as_str() {
        "minute" | "minutes" | "min" | "mins" => "MINUTE",
        "hour" | "hours" | "hr" | "hrs" => "HOUR",
        "day" | "days" => "DAY",
        other => bail!("unsupported mysql interval unit: {other}"),
    };
    Ok(format!("INTERVAL {amount} {unit}"))
}

fn is_ignorable_mysql_migration_error(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(database_error) => {
            matches!(database_error.code().as_deref(), Some("1061"))
        }
        _ => false,
    }
}

async fn list_api_key_route_ids(pool: &Pool<MySql>, api_key_id: &str) -> anyhow::Result<Vec<String>> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT route_id FROM api_key_routes WHERE api_key_id = ? ORDER BY route_id ASC",
    )
    .bind(api_key_id)
    .fetch_all(pool)
    .await?)
}

async fn replace_api_key_routes(
    pool: &Pool<MySql>,
    api_key_id: &str,
    route_ids: &[String],
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM api_key_routes WHERE api_key_id = ?")
        .bind(api_key_id)
        .execute(&mut *tx)
        .await?;

    for route_id in route_ids.iter().filter(|id| !id.trim().is_empty()) {
        sqlx::query(
            "INSERT INTO api_key_routes (api_key_id, route_id) VALUES (?, ?) ON DUPLICATE KEY UPDATE route_id = route_id",
        )
        .bind(api_key_id)
        .bind(route_id.trim())
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

const MYSQL_INIT_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS providers (
    id VARCHAR(64) PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    vendor VARCHAR(64) NULL,
    protocol VARCHAR(64) NOT NULL,
    base_url TEXT NOT NULL,
    preset_key VARCHAR(255) NULL,
    channel VARCHAR(128) NULL,
    models_source TEXT NULL,
    capabilities_source TEXT NULL,
    static_models LONGTEXT NULL,
    api_key TEXT NOT NULL,
    use_proxy BOOLEAN NOT NULL DEFAULT FALSE,
    last_test_success BOOLEAN NULL,
    last_test_at DATETIME NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    priority INT NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS routes (
    id VARCHAR(64) PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    virtual_model VARCHAR(255) NULL,
    strategy VARCHAR(32) NULL,
    target_provider VARCHAR(64) NOT NULL,
    target_model VARCHAR(255) NOT NULL,
    access_control BOOLEAN NOT NULL DEFAULT FALSE,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    priority INT NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT fk_routes_target_provider FOREIGN KEY (target_provider) REFERENCES providers(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS route_targets (
    id VARCHAR(64) PRIMARY KEY,
    route_id VARCHAR(64) NOT NULL,
    provider_id VARCHAR(64) NOT NULL,
    model VARCHAR(255) NOT NULL,
    weight INT DEFAULT 100,
    priority INT DEFAULT 1,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT fk_route_targets_route FOREIGN KEY (route_id) REFERENCES routes(id) ON DELETE CASCADE,
    CONSTRAINT fk_route_targets_provider FOREIGN KEY (provider_id) REFERENCES providers(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE INDEX idx_route_targets_route_id ON route_targets(route_id);

CREATE TABLE IF NOT EXISTS request_logs (
    id VARCHAR(64) PRIMARY KEY,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    api_key_id VARCHAR(64) NULL,
    ingress_protocol VARCHAR(64) NULL,
    egress_protocol VARCHAR(64) NULL,
    request_model VARCHAR(255) NULL,
    actual_model VARCHAR(255) NULL,
    provider_name VARCHAR(255) NULL,
    status_code INT NULL,
    duration_ms DOUBLE NULL,
    input_tokens INT NOT NULL DEFAULT 0,
    output_tokens INT NOT NULL DEFAULT 0,
    is_stream BOOLEAN NOT NULL DEFAULT FALSE,
    is_tool_call BOOLEAN NOT NULL DEFAULT FALSE,
    error_message TEXT NULL,
    response_preview LONGTEXT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE INDEX idx_logs_created_at ON request_logs(created_at);
CREATE INDEX idx_logs_provider ON request_logs(provider_name);
CREATE INDEX idx_logs_status ON request_logs(status_code);
CREATE INDEX idx_logs_model ON request_logs(actual_model);

CREATE TABLE IF NOT EXISTS settings (
    `key` VARCHAR(191) PRIMARY KEY,
    value LONGTEXT NOT NULL,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS cache_entries (
    `key` VARCHAR(255) PRIMARY KEY,
    data LONGBLOB NOT NULL,
    expires_at DATETIME NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE INDEX idx_cache_entries_expires_at ON cache_entries(expires_at);

CREATE TABLE IF NOT EXISTS api_keys (
    id VARCHAR(64) PRIMARY KEY,
    `key` VARCHAR(191) NOT NULL UNIQUE,
    name VARCHAR(255) NOT NULL,
    rpm INT NULL,
    rpd INT NULL,
    tpm INT NULL,
    tpd INT NULL,
    status VARCHAR(32) NOT NULL DEFAULT 'active',
    expires_at DATETIME NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS api_key_routes (
    api_key_id VARCHAR(64) NOT NULL,
    route_id VARCHAR(64) NOT NULL,
    PRIMARY KEY (api_key_id, route_id),
    CONSTRAINT fk_api_key_routes_api_key FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE,
    CONSTRAINT fk_api_key_routes_route FOREIGN KEY (route_id) REFERENCES routes(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE INDEX idx_api_keys_key ON api_keys(`key`);
CREATE INDEX idx_api_key_routes_route_id ON api_key_routes(route_id);
"#;
