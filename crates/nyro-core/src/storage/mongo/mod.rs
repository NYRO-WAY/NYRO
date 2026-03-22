pub mod adapter;
pub mod config;
pub mod documents;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;

pub use adapter::{
    ApiKeyPolicySnapshot, AuthorizationContext, MongoStorageAdapter, UsageSnapshot, WindowUsage,
};
use adapter::{
    format_datetime, format_optional_datetime, normalize_name_key, normalize_route_key,
    parse_datetime_string,
};
pub use config::{MongoCollectionNames, MongoStorageConfig};
use documents::{ApiKeyDocument, ProviderDocument, RequestLogDocument, RouteDocument};

use crate::db::models::{
    ApiKey, ApiKeyWithBindings, CreateApiKey, CreateProvider, CreateRoute, LogPage, LogQuery,
    ModelStats, Provider, ProviderStats, RequestLog, Route, StatsHourly, StatsOverview,
    UpdateApiKey, UpdateProvider, UpdateRoute,
};
use crate::logging::LogEntry;
use crate::storage::traits::{
    ApiKeyAccessRecord, ApiKeyStore, AuthAccessStore, DynStorage, LogStore, ProviderStore,
    ProviderTestResult, RouteSnapshotStore, RouteStore, SettingsStore, Storage, StorageBackend,
    StorageBootstrap, StorageCapabilities, StorageHealth, UsageWindow,
};

#[derive(Clone)]
pub struct MongoStorage {
    provider_store: Arc<MongoProviderStore>,
    route_store: Arc<MongoRouteStore>,
    settings_store: Arc<MongoSettingsStore>,
    api_key_store: Arc<MongoApiKeyStore>,
    auth_store: Arc<MongoAuthAccessStore>,
    log_store: Arc<MongoLogStore>,
    bootstrap: Arc<MongoBootstrap>,
}

