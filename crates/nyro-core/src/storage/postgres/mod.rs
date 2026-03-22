use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use sqlx::{Pool, Postgres};

use crate::db::models::{
    ApiKey, ApiKeyWithBindings, CreateApiKey, CreateProvider, CreateRoute, LogPage, LogQuery,
    ModelStats, Provider, ProviderStats, RequestLog, Route, StatsHourly, StatsOverview,
    UpdateApiKey, UpdateProvider, UpdateRoute,
};
use crate::logging::LogEntry;
use crate::storage::sql::config::SqlBackendConfig;
use crate::storage::sql::pool::RelationalPool;
use crate::storage::traits::{
    ApiKeyAccessRecord, ApiKeyStore, AuthAccessStore, DynStorage, LogStore, ProviderStore,
    ProviderTestResult, RouteSnapshotStore, RouteStore, SettingsStore, Storage, StorageBackend,
    StorageBootstrap, StorageCapabilities, StorageHealth, UsageWindow,
};

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
        let pool = RelationalPool::connect(crate::storage::sql::config::SqlBackendKind::Postgres, &config)
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
    provider_store: Arc<PostgresProviderStore>,
    route_store: Arc<PostgresRouteStore>,
    settings_store: Arc<PostgresSettingsStore>,
    api_key_store: Arc<PostgresApiKeyStore>,
    auth_store: Arc<PostgresAuthAccessStore>,
    log_store: Arc<PostgresLogStore>,
    bootstrap: Arc<PostgresBootstrap>,
}

