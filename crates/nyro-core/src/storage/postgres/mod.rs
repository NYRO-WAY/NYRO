use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use sqlx::{Pool, Postgres};
use std::time::Duration;

use crate::db::models::{
    ApiKey, ApiKeyWithBindings, CreateApiKey, CreateProvider, CreateRoute, CreateRouteTarget,
    LogPage, LogQuery, ModelStats, Provider, ProviderStats, RequestLog, Route, RouteTarget,
    StatsHourly, StatsOverview, UpdateApiKey, UpdateProvider, UpdateRoute,
    is_valid_provider_auth_mode,
};
use crate::logging::LogEntry;
use crate::storage::sql::config::SqlBackendConfig;
use crate::storage::sql::pool::RelationalPool;
use crate::storage::traits::{
    ApiKeyAccessRecord, ApiKeyStore, AuthAccessStore, CacheStore, LogStore, ProviderStore,
    ProviderTestResult, RouteSnapshotStore, RouteStore, RouteTargetStore, SettingsStore, Storage,
    StorageBackend, StorageBootstrap, StorageHealth, UsageWindow,
};

const VECTOR_DIMENSIONS_SETTING_KEY: &str = "vector_embedding_dimensions";

#[derive(Clone)]
pub struct PostgresAdapter {
    pool: Pool<Postgres>,
    config: SqlBackendConfig,
}

#[derive(Debug, Clone)]
pub struct PostgresHealth {
    pub can_connect: bool,
    pub schema_compatible: bool,
}

impl PostgresAdapter {
    pub async fn connect(config: SqlBackendConfig) -> anyhow::Result<Self> {
        let pool = RelationalPool::connect(
            crate::storage::sql::config::SqlBackendKind::Postgres,
            &config,
        )
        .await
        .context("connect postgres adapter")?;
        let pool = pool
            .as_postgres()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("relational pool kind mismatch: expected postgres"))?;
        Ok(Self { pool, config })
    }

    pub fn config(&self) -> &SqlBackendConfig {
        &self.config
    }

    pub fn pool(&self) -> &Pool<Postgres> {
        &self.pool
    }

    pub async fn ping(&self) -> anyhow::Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn health(&self) -> PostgresHealth {
        let can_connect = self.ping().await.is_ok();
        PostgresHealth {
            can_connect,
            schema_compatible: can_connect,
        }
    }
}

#[derive(Clone)]
pub struct PostgresStorage {
    pool: Pool<Postgres>,
    provider_store: Arc<PostgresProviderStore>,
    route_store: Arc<PostgresRouteStore>,
    route_target_store: Arc<PostgresRouteTargetStore>,
    settings_store: Arc<PostgresSettingsStore>,
    api_key_store: Arc<PostgresApiKeyStore>,
    auth_store: Arc<PostgresAuthAccessStore>,
    log_store: Arc<PostgresLogStore>,
    bootstrap: Arc<PostgresBootstrap>,
}

impl PostgresStorage {
    pub async fn connect(config: SqlBackendConfig) -> anyhow::Result<Self> {
        Self::connect_with_vector_dimensions(config, 1536).await
    }