impl MongoStorage {
    pub async fn connect(config: MongoStorageConfig, _fallback: DynStorage) -> anyhow::Result<Self> {
        let adapter = MongoStorageAdapter::connect(&config).await?;
        let provider_store = Arc::new(MongoProviderStore {
            adapter: adapter.clone(),
        });
        let route_store = Arc::new(MongoRouteStore {
            adapter: adapter.clone(),
        });
        let settings_store = Arc::new(MongoSettingsStore {
            adapter: adapter.clone(),
        });
        let api_key_store = Arc::new(MongoApiKeyStore {
            adapter: adapter.clone(),
        });
        let auth_store = Arc::new(MongoAuthAccessStore {
            adapter: adapter.clone(),
        });
        let log_store = Arc::new(MongoLogStore {
            adapter: adapter.clone(),
        });
        let bootstrap = Arc::new(MongoBootstrap { adapter });

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

impl Storage for MongoStorage {
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
struct MongoProviderStore {
    adapter: MongoStorageAdapter,
}

#[async_trait]
impl ProviderStore for MongoProviderStore {
    async fn list(&self) -> anyhow::Result<Vec<Provider>> {
        Ok(self
            .adapter
            .list_providers()
            .await?
            .into_iter()
            .map(provider_from_document)
            .collect())
    }

    async fn get(&self, id: &str) -> anyhow::Result<Option<Provider>> {
        Ok(self.adapter.get_provider(id).await?.map(provider_from_document))
    }

    async fn create(&self, input: CreateProvider) -> anyhow::Result<Provider> {
        let id = uuid::Uuid::new_v4().to_string();
        let provider = provider_document_from_create(id.clone(), input);
        self.adapter.insert_provider(provider).await?;
        self.get(&id).await?.context("provider missing after create")
    }

    async fn update(&self, id: &str, input: UpdateProvider) -> anyhow::Result<Provider> {
        let current = self
            .adapter
            .get_provider(id)
            .await?
            .context("provider not found for update")?;
        let updated = provider_document_from_update(current, input);
        self.adapter.replace_provider(id, updated).await?;
        self.get(id).await?.context("provider missing after update")
    }

    async fn delete(&self, id: &str) -> anyhow::Result<()> {
        if self.adapter.count_routes_by_provider(id).await? > 0 {
            anyhow::bail!("provider is still referenced by routes");
        }
        self.adapter.delete_provider(id).await
    }

    async fn exists_by_name(&self, name: &str, exclude_id: Option<&str>) -> anyhow::Result<bool> {
        self.adapter
            .provider_exists_by_name_key(&normalize_name_key(name), exclude_id)
            .await
    }

    async fn record_test_result(&self, provider_id: &str, result: ProviderTestResult) -> anyhow::Result<()> {
        let tested_at = parse_datetime_string(&result.tested_at)
            .unwrap_or_else(|_| mongodb::bson::DateTime::now());
        self.adapter
            .record_provider_test_result(provider_id, result.success, tested_at)
            .await
    }
}

#[derive(Clone)]
struct MongoRouteStore {
    adapter: MongoStorageAdapter,
}

#[async_trait]
impl RouteStore for MongoRouteStore {
    async fn list(&self) -> anyhow::Result<Vec<Route>> {
        Ok(self
            .adapter
            .list_routes()
            .await?
            .into_iter()
            .map(route_from_document)
            .collect())
    }

    async fn get(&self, id: &str) -> anyhow::Result<Option<Route>> {
        Ok(self.adapter.get_route(id).await?.map(route_from_document))
    }

    async fn create(&self, input: CreateRoute) -> anyhow::Result<Route> {
        ensure_provider_exists(&self.adapter, &input.target_provider).await?;
        let id = uuid::Uuid::new_v4().to_string();
        let route = route_document_from_create(id.clone(), input);
        self.adapter.insert_route(route).await?;
        self.get(&id).await?.context("route missing after create")
    }

    async fn update(&self, id: &str, input: UpdateRoute) -> anyhow::Result<Route> {
        let current = self
            .adapter
            .get_route(id)
            .await?
            .context("route not found for update")?;
        if let Some(target_provider) = input.target_provider.as_deref() {
            ensure_provider_exists(&self.adapter, target_provider).await?;
        }
        let updated = route_document_from_update(current, input);
        self.adapter.replace_route(id, updated).await?;
        self.get(id).await?.context("route missing after update")
    }

    async fn delete(&self, id: &str) -> anyhow::Result<()> {
        self.adapter.delete_route(id).await
    }

    async fn exists_by_name(&self, name: &str, exclude_id: Option<&str>) -> anyhow::Result<bool> {
        self.adapter
            .route_exists_by_name_key(&normalize_name_key(name), exclude_id)
            .await
    }

    async fn exists_by_protocol_model(
        &self,
        ingress_protocol: &str,
        virtual_model: &str,
        exclude_id: Option<&str>,
    ) -> anyhow::Result<bool> {
        self.adapter
            .route_exists_by_route_key(
                &normalize_route_key(ingress_protocol, virtual_model),
                exclude_id,
            )
            .await
    }

    async fn list_active(&self) -> anyhow::Result<Vec<Route>> {
        self.load_active_snapshot().await
    }
}

#[async_trait]
impl RouteSnapshotStore for MongoRouteStore {
    async fn load_active_snapshot(&self) -> anyhow::Result<Vec<Route>> {
        Ok(self
            .adapter
            .list_active_routes()
            .await?
            .into_iter()
            .map(route_from_document)
            .collect())
    }
}

#[derive(Clone)]
struct MongoSettingsStore {
    adapter: MongoStorageAdapter,
}

#[async_trait]
impl SettingsStore for MongoSettingsStore {
    async fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        self.adapter.get_setting(key).await
    }

    async fn set(&self, key: &str, value: &str) -> anyhow::Result<()> {
        self.adapter.set_setting(key, value).await
    }

    async fn list_all(&self) -> anyhow::Result<Vec<(String, String)>> {
        Ok(self
            .adapter
            .list_settings()
            .await?
            .into_iter()
            .map(|doc| (doc.key, doc.value))
            .collect())
    }
}

#[derive(Clone)]
struct MongoApiKeyStore {
    adapter: MongoStorageAdapter,
}

#[async_trait]
impl ApiKeyStore for MongoApiKeyStore {
    async fn list(&self) -> anyhow::Result<Vec<ApiKeyWithBindings>> {
        let rows = self.adapter.list_api_keys().await?;
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let route_ids = self.adapter.list_api_key_route_ids(&row.id).await?;
            items.push(api_key_with_bindings_from_document(row, route_ids));
        }
        Ok(items)
    }

    async fn get(&self, id: &str) -> anyhow::Result<Option<ApiKeyWithBindings>> {
        let row = self.adapter.get_api_key(id).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let route_ids = self.adapter.list_api_key_route_ids(id).await?;
        Ok(Some(api_key_with_bindings_from_document(row, route_ids)))
    }

    async fn create(&self, input: CreateApiKey) -> anyhow::Result<ApiKeyWithBindings> {
        let id = uuid::Uuid::new_v4().to_string();
        let route_ids = validate_route_ids(&self.adapter, &input.route_ids).await?;
        let api_key = api_key_document_from_create(id.clone(), input)?;
        self.adapter.insert_api_key(api_key).await?;
        self.adapter.replace_api_key_routes(&id, &route_ids).await?;
        self.get(&id).await?.context("api key missing after create")
    }

    async fn update(&self, id: &str, input: UpdateApiKey) -> anyhow::Result<ApiKeyWithBindings> {
        let current = self
            .adapter
            .get_api_key(id)
            .await?
            .context("api key not found for update")?;
        let route_ids = match input.route_ids.as_ref() {
            Some(route_ids) => Some(validate_route_ids(&self.adapter, route_ids).await?),
            None => None,
        };
        let updated = api_key_document_from_update(current, input)?;
        self.adapter.replace_api_key(id, updated).await?;
        if let Some(route_ids) = route_ids {
            self.adapter.replace_api_key_routes(id, &route_ids).await?;
        }
        self.get(id).await?.context("api key missing after update")
    }

    async fn delete(&self, id: &str) -> anyhow::Result<()> {
        self.adapter.delete_api_key(id).await
    }

    async fn exists_by_name(&self, name: &str, exclude_id: Option<&str>) -> anyhow::Result<bool> {
        self.adapter
            .api_key_exists_by_name_key(&normalize_name_key(name), exclude_id)
            .await
    }
}

#[derive(Clone)]
struct MongoAuthAccessStore {
    adapter: MongoStorageAdapter,
}

#[async_trait]
impl AuthAccessStore for MongoAuthAccessStore {
    async fn find_api_key(&self, raw_key: &str) -> anyhow::Result<Option<ApiKeyAccessRecord>> {
        Ok(self
            .adapter
            .find_api_key_policy_by_key(raw_key)
            .await?
            .map(|policy| ApiKeyAccessRecord {
                id: policy.id,
                status: policy.status,
                expires_at: format_optional_datetime(policy.expires_at),
                rpm: policy.rpm,
                rpd: policy.rpd,
                tpm: policy.tpm,
                tpd: policy.tpd,
            }))
    }