impl PostgresStorage {
    pub async fn connect(config: SqlBackendConfig, _fallback: DynStorage) -> anyhow::Result<Self> {
        let adapter = PostgresAdapter::connect(config).await?;
        let provider_store = Arc::new(PostgresProviderStore {
            pool: adapter.pool().clone(),
        });
        let route_store = Arc::new(PostgresRouteStore {
            pool: adapter.pool().clone(),
        });
        let settings_store = Arc::new(PostgresSettingsStore {
            pool: adapter.pool().clone(),
        });
        let api_key_store = Arc::new(PostgresApiKeyStore {
            pool: adapter.pool().clone(),
        });
        let auth_store = Arc::new(PostgresAuthAccessStore {
            pool: adapter.pool().clone(),
        });
        let log_store = Arc::new(PostgresLogStore {
            pool: adapter.pool().clone(),
        });
        let bootstrap = Arc::new(PostgresBootstrap { adapter });
        Ok(Self {
            provider_store,
            route_store,
            settings_store,
            api_key_store,
            auth_store,
            log_store,
            bootstrap,
        })
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

    fn api_keys(&self) -> Option<&dyn ApiKeyStore> {
        Some(self.api_key_store.as_ref())
    }

    fn auth(&self) -> Option<&dyn AuthAccessStore> {
        Some(self.auth_store.as_ref())
    }

    fn logs(&self) -> &dyn LogStore {
        self.log_store.as_ref()
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
        Ok(sqlx::query_as::<_, Provider>(&provider_select(Some("WHERE id = $1")))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }

    async fn create(&self, input: CreateProvider) -> anyhow::Result<Provider> {
        let id = uuid::Uuid::new_v4().to_string();
        let vendor = normalize_provider_vendor(input.vendor.as_deref());
        let models_source = input.effective_models_source().map(ToString::to_string);
        sqlx::query(
            "INSERT INTO providers (id, name, vendor, protocol, base_url, preset_key, channel, models_endpoint, models_source, capabilities_source, static_models, api_key) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(&id)
        .bind(input.name.trim())
        .bind(vendor)
        .bind(input.protocol.trim())
        .bind(input.base_url.trim())
        .bind(input.preset_key)
        .bind(input.channel)
        .bind(models_source.clone())
        .bind(models_source)
        .bind(input.capabilities_source)
        .bind(input.static_models)
        .bind(input.api_key)
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
            .or_else(|| current.models_source.clone().or(current.models_endpoint.clone()));
        let protocol = input.protocol.unwrap_or(current.protocol);
        let base_url = input.base_url.unwrap_or(current.base_url);
        let preset_key = input.preset_key.or(current.preset_key);
        let channel = input.channel.or(current.channel);
        let capabilities_source = input.capabilities_source.or(current.capabilities_source);
        let static_models = input.static_models.or(current.static_models);
        let api_key = input.api_key.unwrap_or(current.api_key);
        let is_active = input.is_active.unwrap_or(current.is_active);

        sqlx::query(
            "UPDATE providers SET name=$1, vendor=$2, protocol=$3, base_url=$4, preset_key=$5, channel=$6, models_endpoint=$7, models_source=$8, capabilities_source=$9, static_models=$10, api_key=$11, is_active=$12, updated_at=CURRENT_TIMESTAMP WHERE id=$13",
        )
        .bind(name.trim())
        .bind(vendor)
        .bind(protocol.trim())
        .bind(base_url.trim())
        .bind(preset_key)
        .bind(channel)
        .bind(models_source.clone())
        .bind(models_source)
        .bind(capabilities_source)
        .bind(static_models)
        .bind(api_key)
        .bind(is_active)
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

    async fn record_test_result(&self, provider_id: &str, result: ProviderTestResult) -> anyhow::Result<()> {
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
        Ok(sqlx::query_as::<_, Route>(&route_select(Some("ORDER BY created_at DESC")))
            .fetch_all(&self.pool)
            .await?)
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
        let ingress_protocol = input.ingress_protocol.trim().to_lowercase();
        let virtual_model = input.virtual_model.trim().to_string();
        sqlx::query(
            "INSERT INTO routes (id, name, ingress_protocol, virtual_model, match_pattern, target_provider, target_model, access_control) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&id)
        .bind(input.name.trim())
        .bind(ingress_protocol)
        .bind(&virtual_model)
        .bind(&virtual_model)
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
        let ingress_protocol = input
            .ingress_protocol
            .unwrap_or(current.ingress_protocol)
            .trim()
            .to_lowercase();
        let virtual_model = input
            .virtual_model
            .unwrap_or(current.virtual_model)
            .trim()
            .to_string();
        let target_provider = input.target_provider.unwrap_or(current.target_provider);
        let target_model = input.target_model.unwrap_or(current.target_model);
        let access_control = input.access_control.unwrap_or(current.access_control);
        let is_active = input.is_active.unwrap_or(current.is_active);

        sqlx::query(
            "UPDATE routes SET name=$1, ingress_protocol=$2, virtual_model=$3, match_pattern=$4, target_provider=$5, target_model=$6, access_control=$7, is_active=$8 WHERE id=$9",
        )
        .bind(name.trim())
        .bind(ingress_protocol)
        .bind(&virtual_model)
        .bind(&virtual_model)
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

    async fn exists_by_protocol_model(
        &self,
        ingress_protocol: &str,
        virtual_model: &str,
        exclude_id: Option<&str>,
    ) -> anyhow::Result<bool> {
        let normalized_protocol = ingress_protocol.trim().to_lowercase();
        let normalized_model = virtual_model.trim();
        let row = if let Some(exclude_id) = exclude_id {
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM routes WHERE COALESCE(ingress_protocol, 'openai') = $1 AND COALESCE(NULLIF(virtual_model, ''), match_pattern) = $2 AND id != $3 LIMIT 1",
            )
            .bind(&normalized_protocol)
            .bind(normalized_model)
            .bind(exclude_id)
            .fetch_optional(&self.pool)
            .await?
        } else {
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM routes WHERE COALESCE(ingress_protocol, 'openai') = $1 AND COALESCE(NULLIF(virtual_model, ''), match_pattern) = $2 LIMIT 1",
            )
            .bind(&normalized_protocol)
            .bind(normalized_model)
            .fetch_optional(&self.pool)
            .await?
        };
        Ok(row.is_some())
    }

    async fn list_active(&self) -> anyhow::Result<Vec<Route>> {
        self.load_active_snapshot().await
    }
}

#[async_trait]
impl RouteSnapshotStore for PostgresRouteStore {
    async fn load_active_snapshot(&self) -> anyhow::Result<Vec<Route>> {
        let sql = format!("{} WHERE is_active = true", route_select(None));
        Ok(sqlx::query_as::<_, Route>(&sql)
            .fetch_all(&self.pool)
            .await?)
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
        Ok(sqlx::query_as::<_, (String, String)>("SELECT key, value FROM settings")
            .fetch_all(&self.pool)
            .await?)
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
        let status = input.status.unwrap_or(current.status);
        let expires_at = input.expires_at.or(current.expires_at);

        sqlx::query(
            "UPDATE api_keys SET name=$1, rpm=$2, rpd=$3, tpm=$4, tpd=$5, status=$6, expires_at=NULLIF($7, '')::timestamptz, updated_at=CURRENT_TIMESTAMP WHERE id=$8",
        )
        .bind(name.trim())
        .bind(rpm)
        .bind(rpd)
        .bind(tpm)
        .bind(tpd)
        .bind(status)
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
                String,
                Option<String>,
                Option<i32>,
                Option<i32>,
                Option<i32>,
                Option<i32>,
            ),
        >(
            "SELECT id, status, to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS expires_at, rpm, rpd, tpm, tpd FROM api_keys WHERE key = $1",
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
            "SELECT COUNT(*) FROM api_key_routes WHERE api_key_id = $1 AND route_id = $2",
        )
        .bind(api_key_id)
        .bind(route_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    async fn request_count_since(&self, api_key_id: &str, window: UsageWindow) -> anyhow::Result<i64> {
        let interval = interval_expr(window);
        let sql = format!(
            "SELECT COUNT(*) FROM request_logs WHERE api_key_id = $1 AND created_at >= CURRENT_TIMESTAMP - INTERVAL '{interval}'"
        );
        Ok(sqlx::query_scalar::<_, i64>(&sql)
            .bind(api_key_id)
            .fetch_one(&self.pool)
            .await?)
    }

    async fn token_count_since(&self, api_key_id: &str, window: UsageWindow) -> anyhow::Result<i64> {
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
                     is_stream, is_tool_call, error_message, request_preview, response_preview)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)"#,
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
            .bind(&entry.request_preview)
            .bind(&entry.response_preview)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn query(&self, query: LogQuery) -> anyhow::Result<LogPage> {
        let mut count_sql = String::from("SELECT COUNT(*) AS total FROM request_logs WHERE 1=1");
        let mut data_sql = String::from(
            "SELECT id, to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS created_at, api_key_id, ingress_protocol, egress_protocol, request_model, actual_model, provider_name, status_code, duration_ms, input_tokens, output_tokens, is_stream, is_tool_call, error_message, request_preview, response_preview FROM request_logs WHERE 1=1",
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

        data_sql.push_str(&format!(" ORDER BY created_at DESC LIMIT ${idx} OFFSET ${}", idx + 1));

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
        let interval = cutoff_expression.trim().trim_start_matches('-').trim();
        let sql = format!("DELETE FROM request_logs WHERE created_at < CURRENT_TIMESTAMP - INTERVAL '{interval}'");
        let result = sqlx::query(&sql).execute(&self.pool).await?;
        Ok(result.rows_affected())
    }

    async fn stats_overview(&self, hours: Option<i64>) -> anyhow::Result<StatsOverview> {
        let sql = if let Some(hours) = hours {
            format!("SELECT COUNT(*) AS total_requests, COALESCE(SUM(input_tokens), 0) AS total_input_tokens, COALESCE(SUM(output_tokens), 0) AS total_output_tokens, COALESCE(AVG(duration_ms), 0) AS avg_duration_ms, COALESCE(SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END), 0) AS error_count FROM request_logs WHERE created_at >= CURRENT_TIMESTAMP - INTERVAL '{hours} hours'")
        } else {
            "SELECT COUNT(*) AS total_requests, COALESCE(SUM(input_tokens), 0) AS total_input_tokens, COALESCE(SUM(output_tokens), 0) AS total_output_tokens, COALESCE(AVG(duration_ms), 0) AS avg_duration_ms, COALESCE(SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END), 0) AS error_count FROM request_logs".to_string()
        };
        Ok(sqlx::query_as::<_, StatsOverview>(&sql)
            .fetch_one(&self.pool)
            .await?)
    }

    async fn stats_hourly(&self, hours: i64) -> anyhow::Result<Vec<StatsHourly>> {
        let sql = format!("SELECT to_char(date_trunc('hour', created_at AT TIME ZONE 'UTC'), 'YYYY-MM-DD HH24:00:00') AS hour, COUNT(*) AS request_count, COALESCE(SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END), 0) AS error_count, COALESCE(SUM(input_tokens), 0) AS total_input_tokens, COALESCE(SUM(output_tokens), 0) AS total_output_tokens, COALESCE(AVG(duration_ms), 0) AS avg_duration_ms FROM request_logs WHERE created_at >= CURRENT_TIMESTAMP - INTERVAL '{hours} hours' GROUP BY 1 ORDER BY 1 ASC");
        Ok(sqlx::query_as::<_, StatsHourly>(&sql)
            .fetch_all(&self.pool)
            .await?)
    }

    async fn stats_by_model(&self, hours: Option<i64>) -> anyhow::Result<Vec<ModelStats>> {
        let sql = if let Some(hours) = hours {
            format!("SELECT actual_model AS model, COUNT(*) AS request_count, COALESCE(SUM(input_tokens), 0) AS total_input_tokens, COALESCE(SUM(output_tokens), 0) AS total_output_tokens, COALESCE(AVG(duration_ms), 0) AS avg_duration_ms FROM request_logs WHERE created_at >= CURRENT_TIMESTAMP - INTERVAL '{hours} hours' GROUP BY actual_model ORDER BY request_count DESC")
        } else {
            "SELECT actual_model AS model, COUNT(*) AS request_count, COALESCE(SUM(input_tokens), 0) AS total_input_tokens, COALESCE(SUM(output_tokens), 0) AS total_output_tokens, COALESCE(AVG(duration_ms), 0) AS avg_duration_ms FROM request_logs GROUP BY actual_model ORDER BY request_count DESC".to_string()
        };
        Ok(sqlx::query_as::<_, ModelStats>(&sql)
            .fetch_all(&self.pool)
            .await?)
    }

    async fn stats_by_provider(&self, hours: Option<i64>) -> anyhow::Result<Vec<ProviderStats>> {
        let sql = if let Some(hours) = hours {
            format!("SELECT provider_name AS provider, COUNT(*) AS request_count, COALESCE(SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END), 0) AS error_count, COALESCE(AVG(duration_ms), 0) AS avg_duration_ms FROM request_logs WHERE created_at >= CURRENT_TIMESTAMP - INTERVAL '{hours} hours' GROUP BY provider_name ORDER BY request_count DESC")
        } else {
            "SELECT provider_name AS provider, COUNT(*) AS request_count, COALESCE(SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END), 0) AS error_count, COALESCE(AVG(duration_ms), 0) AS avg_duration_ms FROM request_logs GROUP BY provider_name ORDER BY request_count DESC".to_string()
        };
        Ok(sqlx::query_as::<_, ProviderStats>(&sql)
            .fetch_all(&self.pool)
            .await?)
    }
}

#[derive(Clone)]
struct PostgresBootstrap {
    adapter: PostgresAdapter,
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

    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities {
            transactions: true,
            batch_writes: true,
            aggregations: true,
            managed_migrations: true,
        }
    }
}

fn provider_select(suffix: Option<&str>) -> String {
    let mut sql = String::from(
        "SELECT id, name, vendor, protocol, base_url, preset_key, COALESCE(channel, region) AS channel, models_endpoint, COALESCE(models_source, models_endpoint) AS models_source, capabilities_source, static_models, api_key, last_test_success, to_char(last_test_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS last_test_at, is_active, to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS created_at, to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS updated_at FROM providers",
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
        "SELECT id, name, COALESCE(ingress_protocol, 'openai') AS ingress_protocol, COALESCE(NULLIF(virtual_model, ''), match_pattern) AS virtual_model, target_provider, target_model, COALESCE(access_control, false) AS access_control, is_active, to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS created_at FROM routes",
    );
    if let Some(suffix) = suffix {
        sql.push(' ');
        sql.push_str(suffix);
    }
    sql
}

fn api_key_select(suffix: Option<&str>) -> String {
    let mut sql = String::from(
        "SELECT id, key, name, rpm, rpd, tpm, tpd, status, to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS expires_at, to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS created_at, to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') AS updated_at FROM api_keys",
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

fn interval_expr(window: UsageWindow) -> &'static str {
    match window {
        UsageWindow::Minute => "1 minute",
        UsageWindow::Day => "1 day",
    }
}

async fn list_api_key_route_ids(pool: &Pool<Postgres>, api_key_id: &str) -> anyhow::Result<Vec<String>> {
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
    region TEXT,
    channel TEXT,
    models_endpoint TEXT,
    models_source TEXT,
    capabilities_source TEXT,
    static_models TEXT,
    api_key TEXT NOT NULL,
    last_test_success BOOLEAN,
    last_test_at TIMESTAMPTZ,
    is_active BOOLEAN DEFAULT TRUE,
    priority INTEGER DEFAULT 0,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS routes (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    match_pattern TEXT NOT NULL,
    ingress_protocol TEXT,
    virtual_model TEXT,
    target_provider TEXT NOT NULL REFERENCES providers(id),
    target_model TEXT NOT NULL,
    fallback_provider TEXT REFERENCES providers(id),
    fallback_model TEXT,
    access_control BOOLEAN DEFAULT FALSE,
    is_active BOOLEAN DEFAULT TRUE,
    priority INTEGER DEFAULT 0,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

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
    request_preview TEXT,
    response_preview TEXT
);

CREATE INDEX IF NOT EXISTS idx_logs_created_at ON request_logs(created_at);
CREATE INDEX IF NOT EXISTS idx_logs_provider ON request_logs(provider_name);
CREATE INDEX IF NOT EXISTS idx_logs_status ON request_logs(status_code);
CREATE INDEX IF NOT EXISTS idx_logs_model ON request_logs(actual_model);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS api_keys (
    id TEXT PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    rpm INTEGER,
    rpd INTEGER,
    tpm INTEGER,
    tpd INTEGER,
    status TEXT NOT NULL DEFAULT 'active',
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