    pub async fn connect_with_vector_dimensions(
        config: SqlBackendConfig,
        vector_dimensions: usize,
    ) -> anyhow::Result<Self> {
        let adapter = PostgresAdapter::connect(config).await?;
        let pool = adapter.pool().clone();
        let provider_store = Arc::new(PostgresProviderStore { pool: pool.clone() });
        let route_store = Arc::new(PostgresRouteStore { pool: pool.clone() });
        let route_target_store = Arc::new(PostgresRouteTargetStore { pool: pool.clone() });
        let settings_store = Arc::new(PostgresSettingsStore { pool: pool.clone() });
        let api_key_store = Arc::new(PostgresApiKeyStore { pool: pool.clone() });
        let auth_store = Arc::new(PostgresAuthAccessStore { pool: pool.clone() });
        let log_store = Arc::new(PostgresLogStore { pool: pool.clone() });
        let bootstrap = Arc::new(PostgresBootstrap {
            adapter,
            vector_dimensions: vector_dimensions.max(1),
        });
        Ok(Self {
            pool,
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

    pub fn pool(&self) -> &Pool<Postgres> {
        &self.pool
    }
}

pub async fn recreate_pg_vector_table(
    pool: &Pool<Postgres>,
    vector_dimensions: usize,
) -> anyhow::Result<()> {
    let dimensions = vector_dimensions.max(1);
    let mut tx = pool.begin().await?;
    let result: anyhow::Result<()> = async {
        sqlx::query("DROP TABLE IF EXISTS semantic_cache_vectors")
            .execute(&mut *tx)
            .await?;
        sqlx::query(&semantic_cache_vectors_table_ddl(dimensions))
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "CREATE UNIQUE INDEX idx_semantic_cache_vectors_partition_key
             ON semantic_cache_vectors(partition, cache_key)",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "CREATE INDEX idx_semantic_cache_vectors_partition_created_at
             ON semantic_cache_vectors(partition, created_at)",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "CREATE INDEX idx_semantic_cache_vectors_hnsw
             ON semantic_cache_vectors USING hnsw (embedding vector_cosine_ops)",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO settings(key, value, updated_at)
             VALUES($1, $2, CURRENT_TIMESTAMP)
             ON CONFLICT(key) DO UPDATE
             SET value = EXCLUDED.value, updated_at = CURRENT_TIMESTAMP",
        )
        .bind(VECTOR_DIMENSIONS_SETTING_KEY)
        .bind(dimensions.to_string())
        .execute(&mut *tx)
        .await?;
        Ok(())
    }
    .await;

    match result {
        Ok(()) => {
            tx.commit().await?;
            Ok(())
        }
        Err(error) => {
            let _ = tx.rollback().await;
            if is_pg_permission_error(&error) {
                anyhow::bail!(
                    "insufficient database privileges to rebuild semantic_cache_vectors automatically. ask your DBA to run:\n\n{}",
                    rebuild_guidance_sql(dimensions)
                );
            }
            Err(error)
        }
    }
}

impl Storage for PostgresStorage {
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
struct PostgresProviderStore {
    pool: Pool<Postgres>,
}

#[async_trait]
impl ProviderStore for PostgresProviderStore {
    async fn list(&self) -> anyhow::Result<Vec<Provider>> {
        Ok(sqlx::query_as::<_, Provider>(&provider_select(None))
            .fetch_all(&self.pool)
            .await?)
    }

    async fn get(&self, id: &str) -> anyhow::Result<Option<Provider>> {
        Ok(
            sqlx::query_as::<_, Provider>(&provider_select(Some("WHERE id = $1")))
                .bind(id)
                .fetch_optional(&self.pool)
                .await?,
        )
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
        if !is_valid_provider_auth_mode(&input.auth_mode) {
            anyhow::bail!("unsupported provider auth_mode: {}", input.auth_mode);
        }
        sqlx::query(
            "INSERT INTO providers (id, name, vendor, protocol, base_url, default_protocol, protocol_endpoints, preset_key, channel, models_source, capabilities_source, static_models, api_key, auth_mode, access_token, refresh_token, expires_at, use_proxy) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)",
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
        .bind(input.auth_mode)
        .bind(input.access_token)
        .bind(input.refresh_token)
        .bind(input.expires_at)
        .bind(input.use_proxy)
        .execute(&self.pool)
        .await?;
        self.get(&id)
            .await?
            .context("provider missing after create")
    }

    async fn update(&self, id: &str, input: UpdateProvider) -> anyhow::Result<Provider> {
        let current = self
            .get(id)
            .await?
            .context("provider not found for update")?;
        let models_source_input = input.effective_models_source().map(ToString::to_string);
        let name = input.name.unwrap_or(current.name);
        let vendor = if input.vendor.is_some() {
            normalize_provider_vendor(input.vendor.as_deref())
        } else {
            normalize_provider_vendor(current.vendor.as_deref())
        };
        let models_source = models_source_input.or_else(|| current.models_source.clone());
        let protocol = input.protocol.unwrap_or(current.protocol.clone());
        let base_url = input.base_url.unwrap_or(current.base_url);
        let default_protocol = input.default_protocol.unwrap_or(current.default_protocol);
        let protocol_endpoints = input
            .protocol_endpoints
            .unwrap_or(current.protocol_endpoints);
        let preset_key = input.preset_key.or(current.preset_key);
        let channel = input.channel.or(current.channel);
        let capabilities_source = input.capabilities_source.or(current.capabilities_source);
        let static_models = input.static_models.or(current.static_models);
        let api_key = input.api_key.unwrap_or(current.api_key);
        let auth_mode = input.auth_mode.unwrap_or(current.auth_mode);
        if !is_valid_provider_auth_mode(&auth_mode) {
            anyhow::bail!("unsupported provider auth_mode: {}", auth_mode);
        }
        let access_token = input.access_token.or(current.access_token);
        let refresh_token = input.refresh_token.or(current.refresh_token);
        let expires_at = input.expires_at.or(current.expires_at);
        let use_proxy = input.use_proxy.unwrap_or(current.use_proxy);
        let is_enabled = input.is_enabled.unwrap_or(current.is_enabled);

        sqlx::query(
            "UPDATE providers SET name=$1, vendor=$2, protocol=$3, base_url=$4, default_protocol=$5, protocol_endpoints=$6, preset_key=$7, channel=$8, models_source=$9, capabilities_source=$10, static_models=$11, api_key=$12, auth_mode=$13, access_token=$14, refresh_token=$15, expires_at=$16, use_proxy=$17, is_enabled=$18, updated_at=CURRENT_TIMESTAMP WHERE id=$19",
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
        .bind(auth_mode)
        .bind(access_token)
        .bind(refresh_token)
        .bind(expires_at)
        .bind(use_proxy)
        .bind(is_enabled)
        .bind(id)
        .execute(&self.pool)
        .await?;
        self.get(id).await?.context("provider missing after update")
    }

    async fn delete(&self, id: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM providers WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn exists_by_name(&self, name: &str, exclude_id: Option<&str>) -> anyhow::Result<bool> {
        let row = if let Some(exclude_id) = exclude_id {
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM providers WHERE lower(trim(name)) = lower(trim($1)) AND id != $2 LIMIT 1",
            )
            .bind(name)
            .bind(exclude_id)
            .fetch_optional(&self.pool)
            .await?
        } else {
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM providers WHERE lower(trim(name)) = lower(trim($1)) LIMIT 1",
            )
            .bind(name)
            .fetch_optional(&self.pool)
            .await?
        };
        Ok(row.is_some())
    }

    async fn record_test_result(
        &self,
        provider_id: &str,
        result: ProviderTestResult,
    ) -> anyhow::Result<()> {
        let _ = result.tested_at;
        sqlx::query(
            "UPDATE providers SET last_test_success = $1, last_test_at = CURRENT_TIMESTAMP WHERE id = $2",
        )
        .bind(result.success)
        .bind(provider_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[derive(Clone)]
struct PostgresRouteStore {
    pool: Pool<Postgres>,
}

#[async_trait]
impl RouteStore for PostgresRouteStore {
    async fn list(&self) -> anyhow::Result<Vec<Route>> {
        Ok(
            sqlx::query_as::<_, Route>(&route_select(Some("ORDER BY created_at DESC")))
                .fetch_all(&self.pool)
                .await?,
        )
    }

    async fn get(&self, id: &str) -> anyhow::Result<Option<Route>> {
        let sql = format!("{} WHERE id = $1", route_select(None));
        Ok(sqlx::query_as::<_, Route>(&sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }

    async fn create(&self, input: CreateRoute) -> anyhow::Result<Route> {
        let id = uuid::Uuid::new_v4().to_string();
        let virtual_model = input.virtual_model.trim().to_string();
        let route_type = input
            .route_type
            .unwrap_or_else(|| "chat".to_string())
            .trim()
            .to_string();
        sqlx::query(
            "INSERT INTO routes (id, name, virtual_model, strategy, target_provider, target_model, access_control, route_type, cache_exact_ttl, cache_semantic_ttl, cache_semantic_threshold) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(&id)
        .bind(input.name.trim())
        .bind(&virtual_model)
        .bind(input.strategy.unwrap_or_else(|| "weighted".to_string()))
        .bind(input.target_provider.trim())
        .bind(input.target_model.trim())
        .bind(input.access_control.unwrap_or(false))
        .bind(route_type)
        .bind(input.cache_exact_ttl)
        .bind(input.cache_semantic_ttl)
        .bind(input.cache_semantic_threshold)
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
        let route_type = input
            .route_type
            .unwrap_or(current.route_type)
            .trim()
            .to_string();
        let cache_exact_ttl = input.cache_exact_ttl;
        let cache_semantic_ttl = input.cache_semantic_ttl;
        let cache_semantic_threshold = input.cache_semantic_threshold;
        let is_enabled = input.is_enabled.unwrap_or(current.is_enabled);

        sqlx::query(
            "UPDATE routes SET name=$1, virtual_model=$2, strategy=$3, target_provider=$4, target_model=$5, access_control=$6, route_type=$7, cache_exact_ttl=$8, cache_semantic_ttl=$9, cache_semantic_threshold=$10, is_enabled=$11 WHERE id=$12",
        )
        .bind(name.trim())
        .bind(&virtual_model)
        .bind(strategy.trim().to_lowercase())
        .bind(target_provider.trim())
        .bind(target_model.trim())
        .bind(access_control)
        .bind(route_type)
        .bind(cache_exact_ttl)
        .bind(cache_semantic_ttl)
        .bind(cache_semantic_threshold)
        .bind(is_enabled)
        .bind(id)
        .execute(&self.pool)
        .await?;
        self.get(id).await?.context("route missing after update")
    }

    async fn delete(&self, id: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM routes WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn exists_by_name(&self, name: &str, exclude_id: Option<&str>) -> anyhow::Result<bool> {
        let row = if let Some(exclude_id) = exclude_id {
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM routes WHERE lower(trim(name)) = lower(trim($1)) AND id != $2 LIMIT 1",
            )
            .bind(name)
            .bind(exclude_id)
            .fetch_optional(&self.pool)
            .await?
        } else {
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM routes WHERE lower(trim(name)) = lower(trim($1)) LIMIT 1",
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
                "SELECT id FROM routes WHERE virtual_model = $1 AND id != $2 LIMIT 1",
            )
            .bind(normalized_model)
            .bind(exclude_id)
            .fetch_optional(&self.pool)
            .await?
        } else {
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM routes WHERE virtual_model = $1 LIMIT 1",
            )
            .bind(normalized_model)
            .fetch_optional(&self.pool)
            .await?
        };
        Ok(row.is_some())
    }
}

#[async_trait]
impl RouteSnapshotStore for PostgresRouteStore {
    async fn load_active_snapshot(&self) -> anyhow::Result<Vec<Route>> {
        let sql = format!("{} WHERE COALESCE(is_enabled, TRUE) = true", route_select(None));
        Ok(sqlx::query_as::<_, Route>(&sql)
            .fetch_all(&self.pool)
            .await?)
    }
}

#[derive(Clone)]
struct PostgresRouteTargetStore {
    pool: Pool<Postgres>,
}

#[async_trait]
impl RouteTargetStore for PostgresRouteTargetStore {
    async fn list_targets_by_route(&self, route_id: &str) -> anyhow::Result<Vec<RouteTarget>> {
        Ok(sqlx::query_as::<_, RouteTarget>(
            "SELECT id, route_id, provider_id, model, weight, priority, to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS created_at FROM route_targets WHERE route_id = $1 ORDER BY priority ASC, created_at ASC",
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
        sqlx::query("DELETE FROM route_targets WHERE route_id = $1")
            .bind(route_id)
            .execute(&mut *tx)
            .await?;

        for target in targets {
            let id = uuid::Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO route_targets (id, route_id, provider_id, model, weight, priority) VALUES ($1, $2, $3, $4, $5, $6)",
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
        sqlx::query("DELETE FROM route_targets WHERE route_id = $1")
            .bind(route_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[derive(Clone)]
struct PostgresSettingsStore {
    pool: Pool<Postgres>,
}

#[async_trait]
impl SettingsStore for PostgresSettingsStore {
    async fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = $1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.0))
    }

    async fn set(&self, key: &str, value: &str) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO settings (key, value, updated_at) VALUES ($1, $2, CURRENT_TIMESTAMP) ON CONFLICT(key) DO UPDATE SET value=EXCLUDED.value, updated_at=EXCLUDED.updated_at",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_all(&self) -> anyhow::Result<Vec<(String, String)>> {
        Ok(
            sqlx::query_as::<_, (String, String)>("SELECT key, value FROM settings")
                .fetch_all(&self.pool)
                .await?,
        )
    }
}

#[derive(Clone)]
struct PostgresApiKeyStore {
    pool: Pool<Postgres>,
}

#[async_trait]
impl ApiKeyStore for PostgresApiKeyStore {
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
        let row = sqlx::query_as::<_, ApiKey>(&api_key_select(Some("WHERE id = $1")))
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
            "INSERT INTO api_keys (id, key, name, rpm, rpd, tpm, tpd, status, expires_at) VALUES ($1, $2, $3, $4, $5, $6, $7, 'active', NULLIF($8, '')::timestamptz)",
        )
        .bind(&id)
        .bind(&key)
        .bind(input.name.trim())
        .bind(input.rpm)
        .bind(input.rpd)
        .bind(input.tpm)
        .bind(input.tpd)
        .bind(input.expires_at.as_deref().map(str::trim).unwrap_or(""))
        .execute(&self.pool)
        .await?;
        replace_api_key_routes(&self.pool, &id, &input.route_ids).await?;
        self.get(&id).await?.context("api key missing after create")
    }

    async fn update(&self, id: &str, input: UpdateApiKey) -> anyhow::Result<ApiKeyWithBindings> {
        let current = sqlx::query_as::<_, ApiKey>(&api_key_select(Some("WHERE id = $1")))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .context("api key not found for update")?;
        let name = input.name.unwrap_or(current.name);
        let rpm = input.rpm.or(current.rpm);
        let rpd = input.rpd.or(current.rpd);
        let tpm = input.tpm.or(current.tpm);
        let tpd = input.tpd.or(current.tpd);
        let is_enabled = input.is_enabled.unwrap_or(current.is_enabled);
        let expires_at = input.expires_at.or(current.expires_at);

        sqlx::query(
            "UPDATE api_keys SET name=$1, rpm=$2, rpd=$3, tpm=$4, tpd=$5, is_enabled=$6, expires_at=NULLIF($7, '')::timestamptz, updated_at=CURRENT_TIMESTAMP WHERE id=$8",
        )
        .bind(name.trim())
        .bind(rpm)
        .bind(rpd)
        .bind(tpm)
        .bind(tpd)
        .bind(is_enabled)
        .bind(expires_at.as_deref().map(str::trim).unwrap_or(""))
        .bind(id)
        .execute(&self.pool)
        .await?;

        if let Some(route_ids) = input.route_ids {
            replace_api_key_routes(&self.pool, id, &route_ids).await?;
        }
        self.get(id).await?.context("api key missing after update")
    }

    async fn delete(&self, id: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM api_keys WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn exists_by_name(&self, name: &str, exclude_id: Option<&str>) -> anyhow::Result<bool> {
        let row = if let Some(exclude_id) = exclude_id {
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM api_keys WHERE lower(trim(name)) = lower(trim($1)) AND id != $2 LIMIT 1",
            )
            .bind(name)
            .bind(exclude_id)
            .fetch_optional(&self.pool)
            .await?
        } else {
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM api_keys WHERE lower(trim(name)) = lower(trim($1)) LIMIT 1",
            )
            .bind(name)
            .fetch_optional(&self.pool)
            .await?
        };
        Ok(row.is_some())
    }
}

#[derive(Clone)]
struct PostgresAuthAccessStore {
    pool: Pool<Postgres>,
}

#[async_trait]
impl AuthAccessStore for PostgresAuthAccessStore {
    async fn find_api_key(&self, raw_key: &str) -> anyhow::Result<Option<ApiKeyAccessRecord>> {
        let row = sqlx::query_as::<
            _,
            (
                String,
                bool,
                Option<String>,
                Option<i32>,
                Option<i32>,
                Option<i32>,
                Option<i32>,
            ),
        >(
            "SELECT id, COALESCE(is_enabled, TRUE) AS is_enabled, to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS expires_at, rpm, rpd, tpm, tpd FROM api_keys WHERE key = $1",
        )
        .bind(raw_key)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(
            |(id, is_enabled, expires_at, rpm, rpd, tpm, tpd)| ApiKeyAccessRecord {
                id,
                is_enabled,
                expires_at,
                rpm,
                rpd,
                tpm,
                tpd,
            },
        ))
    }

    async fn route_binding_exists(&self, api_key_id: &str, route_id: &str) -> anyhow::Result<bool> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM api_key_routes WHERE api_key_id = $1 AND route_id = $2",
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

    async fn request_count_since(
        &self,
        api_key_id: &str,
        window: UsageWindow,
    ) -> anyhow::Result<i64> {
        let interval = interval_expr(window);
        let sql = format!(
            "SELECT COUNT(*) FROM request_logs WHERE api_key_id = $1 AND created_at >= CURRENT_TIMESTAMP - INTERVAL '{interval}'"
        );
        Ok(sqlx::query_scalar::<_, i64>(&sql)
            .bind(api_key_id)
            .fetch_one(&self.pool)
            .await?)
    }

    async fn token_count_since(
        &self,
        api_key_id: &str,
        window: UsageWindow,
    ) -> anyhow::Result<i64> {
        let interval = interval_expr(window);
        let sql = format!(
            "SELECT COALESCE(SUM(input_tokens + output_tokens), 0) FROM request_logs WHERE api_key_id = $1 AND created_at >= CURRENT_TIMESTAMP - INTERVAL '{interval}'"
        );
        Ok(sqlx::query_scalar::<_, i64>(&sql)
            .bind(api_key_id)
            .fetch_one(&self.pool)
            .await?)
    }
}

#[derive(Clone)]
struct PostgresLogStore {
    pool: Pool<Postgres>,
}

#[async_trait]
impl LogStore for PostgresLogStore {
    async fn append_batch(&self, entries: Vec<LogEntry>) -> anyhow::Result<()> {
        for entry in entries {
            let id = uuid::Uuid::new_v4().to_string();
            sqlx::query(
                r#"INSERT INTO request_logs
                    (id, api_key_id, ingress_protocol, egress_protocol, request_model, actual_model,
                     provider_name, status_code, duration_ms, input_tokens, output_tokens,
                     is_stream, is_tool_call, error_message, response_preview,
                     method, path, request_headers, request_body, response_headers, response_body)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21)"#,
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
            .bind(&entry.method)
            .bind(&entry.path)
            .bind(&entry.request_headers)
            .bind(&entry.request_body)
            .bind(&entry.response_headers)
            .bind(&entry.response_body)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn query(&self, query: LogQuery) -> anyhow::Result<LogPage> {
        let mut count_sql = String::from("SELECT COUNT(*) AS total FROM request_logs WHERE 1=1");
        // List query skips heavy body/header columns (NULL placeholders preserve struct layout).
        let mut data_sql = String::from(
            "SELECT id, to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS created_at, api_key_id, ingress_protocol, egress_protocol, request_model, actual_model, provider_name, status_code, duration_ms, input_tokens, output_tokens, is_stream, is_tool_call, error_message, response_preview, method, path, NULL::text AS request_headers, NULL::text AS request_body, NULL::text AS response_headers, NULL::text AS response_body FROM request_logs WHERE 1=1",
        );
        let mut idx = 1;
        let mut bind_values: Vec<String> = Vec::new();

        if let Some(provider) = query.provider.filter(|v| !v.is_empty()) {
            count_sql.push_str(&format!(" AND provider_name = ${idx}"));
            data_sql.push_str(&format!(" AND provider_name = ${idx}"));
            bind_values.push(provider);
            idx += 1;
        }
        if let Some(model) = query.model.filter(|v| !v.is_empty()) {
            count_sql.push_str(&format!(" AND actual_model = ${idx}"));
            data_sql.push_str(&format!(" AND actual_model = ${idx}"));
            bind_values.push(model);
            idx += 1;
        }
        if let Some(status_min) = query.status_min {
            count_sql.push_str(&format!(" AND status_code >= ${idx}"));
            data_sql.push_str(&format!(" AND status_code >= ${idx}"));
            bind_values.push(status_min.to_string());
            idx += 1;
        }
        if let Some(status_max) = query.status_max {
            count_sql.push_str(&format!(" AND status_code <= ${idx}"));
            data_sql.push_str(&format!(" AND status_code <= ${idx}"));
            bind_values.push(status_max.to_string());
            idx += 1;
        }

        data_sql.push_str(&format!(
            " ORDER BY created_at DESC LIMIT ${idx} OFFSET ${}",
            idx + 1
        ));

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

    async fn find_by_id(&self, id: &str) -> anyhow::Result<Option<RequestLog>> {
        let row = sqlx::query_as::<_, RequestLog>(
            "SELECT id, to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS created_at, api_key_id, ingress_protocol, egress_protocol, request_model, actual_model, provider_name, status_code, duration_ms, input_tokens, output_tokens, is_stream, is_tool_call, error_message, response_preview, method, path, request_headers, request_body, response_headers, response_body FROM request_logs WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn cleanup_before(&self, cutoff_expression: &str) -> anyhow::Result<u64> {
        let interval = cutoff_expression.trim().trim_start_matches('-').trim();
        let sql = format!(
            "DELETE FROM request_logs WHERE created_at < CURRENT_TIMESTAMP - INTERVAL '{interval}'"
        );
        let result = sqlx::query(&sql).execute(&self.pool).await?;
        Ok(result.rows_affected())
    }

    async fn stats_overview(&self, hours: Option<i64>) -> anyhow::Result<StatsOverview> {
        let sql = if let Some(hours) = hours {
            format!(
                "SELECT COUNT(*) AS total_requests, COALESCE(SUM(input_tokens), 0) AS total_input_tokens, COALESCE(SUM(output_tokens), 0) AS total_output_tokens, COALESCE(AVG(duration_ms), 0) AS avg_duration_ms, COALESCE(SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END), 0) AS error_count FROM request_logs WHERE created_at >= CURRENT_TIMESTAMP - INTERVAL '{hours} hours'"
            )
        } else {
            "SELECT COUNT(*) AS total_requests, COALESCE(SUM(input_tokens), 0) AS total_input_tokens, COALESCE(SUM(output_tokens), 0) AS total_output_tokens, COALESCE(AVG(duration_ms), 0) AS avg_duration_ms, COALESCE(SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END), 0) AS error_count FROM request_logs".to_string()
        };
        Ok(sqlx::query_as::<_, StatsOverview>(&sql)
            .fetch_one(&self.pool)
            .await?)
    }

    async fn stats_hourly(&self, hours: i64) -> anyhow::Result<Vec<StatsHourly>> {
        let sql = format!(
            "SELECT to_char(date_trunc('hour', created_at AT TIME ZONE 'UTC'), 'YYYY-MM-DD HH24:00:00') AS hour, COUNT(*) AS request_count, COALESCE(SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END), 0) AS error_count, COALESCE(SUM(input_tokens), 0) AS total_input_tokens, COALESCE(SUM(output_tokens), 0) AS total_output_tokens, COALESCE(AVG(duration_ms), 0) AS avg_duration_ms FROM request_logs WHERE created_at >= CURRENT_TIMESTAMP - INTERVAL '{hours} hours' GROUP BY 1 ORDER BY 1 ASC"
        );
        Ok(sqlx::query_as::<_, StatsHourly>(&sql)
            .fetch_all(&self.pool)
            .await?)
    }

    async fn stats_by_model(&self, hours: Option<i64>) -> anyhow::Result<Vec<ModelStats>> {
        let sql = if let Some(hours) = hours {
            format!(
                "SELECT actual_model AS model, COUNT(*) AS request_count, COALESCE(SUM(input_tokens), 0) AS total_input_tokens, COALESCE(SUM(output_tokens), 0) AS total_output_tokens, COALESCE(AVG(duration_ms), 0) AS avg_duration_ms FROM request_logs WHERE created_at >= CURRENT_TIMESTAMP - INTERVAL '{hours} hours' GROUP BY actual_model ORDER BY request_count DESC"
            )
        } else {
            "SELECT actual_model AS model, COUNT(*) AS request_count, COALESCE(SUM(input_tokens), 0) AS total_input_tokens, COALESCE(SUM(output_tokens), 0) AS total_output_tokens, COALESCE(AVG(duration_ms), 0) AS avg_duration_ms FROM request_logs GROUP BY actual_model ORDER BY request_count DESC".to_string()
        };
        Ok(sqlx::query_as::<_, ModelStats>(&sql)
            .fetch_all(&self.pool)
            .await?)
    }

    async fn stats_by_provider(&self, hours: Option<i64>) -> anyhow::Result<Vec<ProviderStats>> {
        let sql = if let Some(hours) = hours {
            format!(
                "SELECT provider_name AS provider, COUNT(*) AS request_count, COALESCE(SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END), 0) AS error_count, COALESCE(AVG(duration_ms), 0) AS avg_duration_ms FROM request_logs WHERE created_at >= CURRENT_TIMESTAMP - INTERVAL '{hours} hours' GROUP BY provider_name ORDER BY request_count DESC"
            )
        } else {
            "SELECT provider_name AS provider, COUNT(*) AS request_count, COALESCE(SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END), 0) AS error_count, COALESCE(AVG(duration_ms), 0) AS avg_duration_ms FROM request_logs GROUP BY provider_name ORDER BY request_count DESC".to_string()
        };
        Ok(sqlx::query_as::<_, ProviderStats>(&sql)
            .fetch_all(&self.pool)
            .await?)
    }
}

#[async_trait]
impl CacheStore for PostgresLogStore {
    async fn get(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let row = sqlx::query_as::<_, (Vec<u8>,)>(
            "SELECT data FROM cache_entries WHERE key = $1 AND expires_at > CURRENT_TIMESTAMP",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|v| v.0))
    }

    async fn set(&self, key: &str, data: &[u8], ttl: Option<Duration>) -> anyhow::Result<()> {
        let ttl_secs = ttl.unwrap_or_else(|| Duration::from_secs(3600)).as_secs() as i64;
        sqlx::query(
            "INSERT INTO cache_entries (key, data, expires_at, created_at) VALUES ($1, $2, CURRENT_TIMESTAMP + ($3 * INTERVAL '1 second'), CURRENT_TIMESTAMP) \
             ON CONFLICT(key) DO UPDATE SET data = EXCLUDED.data, expires_at = EXCLUDED.expires_at",
        )
        .bind(key)
        .bind(data)
        .bind(ttl_secs)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM cache_entries WHERE key = $1")
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
        let result = sqlx::query("DELETE FROM cache_entries WHERE expires_at <= CURRENT_TIMESTAMP")
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}

#[derive(Clone)]
struct PostgresBootstrap {
    adapter: PostgresAdapter,
    vector_dimensions: usize,
}

#[async_trait]
impl StorageBootstrap for PostgresBootstrap {
    async fn init(&self) -> anyhow::Result<()> {
        self.adapter.ping().await
    }

    async fn migrate(&self) -> anyhow::Result<()> {
        sqlx::raw_sql(POSTGRES_INIT_SQL)
            .execute(self.adapter.pool())
            .await?;
        sqlx::query("ALTER TABLE routes ADD COLUMN IF NOT EXISTS strategy TEXT DEFAULT 'weighted'")
            .execute(self.adapter.pool())
            .await?;
        sqlx::query("ALTER TABLE routes ADD COLUMN IF NOT EXISTS route_type TEXT DEFAULT 'chat'")
            .execute(self.adapter.pool())
            .await?;
        sqlx::query("ALTER TABLE routes ADD COLUMN IF NOT EXISTS cache_exact_ttl BIGINT")
            .execute(self.adapter.pool())
            .await?;
        sqlx::query("ALTER TABLE routes ADD COLUMN IF NOT EXISTS cache_semantic_ttl BIGINT")
            .execute(self.adapter.pool())
            .await?;
        sqlx::query(
            "ALTER TABLE routes ADD COLUMN IF NOT EXISTS cache_semantic_threshold DOUBLE PRECISION",
        )
        .execute(self.adapter.pool())
        .await?;
        sqlx::query("UPDATE routes SET strategy = 'weighted' WHERE strategy IS NULL OR btrim(strategy) = ''")
            .execute(self.adapter.pool())
            .await?;
        sqlx::query("UPDATE routes SET route_type = 'chat' WHERE route_type IS NULL OR btrim(route_type) = ''")
            .execute(self.adapter.pool())
            .await?;
        sqlx::query(
            "UPDATE routes SET cache_exact_ttl = cache_ttl WHERE cache_exact_ttl IS NULL AND cache_ttl IS NOT NULL",
        )
        .execute(self.adapter.pool())
        .await
        .ok();
        sqlx::query(
            "UPDATE routes SET cache_exact_ttl = 3600 WHERE cache_enabled = true AND cache_exact_ttl IS NULL",
        )
        .execute(self.adapter.pool())
        .await
        .ok();
        sqlx::query("UPDATE routes SET cache_exact_ttl = NULL WHERE cache_enabled = false")
            .execute(self.adapter.pool())
            .await
            .ok();
        sqlx::query("ALTER TABLE providers ADD COLUMN IF NOT EXISTS use_proxy BOOLEAN NOT NULL DEFAULT FALSE")
            .execute(self.adapter.pool())
            .await?;
        sqlx::query("ALTER TABLE providers ADD COLUMN IF NOT EXISTS auth_mode TEXT NOT NULL DEFAULT 'apikey'")
            .execute(self.adapter.pool())
            .await?;
        sqlx::query("ALTER TABLE providers ADD COLUMN IF NOT EXISTS access_token TEXT")
            .execute(self.adapter.pool())
            .await?;
        sqlx::query("ALTER TABLE providers ADD COLUMN IF NOT EXISTS refresh_token TEXT")
            .execute(self.adapter.pool())
            .await?;
        sqlx::query("ALTER TABLE providers ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ")
            .execute(self.adapter.pool())
            .await?;
        sqlx::query("ALTER TABLE providers DROP CONSTRAINT IF EXISTS providers_auth_mode_check")
            .execute(self.adapter.pool())
            .await?;
        sqlx::query("UPDATE providers SET auth_mode = 'apikey' WHERE auth_mode = 'api_key'")
            .execute(self.adapter.pool())
            .await?;
        sqlx::query(
            r#"DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'providers_auth_mode_check'
    ) THEN
        ALTER TABLE providers
        ADD CONSTRAINT providers_auth_mode_check
        CHECK (auth_mode IN ('apikey', 'oauth'));
    END IF;
END $$;"#,
        )
        .execute(self.adapter.pool())
        .await?;
        sqlx::query("ALTER TABLE providers ADD COLUMN IF NOT EXISTS default_protocol TEXT NOT NULL DEFAULT ''")
            .execute(self.adapter.pool())
            .await?;
        sqlx::query("ALTER TABLE providers ADD COLUMN IF NOT EXISTS protocol_endpoints TEXT NOT NULL DEFAULT '{}'")
            .execute(self.adapter.pool())
            .await?;
        sqlx::query(
            "UPDATE providers SET default_protocol = protocol WHERE (default_protocol IS NULL OR btrim(default_protocol) = '') AND protocol IS NOT NULL AND btrim(protocol) != ''",
        )
        .execute(self.adapter.pool())
        .await?;
        sqlx::query(
            "UPDATE providers SET protocol_endpoints = json_build_object(btrim(protocol), json_build_object('base_url', btrim(base_url)))::text WHERE (protocol_endpoints IS NULL OR btrim(protocol_endpoints) = '' OR btrim(protocol_endpoints) = '{}') AND protocol IS NOT NULL AND btrim(protocol) != '' AND base_url IS NOT NULL AND btrim(base_url) != ''",
        )
        .execute(self.adapter.pool())
        .await?;
        sqlx::query(
            r#"
            INSERT INTO route_targets (id, route_id, provider_id, model, weight, priority)
            SELECT md5(random()::text || clock_timestamp()::text), r.id, r.target_provider, r.target_model, 100, 1
            FROM routes r
            WHERE r.target_provider IS NOT NULL
              AND btrim(r.target_provider) != ''
              AND NOT EXISTS (SELECT 1 FROM route_targets rt WHERE rt.route_id = r.id)
            "#,
        )
        .execute(self.adapter.pool())
        .await?;
        sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
            .execute(self.adapter.pool())
            .await?;
        // Migrate: providers/routes is_active -> is_enabled
        sqlx::query("ALTER TABLE providers ADD COLUMN IF NOT EXISTS is_enabled BOOLEAN DEFAULT TRUE")
            .execute(self.adapter.pool())
            .await?;
        sqlx::query("UPDATE providers SET is_enabled = is_active WHERE is_active IS NOT NULL AND is_enabled IS DISTINCT FROM is_active")
            .execute(self.adapter.pool())
            .await
            .ok();
        sqlx::query("ALTER TABLE routes ADD COLUMN IF NOT EXISTS is_enabled BOOLEAN DEFAULT TRUE")
            .execute(self.adapter.pool())
            .await?;
        sqlx::query("UPDATE routes SET is_enabled = is_active WHERE is_active IS NOT NULL AND is_enabled IS DISTINCT FROM is_active")
            .execute(self.adapter.pool())
            .await
            .ok();
        // Migrate: api_keys status -> is_enabled
        sqlx::query("ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS is_enabled BOOLEAN DEFAULT TRUE")
            .execute(self.adapter.pool())
            .await?;
        sqlx::query(
            "UPDATE api_keys SET is_enabled = CASE WHEN status = 'active' THEN TRUE ELSE FALSE END \
             WHERE status IS NOT NULL AND is_enabled IS DISTINCT FROM (status = 'active')",
        )
        .execute(self.adapter.pool())
        .await
        .ok();
        ensure_semantic_cache_vectors_table(self.adapter.pool(), self.vector_dimensions).await?;
        Ok(())
    }

    async fn health(&self) -> anyhow::Result<StorageHealth> {
        let health = self.adapter.health().await;
        Ok(StorageHealth {
            backend: StorageBackend::Postgres,
            can_connect: health.can_connect,
            schema_compatible: health.schema_compatible,
            writable: health.can_connect,
        })
    }
}

async fn ensure_semantic_cache_vectors_table(
    pool: &Pool<Postgres>,
    vector_dimensions: usize,
) -> anyhow::Result<()> {
    let dimensions = vector_dimensions.max(1);
    let stored_dimension = sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = $1")
        .bind(VECTOR_DIMENSIONS_SETTING_KEY)
        .fetch_optional(pool)
        .await?
        .and_then(|raw| raw.trim().parse::<usize>().ok());
    let table_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1
            FROM information_schema.tables
            WHERE table_schema = current_schema()
              AND table_name = 'semantic_cache_vectors'
        )",
    )
    .fetch_one(pool)
    .await?;
    if !table_exists || stored_dimension != Some(dimensions) {
        recreate_pg_vector_table(pool, dimensions).await?;
    }
    Ok(())
}

fn semantic_cache_vectors_table_ddl(vector_dimensions: usize) -> String {
    let dimensions = vector_dimensions.max(1);
    format!(
        "CREATE TABLE semantic_cache_vectors (
            id BIGSERIAL PRIMARY KEY,
            partition TEXT NOT NULL,
            cache_key TEXT NOT NULL,
            dimensions INTEGER NOT NULL,
            embedding vector({dimensions}) NOT NULL,
            data BYTEA NOT NULL,
            created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
        )"
    )
}

fn rebuild_guidance_sql(vector_dimensions: usize) -> String {
    let dimensions = vector_dimensions.max(1);
    format!(
        "BEGIN;
DROP TABLE IF EXISTS semantic_cache_vectors;
CREATE TABLE semantic_cache_vectors (
    id BIGSERIAL PRIMARY KEY,
    partition TEXT NOT NULL,
    cache_key TEXT NOT NULL,
    dimensions INTEGER NOT NULL,
    embedding vector({dimensions}) NOT NULL,
    data BYTEA NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX idx_semantic_cache_vectors_partition_key ON semantic_cache_vectors(partition, cache_key);
CREATE INDEX idx_semantic_cache_vectors_partition_created_at ON semantic_cache_vectors(partition, created_at);
CREATE INDEX idx_semantic_cache_vectors_hnsw ON semantic_cache_vectors USING hnsw (embedding vector_cosine_ops);
INSERT INTO settings(key, value, updated_at)
VALUES('{VECTOR_DIMENSIONS_SETTING_KEY}', '{dimensions}', CURRENT_TIMESTAMP)
ON CONFLICT(key) DO UPDATE SET value = EXCLUDED.value, updated_at = CURRENT_TIMESTAMP;
COMMIT;"
    )
}

fn is_pg_permission_error(error: &anyhow::Error) -> bool {
    for cause in error.chain() {
        if let Some(sqlx_error) = cause.downcast_ref::<sqlx::Error>() {
            if let sqlx::Error::Database(db_error) = sqlx_error {
                if db_error.code().as_deref() == Some("42501") {
                    return true;
                }
            }
        }
    }
    false
}

fn provider_select(suffix: Option<&str>) -> String {
    let mut sql = String::from(
        "SELECT id, name, vendor, protocol, base_url, COALESCE(default_protocol, protocol) AS default_protocol, COALESCE(protocol_endpoints, '{}') AS protocol_endpoints, preset_key, channel, models_source, capabilities_source, static_models, api_key, COALESCE(auth_mode, 'apikey') AS auth_mode, access_token, refresh_token, to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS expires_at, COALESCE(use_proxy, FALSE) AS use_proxy, last_test_success, to_char(last_test_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS last_test_at, COALESCE(is_enabled, TRUE) AS is_enabled, to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS created_at, to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS updated_at FROM providers",
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
        "SELECT id, name, virtual_model, COALESCE(strategy, 'weighted') AS strategy, target_provider, target_model, COALESCE(access_control, false) AS access_control, COALESCE(route_type, 'chat') AS route_type, cache_exact_ttl, cache_semantic_ttl, cache_semantic_threshold, COALESCE(is_enabled, TRUE) AS is_enabled, to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS created_at FROM routes",
    );
    if let Some(suffix) = suffix {
        sql.push(' ');
        sql.push_str(suffix);
    }
    sql
}

fn api_key_select(suffix: Option<&str>) -> String {
    let mut sql = String::from(
        "SELECT id, key, name, rpm, rpd, tpm, tpd, COALESCE(is_enabled, TRUE) AS is_enabled, to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS expires_at, to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS created_at, to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS updated_at FROM api_keys",
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
        is_enabled: row.is_enabled,
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

fn interval_expr(window: UsageWindow) -> &'static str {
    match window {
        UsageWindow::Minute => "1 minute",
        UsageWindow::Day => "1 day",
    }
}

async fn list_api_key_route_ids(
    pool: &Pool<Postgres>,
    api_key_id: &str,
) -> anyhow::Result<Vec<String>> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT route_id FROM api_key_routes WHERE api_key_id = $1 ORDER BY route_id ASC",
    )
    .bind(api_key_id)
    .fetch_all(pool)
    .await?)
}

async fn replace_api_key_routes(
    pool: &Pool<Postgres>,
    api_key_id: &str,
    route_ids: &[String],
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM api_key_routes WHERE api_key_id = $1")
        .bind(api_key_id)
        .execute(&mut *tx)
        .await?;

    for route_id in route_ids.iter().filter(|id| !id.trim().is_empty()) {
        sqlx::query(
            "INSERT INTO api_key_routes (api_key_id, route_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(api_key_id)
        .bind(route_id.trim())
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

const POSTGRES_INIT_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS providers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    vendor TEXT,
    protocol TEXT NOT NULL,
    base_url TEXT NOT NULL,
    preset_key TEXT,
    channel TEXT,
    models_source TEXT,
    capabilities_source TEXT,
    static_models TEXT,
    api_key TEXT NOT NULL,
    auth_mode TEXT NOT NULL DEFAULT 'apikey' CHECK (auth_mode IN ('apikey', 'oauth')),
    access_token TEXT,
    refresh_token TEXT,
    expires_at TIMESTAMPTZ,
    use_proxy BOOLEAN NOT NULL DEFAULT FALSE,
    last_test_success BOOLEAN,
    last_test_at TIMESTAMPTZ,
    is_enabled BOOLEAN DEFAULT TRUE,
    priority INTEGER DEFAULT 0,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS routes (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    virtual_model TEXT,
    strategy TEXT DEFAULT 'weighted',
    route_type TEXT DEFAULT 'chat',
    target_provider TEXT NOT NULL REFERENCES providers(id),
    target_model TEXT NOT NULL,
    cache_exact_ttl BIGINT,
    cache_semantic_ttl BIGINT,
    cache_semantic_threshold DOUBLE PRECISION,
    access_control BOOLEAN DEFAULT FALSE,
    is_enabled BOOLEAN DEFAULT TRUE,
    priority INTEGER DEFAULT 0,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS route_targets (
    id TEXT PRIMARY KEY,
    route_id TEXT NOT NULL REFERENCES routes(id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL REFERENCES providers(id),
    model TEXT NOT NULL,
    weight INTEGER DEFAULT 100,
    priority INTEGER DEFAULT 1,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_route_targets_route_id ON route_targets(route_id);

CREATE TABLE IF NOT EXISTS request_logs (
    id TEXT PRIMARY KEY,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    api_key_id TEXT,
    ingress_protocol TEXT,
    egress_protocol TEXT,
    request_model TEXT,
    actual_model TEXT,
    provider_name TEXT,
    status_code INTEGER,
    duration_ms DOUBLE PRECISION,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    is_stream BOOLEAN DEFAULT FALSE,
    is_tool_call BOOLEAN DEFAULT FALSE,
    error_message TEXT,
    response_preview TEXT,
    method TEXT,
    path TEXT,
    request_headers TEXT,
    request_body TEXT,
    response_headers TEXT,
    response_body TEXT
);

ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS method TEXT;
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS path TEXT;
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS request_headers TEXT;
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS request_body TEXT;
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS response_headers TEXT;
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS response_body TEXT;

CREATE INDEX IF NOT EXISTS idx_logs_created_at ON request_logs(created_at);
CREATE INDEX IF NOT EXISTS idx_logs_provider ON request_logs(provider_name);
CREATE INDEX IF NOT EXISTS idx_logs_status ON request_logs(status_code);
CREATE INDEX IF NOT EXISTS idx_logs_model ON request_logs(actual_model);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS cache_entries (
    key TEXT PRIMARY KEY,
    data BYTEA NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_cache_entries_expires_at ON cache_entries(expires_at);

CREATE TABLE IF NOT EXISTS api_keys (
    id TEXT PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    rpm INTEGER,
    rpd INTEGER,
    tpm INTEGER,
    tpd INTEGER,
    is_enabled BOOLEAN DEFAULT TRUE,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS api_key_routes (
    api_key_id TEXT NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    route_id TEXT NOT NULL REFERENCES routes(id) ON DELETE CASCADE,
    PRIMARY KEY (api_key_id, route_id)
);

CREATE INDEX IF NOT EXISTS idx_api_keys_key ON api_keys(key);
CREATE INDEX IF NOT EXISTS idx_api_key_routes_route_id ON api_key_routes(route_id);
"#;