    async fn route_binding_exists(&self, api_key_id: &str, route_id: &str) -> anyhow::Result<bool> {
        self.adapter.is_api_key_allowed_for_route(api_key_id, route_id).await
    }

    async fn request_count_since(&self, api_key_id: &str, window: UsageWindow) -> anyhow::Result<i64> {
        self.adapter.request_count_since(api_key_id, window).await
    }

    async fn token_count_since(&self, api_key_id: &str, window: UsageWindow) -> anyhow::Result<i64> {
        self.adapter.token_count_since(api_key_id, window).await
    }
}

#[derive(Clone)]
struct MongoLogStore {
    adapter: MongoStorageAdapter,
}

#[async_trait]
impl LogStore for MongoLogStore {
    async fn append_batch(&self, entries: Vec<LogEntry>) -> anyhow::Result<()> {
        let logs = entries
            .into_iter()
            .map(request_log_document_from_entry)
            .collect::<Vec<_>>();
        self.adapter.append_request_logs(logs).await
    }

    async fn query(&self, query: LogQuery) -> anyhow::Result<LogPage> {
        let (items, total) = self.adapter.query_request_logs(&query).await?;
        Ok(LogPage {
            items: items.into_iter().map(request_log_from_document).collect(),
            total,
        })
    }

    async fn cleanup_before(&self, cutoff_expression: &str) -> anyhow::Result<u64> {
        let cutoff = self.adapter.parse_cutoff_expression(cutoff_expression)?;
        self.adapter.cleanup_logs_before(cutoff).await
    }

    async fn stats_overview(&self, hours: Option<i64>) -> anyhow::Result<StatsOverview> {
        let logs = self.adapter.list_logs_since(hours).await?;
        Ok(compute_overview(&logs))
    }

    async fn stats_hourly(&self, hours: i64) -> anyhow::Result<Vec<StatsHourly>> {
        let logs = self.adapter.list_logs_since(Some(hours)).await?;
        Ok(compute_hourly(&logs))
    }

    async fn stats_by_model(&self, hours: Option<i64>) -> anyhow::Result<Vec<ModelStats>> {
        let logs = self.adapter.list_logs_since(hours).await?;
        Ok(compute_model_stats(&logs))
    }

    async fn stats_by_provider(&self, hours: Option<i64>) -> anyhow::Result<Vec<ProviderStats>> {
        let logs = self.adapter.list_logs_since(hours).await?;
        Ok(compute_provider_stats(&logs))
    }
}

#[derive(Clone)]
struct MongoBootstrap {
    adapter: MongoStorageAdapter,
}

#[async_trait]
impl StorageBootstrap for MongoBootstrap {
    async fn init(&self) -> anyhow::Result<()> {
        self.adapter.health_check().await
    }

    async fn migrate(&self) -> anyhow::Result<()> {
        self.adapter.bootstrap_indexes().await
    }

    async fn health(&self) -> anyhow::Result<StorageHealth> {
        let can_connect = self.adapter.health_check().await.is_ok();
        Ok(StorageHealth {
            backend: StorageBackend::Mongo,
            can_connect,
            schema_compatible: can_connect,
            writable: can_connect,
        })
    }

    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities {
            transactions: false,
            batch_writes: true,
            aggregations: true,
            managed_migrations: false,
        }
    }
}

fn provider_from_document(doc: ProviderDocument) -> Provider {
    Provider {
        id: doc.id,
        name: doc.name,
        vendor: doc.vendor,
        protocol: doc.protocol,
        base_url: doc.base_url,
        preset_key: doc.preset_key,
        channel: doc.channel,
        models_endpoint: doc.models_endpoint.clone().or_else(|| doc.models_source.clone()),
        models_source: doc.models_source.or(doc.models_endpoint),
        capabilities_source: doc.capabilities_source,
        static_models: doc.static_models,
        api_key: doc.api_key,
        last_test_success: doc.last_test_success,
        last_test_at: format_optional_datetime(doc.last_test_at),
        is_active: doc.is_active,
        created_at: format_datetime(doc.created_at),
        updated_at: format_datetime(doc.updated_at),
    }
}

fn provider_document_from_create(id: String, input: CreateProvider) -> ProviderDocument {
    let now = mongodb::bson::DateTime::now();
    let name = input.name.trim().to_string();
    let protocol = input.protocol.trim().to_string();
    let base_url = input.base_url.trim().to_string();
    let models_source = input.effective_models_source().map(|v| v.trim().to_string());

    ProviderDocument {
        id,
        name_key: normalize_name_key(&name),
        name,
        vendor: normalize_provider_vendor(input.vendor.as_deref()),
        protocol,
        base_url,
        preset_key: input.preset_key,
        channel: input.channel,
        models_endpoint: models_source.clone(),
        models_source,
        capabilities_source: input.capabilities_source,
        static_models: input.static_models,
        api_key: input.api_key,
        last_test_success: None,
        last_test_at: None,
        is_active: true,
        created_at: now,
        updated_at: now,
    }
}

fn provider_document_from_update(current: ProviderDocument, input: UpdateProvider) -> ProviderDocument {
    let models_source_input = input.effective_models_source().map(|v| v.trim().to_string());
    let name = input.name.unwrap_or(current.name).trim().to_string();
    let protocol = input.protocol.unwrap_or(current.protocol).trim().to_string();
    let base_url = input.base_url.unwrap_or(current.base_url).trim().to_string();
    let models_source = models_source_input
        .or_else(|| current.models_source.clone().or(current.models_endpoint.clone()));

    ProviderDocument {
        id: current.id,
        name_key: normalize_name_key(&name),
        name,
        vendor: if input.vendor.is_some() {
            normalize_provider_vendor(input.vendor.as_deref())
        } else {
            current.vendor
        },
        protocol,
        base_url,
        preset_key: input.preset_key.or(current.preset_key),
        channel: input.channel.or(current.channel),
        models_endpoint: models_source.clone(),
        models_source,
        capabilities_source: input.capabilities_source.or(current.capabilities_source),
        static_models: input.static_models.or(current.static_models),
        api_key: input.api_key.unwrap_or(current.api_key),
        last_test_success: current.last_test_success,
        last_test_at: current.last_test_at,
        is_active: input.is_active.unwrap_or(current.is_active),
        created_at: current.created_at,
        updated_at: mongodb::bson::DateTime::now(),
    }
}

fn route_from_document(doc: RouteDocument) -> Route {
    Route {
        id: doc.id,
        name: doc.name,
        ingress_protocol: doc.ingress_protocol,
        virtual_model: doc.virtual_model,
        target_provider: doc.target_provider,
        target_model: doc.target_model,
        access_control: doc.access_control,
        is_active: doc.is_active,
        created_at: format_datetime(doc.created_at),
    }
}

fn route_document_from_create(id: String, input: CreateRoute) -> RouteDocument {
    let name = input.name.trim().to_string();
    let ingress_protocol = input.ingress_protocol.trim().to_lowercase();
    let virtual_model = input.virtual_model.trim().to_string();

    RouteDocument {
        id,
        name_key: normalize_name_key(&name),
        name,
        ingress_protocol: ingress_protocol.clone(),
        virtual_model: virtual_model.clone(),
        route_key: normalize_route_key(&ingress_protocol, &virtual_model),
        target_provider: input.target_provider.trim().to_string(),
        target_model: input.target_model.trim().to_string(),
        access_control: input.access_control.unwrap_or(false),
        is_active: true,
        created_at: mongodb::bson::DateTime::now(),
    }
}

fn route_document_from_update(current: RouteDocument, input: UpdateRoute) -> RouteDocument {
    let name = input.name.unwrap_or(current.name).trim().to_string();
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

    RouteDocument {
        id: current.id,
        name_key: normalize_name_key(&name),
        name,
        ingress_protocol: ingress_protocol.clone(),
        virtual_model: virtual_model.clone(),
        route_key: normalize_route_key(&ingress_protocol, &virtual_model),
        target_provider: input
            .target_provider
            .unwrap_or(current.target_provider)
            .trim()
            .to_string(),
        target_model: input
            .target_model
            .unwrap_or(current.target_model)
            .trim()
            .to_string(),
        access_control: input.access_control.unwrap_or(current.access_control),
        is_active: input.is_active.unwrap_or(current.is_active),
        created_at: current.created_at,
    }
}

fn api_key_with_bindings_from_document(doc: ApiKeyDocument, route_ids: Vec<String>) -> ApiKeyWithBindings {
    let base = api_key_from_document(doc);
    ApiKeyWithBindings {
        id: base.id,
        key: base.key,
        name: base.name,
        rpm: base.rpm,
        rpd: base.rpd,
        tpm: base.tpm,
        tpd: base.tpd,
        status: base.status,
        expires_at: base.expires_at,
        created_at: base.created_at,
        updated_at: base.updated_at,
        route_ids,
    }
}

fn api_key_from_document(doc: ApiKeyDocument) -> ApiKey {
    ApiKey {
        id: doc.id,
        key: doc.key,
        name: doc.name,
        rpm: doc.rpm,
        rpd: doc.rpd,
        tpm: doc.tpm,
        tpd: doc.tpd,
        status: doc.status,
        expires_at: format_optional_datetime(doc.expires_at),
        created_at: format_datetime(doc.created_at),
        updated_at: format_datetime(doc.updated_at),
    }
}

fn api_key_document_from_create(id: String, input: CreateApiKey) -> anyhow::Result<ApiKeyDocument> {
    let now = mongodb::bson::DateTime::now();
    let name = input.name.trim().to_string();

    Ok(ApiKeyDocument {
        id,
        key: format!("sk-{}", uuid::Uuid::new_v4().simple()),
        name_key: normalize_name_key(&name),
        name,
        rpm: input.rpm,
        rpd: input.rpd,
        tpm: input.tpm,
        tpd: input.tpd,
        status: "active".to_string(),
        expires_at: input
            .expires_at
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(parse_datetime_string)
            .transpose()?,
        created_at: now,
        updated_at: now,
    })
}

fn api_key_document_from_update(
    current: ApiKeyDocument,
    input: UpdateApiKey,
) -> anyhow::Result<ApiKeyDocument> {
    let name = input.name.unwrap_or(current.name).trim().to_string();
    let expires_at = match input.expires_at {
        Some(value) => {
            let value = value.trim().to_string();
            if value.is_empty() {
                None
            } else {
                Some(parse_datetime_string(&value)?)
            }
        }
        None => current.expires_at,
    };

    Ok(ApiKeyDocument {
        id: current.id,
        key: current.key,
        name_key: normalize_name_key(&name),
        name,
        rpm: input.rpm.or(current.rpm),
        rpd: input.rpd.or(current.rpd),
        tpm: input.tpm.or(current.tpm),
        tpd: input.tpd.or(current.tpd),
        status: input.status.unwrap_or(current.status),
        expires_at,
        created_at: current.created_at,
        updated_at: mongodb::bson::DateTime::now(),
    })
}

fn request_log_document_from_entry(entry: LogEntry) -> RequestLogDocument {
    RequestLogDocument {
        id: uuid::Uuid::new_v4().to_string(),
        created_at: mongodb::bson::DateTime::now(),
        api_key_id: entry.api_key_id,
        ingress_protocol: Some(entry.ingress_protocol),
        egress_protocol: Some(entry.egress_protocol),
        request_model: Some(entry.request_model),
        actual_model: Some(entry.actual_model),
        provider_name: Some(entry.provider_name),
        status_code: Some(entry.status_code),
        duration_ms: Some(entry.duration_ms),
        input_tokens: entry.usage.input_tokens as i32,
        output_tokens: entry.usage.output_tokens as i32,
        is_stream: entry.is_stream,
        is_tool_call: entry.is_tool_call,
        error_message: entry.error_message,
        request_preview: entry.request_preview,
        response_preview: entry.response_preview,
    }
}

fn request_log_from_document(doc: RequestLogDocument) -> RequestLog {
    RequestLog {
        id: doc.id,
        created_at: format_datetime(doc.created_at),
        api_key_id: doc.api_key_id,
        ingress_protocol: doc.ingress_protocol,
        egress_protocol: doc.egress_protocol,
        request_model: doc.request_model,
        actual_model: doc.actual_model,
        provider_name: doc.provider_name,
        status_code: doc.status_code,
        duration_ms: doc.duration_ms,
        input_tokens: doc.input_tokens,
        output_tokens: doc.output_tokens,
        is_stream: doc.is_stream,
        is_tool_call: doc.is_tool_call,
        error_message: doc.error_message,
        request_preview: doc.request_preview,
        response_preview: doc.response_preview,
    }
}

#[derive(Default)]
struct RunningStats {
    request_count: i64,
    error_count: i64,
    total_input_tokens: i64,
    total_output_tokens: i64,
    duration_sum: f64,
    duration_samples: i64,
}

impl RunningStats {
    fn push(&mut self, log: &RequestLogDocument) {
        self.request_count += 1;
        self.total_input_tokens += i64::from(log.input_tokens);
        self.total_output_tokens += i64::from(log.output_tokens);
        if log.status_code.unwrap_or_default() >= 400 {
            self.error_count += 1;
        }
        if let Some(duration_ms) = log.duration_ms {
            self.duration_sum += duration_ms;
            self.duration_samples += 1;
        }
    }

    fn avg_duration_ms(&self) -> f64 {
        if self.duration_samples == 0 {
            0.0
        } else {
            self.duration_sum / self.duration_samples as f64
        }
    }
}

fn compute_overview(logs: &[RequestLogDocument]) -> StatsOverview {
    let mut stats = RunningStats::default();
    for log in logs {
        stats.push(log);
    }
    StatsOverview {
        total_requests: stats.request_count,
        total_input_tokens: stats.total_input_tokens,
        total_output_tokens: stats.total_output_tokens,
        avg_duration_ms: stats.avg_duration_ms(),
        error_count: stats.error_count,
    }
}

fn compute_hourly(logs: &[RequestLogDocument]) -> Vec<StatsHourly> {
    let mut buckets: BTreeMap<String, RunningStats> = BTreeMap::new();
    for log in logs {
        let hour = format_datetime(log.created_at)
            .chars()
            .take(13)
            .collect::<String>()
            + ":00:00";
        buckets.entry(hour).or_default().push(log);
    }

    buckets
        .into_iter()
        .map(|(hour, stats)| StatsHourly {
            hour,
            request_count: stats.request_count,
            error_count: stats.error_count,
            total_input_tokens: stats.total_input_tokens,
            total_output_tokens: stats.total_output_tokens,
            avg_duration_ms: stats.avg_duration_ms(),
        })
        .collect()
}

fn compute_model_stats(logs: &[RequestLogDocument]) -> Vec<ModelStats> {
    let mut buckets: HashMap<String, RunningStats> = HashMap::new();
    for log in logs {
        let model = log.actual_model.clone().unwrap_or_default();
        buckets.entry(model).or_default().push(log);
    }

    let mut items = buckets
        .into_iter()
        .map(|(model, stats)| ModelStats {
            model,
            request_count: stats.request_count,
            total_input_tokens: stats.total_input_tokens,
            total_output_tokens: stats.total_output_tokens,
            avg_duration_ms: stats.avg_duration_ms(),
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        b.request_count
            .cmp(&a.request_count)
            .then_with(|| a.model.cmp(&b.model))
    });
    items
}

fn compute_provider_stats(logs: &[RequestLogDocument]) -> Vec<ProviderStats> {
    let mut buckets: HashMap<String, RunningStats> = HashMap::new();
    for log in logs {
        let provider = log.provider_name.clone().unwrap_or_default();
        buckets.entry(provider).or_default().push(log);
    }

    let mut items = buckets
        .into_iter()
        .map(|(provider, stats)| ProviderStats {
            provider,
            request_count: stats.request_count,
            error_count: stats.error_count,
            avg_duration_ms: stats.avg_duration_ms(),
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        b.request_count
            .cmp(&a.request_count)
            .then_with(|| a.provider.cmp(&b.provider))
    });
    items
}

fn normalize_provider_vendor(vendor: Option<&str>) -> Option<String> {
    vendor
        .map(str::trim)
        .filter(|v| !v.is_empty() && *v != "custom")
        .map(|v| v.to_lowercase())
}

async fn ensure_provider_exists(adapter: &MongoStorageAdapter, provider_id: &str) -> anyhow::Result<()> {
    let provider_id = provider_id.trim();
    if provider_id.is_empty() {
        anyhow::bail!("target_provider cannot be empty");
    }
    if !adapter.provider_exists(provider_id).await? {
        anyhow::bail!("target provider not found: {provider_id}");
    }
    Ok(())
}

async fn validate_route_ids(
    adapter: &MongoStorageAdapter,
    route_ids: &[String],
) -> anyhow::Result<Vec<String>> {
    let normalized = route_ids
        .iter()
        .map(|route_id| route_id.trim())
        .filter(|route_id| !route_id.is_empty())
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>();

    let mut validated = Vec::with_capacity(normalized.len());
    for route_id in normalized {
        if adapter.get_route(&route_id).await?.is_none() {
            anyhow::bail!("route not found: {route_id}");
        }
        validated.push(route_id);
    }
    Ok(validated)
}
