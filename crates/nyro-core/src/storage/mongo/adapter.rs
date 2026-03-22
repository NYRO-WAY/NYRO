use std::time::{Duration, SystemTime};

use anyhow::Context;
use chrono::{DateTime as ChronoDateTime, Duration as ChronoDuration, NaiveDateTime, Utc};
use mongodb::bson::{DateTime, Document, doc};
use mongodb::options::{ClientOptions, IndexOptions};
use mongodb::{Client, Collection, Database, IndexModel};
use serde::de::DeserializeOwned;

use crate::db::models::LogQuery;
use crate::storage::mongo::config::MongoStorageConfig;
use crate::storage::mongo::documents::{
    ApiKeyDocument, ApiKeyRouteBindingDocument, ProviderDocument, RequestLogDocument, RouteDocument,
    SettingDocument,
};
use crate::storage::traits::UsageWindow;

#[derive(Debug, Clone)]
pub struct ApiKeyPolicySnapshot {
    pub id: String,
    pub status: String,
    pub expires_at: Option<DateTime>,
    pub rpm: Option<i32>,
    pub rpd: Option<i32>,
    pub tpm: Option<i32>,
    pub tpd: Option<i32>,
}

#[derive(Debug, Clone, Default)]
pub struct WindowUsage {
    pub request_count: u64,
    pub token_count: i64,
}

#[derive(Debug, Clone, Default)]
pub struct UsageSnapshot {
    pub last_minute: WindowUsage,
    pub last_day: WindowUsage,
}

#[derive(Debug, Clone)]
pub struct AuthorizationContext {
    pub policy: ApiKeyPolicySnapshot,
    pub route_allowed: bool,
}

#[derive(Clone)]
pub struct MongoStorageAdapter {
    pub client: Client,
    pub database: Database,
    pub providers: Collection<ProviderDocument>,
    pub routes: Collection<RouteDocument>,
    pub api_keys: Collection<ApiKeyDocument>,
    pub api_key_routes: Collection<ApiKeyRouteBindingDocument>,
    pub request_logs: Collection<RequestLogDocument>,
    pub settings: Collection<SettingDocument>,
}

impl MongoStorageAdapter {
    pub async fn connect(config: &MongoStorageConfig) -> anyhow::Result<Self> {
        config.validate()?;

        let client_opts = ClientOptions::parse(&config.uri).await?;
        let client = Client::with_options(client_opts)?;
        Self::from_client(client, config)
    }

    pub fn from_client(client: Client, config: &MongoStorageConfig) -> anyhow::Result<Self> {
        config.validate()?;

        let db = client.database(&config.database);
        let names = &config.collections;
        Ok(Self {
            client,
            providers: db.collection::<ProviderDocument>(&names.providers),
            routes: db.collection::<RouteDocument>(&names.routes),
            api_keys: db.collection::<ApiKeyDocument>(&names.api_keys),
            api_key_routes: db.collection::<ApiKeyRouteBindingDocument>(&names.api_key_routes),
            request_logs: db.collection::<RequestLogDocument>(&names.request_logs),
            settings: db.collection::<SettingDocument>(&names.settings),
            database: db,
        })
    }

    pub async fn health_check(&self) -> anyhow::Result<()> {
        self.database.run_command(doc! { "ping": 1 }).await?;
        Ok(())
    }

    pub async fn bootstrap_indexes(&self) -> anyhow::Result<()> {
        let unique = IndexOptions::builder().unique(Some(true)).build();

        self.providers
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "id": 1 })
                    .options(unique.clone())
                    .build(),
            )
            .await?;
        self.providers
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "name_key": 1 })
                    .options(unique.clone())
                    .build(),
            )
            .await?;

        self.routes
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "id": 1 })
                    .options(unique.clone())
                    .build(),
            )
            .await?;
        self.routes
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "name_key": 1 })
                    .options(unique.clone())
                    .build(),
            )
            .await?;
        self.routes
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "route_key": 1 })
                    .options(unique.clone())
                    .build(),
            )
            .await?;

        self.api_keys
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "id": 1 })
                    .options(unique.clone())
                    .build(),
            )
            .await?;
        self.api_keys
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "key": 1 })
                    .options(unique.clone())
                    .build(),
            )
            .await?;
        self.api_keys
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "name_key": 1 })
                    .options(unique.clone())
                    .build(),
            )
            .await?;

        self.api_key_routes
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "api_key_id": 1, "route_id": 1 })
                    .options(unique)
                    .build(),
            )
            .await?;

        self.request_logs
            .create_index(IndexModel::builder().keys(doc! { "created_at": -1 }).build())
            .await?;
        self.request_logs
            .create_index(IndexModel::builder().keys(doc! { "api_key_id": 1, "created_at": -1 }).build())
            .await?;
        self.request_logs
            .create_index(IndexModel::builder().keys(doc! { "provider_name": 1, "created_at": -1 }).build())
            .await?;
        self.request_logs
            .create_index(IndexModel::builder().keys(doc! { "actual_model": 1, "created_at": -1 }).build())
            .await?;

        self.settings
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "key": 1 })
                    .options(IndexOptions::builder().unique(Some(true)).build())
                    .build(),
            )
            .await?;

        Ok(())
    }

    pub async fn list_providers(&self) -> anyhow::Result<Vec<ProviderDocument>> {
        collect(self.providers.find(doc! {}).sort(doc! { "created_at": -1 }).await?).await
    }

    pub async fn get_provider(&self, id: &str) -> anyhow::Result<Option<ProviderDocument>> {
        Ok(self.providers.find_one(doc! { "id": id }).await?)
    }

    pub async fn insert_provider(&self, provider: ProviderDocument) -> anyhow::Result<()> {
        self.providers.insert_one(provider).await?;
        Ok(())
    }

    pub async fn replace_provider(&self, id: &str, provider: ProviderDocument) -> anyhow::Result<()> {
        self.providers.replace_one(doc! { "id": id }, provider).await?;
        Ok(())
    }

    pub async fn delete_provider(&self, id: &str) -> anyhow::Result<()> {
        self.providers.delete_one(doc! { "id": id }).await?;
        Ok(())
    }

    pub async fn provider_exists_by_name_key(
        &self,
        name_key: &str,
        exclude_id: Option<&str>,
    ) -> anyhow::Result<bool> {
        let mut filter = doc! { "name_key": name_key };
        if let Some(exclude_id) = exclude_id {
            filter.insert("id", doc! { "$ne": exclude_id });
        }
        Ok(self.providers.count_documents(filter).await? > 0)
    }

    pub async fn provider_exists(&self, id: &str) -> anyhow::Result<bool> {
        Ok(self.providers.count_documents(doc! { "id": id }).await? > 0)
    }

    pub async fn count_routes_by_provider(&self, provider_id: &str) -> anyhow::Result<u64> {
        Ok(self
            .routes
            .count_documents(doc! { "target_provider": provider_id })
            .await?)
    }

    pub async fn record_provider_test_result(
        &self,
        provider_id: &str,
        success: bool,
        tested_at: DateTime,
    ) -> anyhow::Result<()> {
        self.providers
            .update_one(
                doc! { "id": provider_id },
                doc! {
                    "$set": {
                        "last_test_success": success,
                        "last_test_at": tested_at,
                        "updated_at": DateTime::now(),
                    }
                },
            )
            .await?;
        Ok(())
    }

    pub async fn list_routes(&self) -> anyhow::Result<Vec<RouteDocument>> {
        collect(self.routes.find(doc! {}).sort(doc! { "created_at": -1 }).await?).await
    }

    pub async fn get_route(&self, id: &str) -> anyhow::Result<Option<RouteDocument>> {
        Ok(self.routes.find_one(doc! { "id": id }).await?)
    }

    pub async fn insert_route(&self, route: RouteDocument) -> anyhow::Result<()> {
        self.routes.insert_one(route).await?;
        Ok(())
    }

    pub async fn replace_route(&self, id: &str, route: RouteDocument) -> anyhow::Result<()> {
        self.routes.replace_one(doc! { "id": id }, route).await?;
        Ok(())
    }

    pub async fn delete_route(&self, id: &str) -> anyhow::Result<()> {
        self.api_key_routes.delete_many(doc! { "route_id": id }).await?;
        self.routes.delete_one(doc! { "id": id }).await?;
        Ok(())
    }

    pub async fn route_exists_by_name_key(
        &self,
        name_key: &str,
        exclude_id: Option<&str>,
    ) -> anyhow::Result<bool> {
        let mut filter = doc! { "name_key": name_key };
        if let Some(exclude_id) = exclude_id {
            filter.insert("id", doc! { "$ne": exclude_id });
        }
        Ok(self.routes.count_documents(filter).await? > 0)
    }

    pub async fn route_exists_by_route_key(
        &self,
        route_key: &str,
        exclude_id: Option<&str>,
    ) -> anyhow::Result<bool> {
        let mut filter = doc! { "route_key": route_key };
        if let Some(exclude_id) = exclude_id {
            filter.insert("id", doc! { "$ne": exclude_id });
        }
        Ok(self.routes.count_documents(filter).await? > 0)
    }

    pub async fn list_active_routes(&self) -> anyhow::Result<Vec<RouteDocument>> {
        collect(
            self.routes
                .find(doc! { "is_active": true })
                .sort(doc! { "created_at": -1 })
                .await?,
        )
        .await
    }

    pub async fn find_api_key_policy_by_key(
        &self,
        raw_key: &str,
    ) -> anyhow::Result<Option<ApiKeyPolicySnapshot>> {
        let policy = self.api_keys.find_one(doc! { "key": raw_key }).await?;
        Ok(policy.map(|row| ApiKeyPolicySnapshot {
            id: row.id,
            status: row.status,
            expires_at: row.expires_at,
            rpm: row.rpm,
            rpd: row.rpd,
            tpm: row.tpm,
            tpd: row.tpd,
        }))
    }

    pub async fn list_api_keys(&self) -> anyhow::Result<Vec<ApiKeyDocument>> {
        collect(self.api_keys.find(doc! {}).sort(doc! { "created_at": -1 }).await?).await
    }

    pub async fn get_api_key(&self, id: &str) -> anyhow::Result<Option<ApiKeyDocument>> {
        Ok(self.api_keys.find_one(doc! { "id": id }).await?)
    }

    pub async fn insert_api_key(&self, api_key: ApiKeyDocument) -> anyhow::Result<()> {
        self.api_keys.insert_one(api_key).await?;
        Ok(())
    }

    pub async fn replace_api_key(&self, id: &str, api_key: ApiKeyDocument) -> anyhow::Result<()> {
        self.api_keys.replace_one(doc! { "id": id }, api_key).await?;
        Ok(())
    }

    pub async fn delete_api_key(&self, id: &str) -> anyhow::Result<()> {
        self.api_key_routes
            .delete_many(doc! { "api_key_id": id })
            .await?;
        self.api_keys.delete_one(doc! { "id": id }).await?;
        Ok(())
    }

    pub async fn api_key_exists_by_name_key(
        &self,
        name_key: &str,
        exclude_id: Option<&str>,
    ) -> anyhow::Result<bool> {
        let mut filter = doc! { "name_key": name_key };
        if let Some(exclude_id) = exclude_id {
            filter.insert("id", doc! { "$ne": exclude_id });
        }
        Ok(self.api_keys.count_documents(filter).await? > 0)
    }

    pub async fn list_api_key_route_ids(&self, api_key_id: &str) -> anyhow::Result<Vec<String>> {
        let bindings = collect(
            self.api_key_routes
                .find(doc! { "api_key_id": api_key_id })
                .sort(doc! { "route_id": 1 })
                .await?,
        )
        .await?;
        Ok(bindings.into_iter().map(|binding| binding.route_id).collect())
    }

    pub async fn is_api_key_allowed_for_route(
        &self,
        api_key_id: &str,
        route_id: &str,
    ) -> anyhow::Result<bool> {
        let count = self
            .api_key_routes
            .count_documents(doc! { "api_key_id": api_key_id, "route_id": route_id })
            .await?;
        Ok(count > 0)
    }

    pub async fn resolve_authorization_context(
        &self,
        raw_key: &str,
        route_id: &str,
    ) -> anyhow::Result<Option<AuthorizationContext>> {
        let Some(policy) = self.find_api_key_policy_by_key(raw_key).await? else {
            return Ok(None);
        };

        let route_allowed = self.is_api_key_allowed_for_route(&policy.id, route_id).await?;
        Ok(Some(AuthorizationContext {
            policy,
            route_allowed,
        }))
    }

    pub async fn request_count_since(
        &self,
        api_key_id: &str,
        window: UsageWindow,
    ) -> anyhow::Result<i64> {
        let since = bson_datetime_before(window_duration(window));
        let count = self
            .request_logs
            .count_documents(doc! {
                "api_key_id": api_key_id,
                "created_at": { "$gte": since },
            })
            .await?;
        i64::try_from(count).context("request count overflow")
    }

    pub async fn token_count_since(
        &self,
        api_key_id: &str,
        window: UsageWindow,
    ) -> anyhow::Result<i64> {
        let usage = self
            .usage_window(api_key_id, bson_datetime_before(window_duration(window)))
            .await?;
        Ok(usage.token_count)
    }

    pub async fn usage_for_api_key(&self, api_key_id: &str) -> anyhow::Result<UsageSnapshot> {
        let minute_since = bson_datetime_before(Duration::from_secs(60));
        let day_since = bson_datetime_before(Duration::from_secs(24 * 60 * 60));

        let last_minute = self.usage_window(api_key_id, minute_since).await?;
        let last_day = self.usage_window(api_key_id, day_since).await?;

        Ok(UsageSnapshot {
            last_minute,
            last_day,
        })
    }

    pub async fn append_request_logs(&self, logs: Vec<RequestLogDocument>) -> anyhow::Result<()> {
        if logs.is_empty() {
            return Ok(());
        }
        self.request_logs.insert_many(logs).await?;
        Ok(())
    }

    pub async fn query_request_logs(&self, query: &LogQuery) -> anyhow::Result<(Vec<RequestLogDocument>, i64)> {
        let filter = build_log_filter(query);
        let total = self.request_logs.count_documents(filter.clone()).await?;
        let limit = query.limit.unwrap_or(50).max(0);
        let offset = query.offset.unwrap_or(0).max(0);

        let items = collect(
            self.request_logs
                .find(filter)
                .sort(doc! { "created_at": -1 })
                .skip(offset as u64)
                .limit(limit)
                .await?,
        )
        .await?;

        Ok((items, i64::try_from(total).context("log total overflow")?))
    }

    pub async fn cleanup_logs_before(&self, cutoff: DateTime) -> anyhow::Result<u64> {
        let result = self
            .request_logs
            .delete_many(doc! { "created_at": { "$lt": cutoff } })
            .await?;
        Ok(result.deleted_count)
    }

    pub async fn list_logs_since(&self, hours: Option<i64>) -> anyhow::Result<Vec<RequestLogDocument>> {
        let filter = if let Some(hours) = hours {
            doc! { "created_at": { "$gte": hours_ago(hours) } }
        } else {
            doc! {}
        };
        collect(
            self.request_logs
                .find(filter)
                .sort(doc! { "created_at": -1 })
                .await?,
        )
        .await
    }

    pub async fn get_setting(&self, key: &str) -> anyhow::Result<Option<String>> {
        let row = self.settings.find_one(doc! { "key": key }).await?;
        Ok(row.map(|x| x.value))
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> anyhow::Result<()> {
        self.settings
            .replace_one(
                doc! { "key": key },
                SettingDocument {
                    key: key.to_string(),
                    value: value.to_string(),
                    updated_at: DateTime::now(),
                },
            )
            .upsert(true)
            .await?;
        Ok(())
    }

    pub async fn list_settings(&self) -> anyhow::Result<Vec<SettingDocument>> {
        collect(self.settings.find(doc! {}).sort(doc! { "key": 1 }).await?).await
    }

    pub async fn replace_api_key_routes(
        &self,
        api_key_id: &str,
        route_ids: &[String],
    ) -> anyhow::Result<()> {
        self.api_key_routes
            .delete_many(doc! { "api_key_id": api_key_id })
            .await?;

        let mut seen = std::collections::BTreeSet::new();
        let docs: Vec<ApiKeyRouteBindingDocument> = route_ids
            .iter()
            .filter_map(|route_id| {
                let route_id = route_id.trim();
                if route_id.is_empty() || !seen.insert(route_id.to_string()) {
                    return None;
                }
                Some(ApiKeyRouteBindingDocument {
                    api_key_id: api_key_id.to_string(),
                    route_id: route_id.to_string(),
                })
            })
            .collect();
        if !docs.is_empty() {
            self.api_key_routes.insert_many(docs).await?;
        }
        Ok(())
    }

    pub fn parse_cutoff_expression(&self, cutoff_expression: &str) -> anyhow::Result<DateTime> {
        parse_cutoff_expression(cutoff_expression)
    }

    async fn usage_window(&self, api_key_id: &str, since: DateTime) -> anyhow::Result<WindowUsage> {
        let filter = doc! {
            "api_key_id": api_key_id,
            "created_at": { "$gte": since },
        };

        let mut cursor = self.request_logs.find(filter).await?;
        let mut request_count: u64 = 0;
        let mut token_count: i64 = 0;
        while cursor.advance().await? {
            let log: RequestLogDocument = cursor.deserialize_current()?;
            request_count = request_count.saturating_add(1);
            token_count = token_count
                .saturating_add(i64::from(log.input_tokens))
                .saturating_add(i64::from(log.output_tokens));
        }

        Ok(WindowUsage {
            request_count,
            token_count,
        })
    }
}

async fn collect<T>(mut cursor: mongodb::Cursor<T>) -> anyhow::Result<Vec<T>>
where
    T: DeserializeOwned,
{
    let mut items = Vec::new();
    while cursor.advance().await? {
        items.push(cursor.deserialize_current()?);
    }
    Ok(items)
}

fn build_log_filter(query: &LogQuery) -> Document {
    let mut clauses: Vec<Document> = Vec::new();
    if let Some(provider) = query.provider.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        clauses.push(doc! { "provider_name": provider });
    }
    if let Some(model) = query.model.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        clauses.push(doc! { "actual_model": model });
    }
    if let Some(status_min) = query.status_min {
        clauses.push(doc! { "status_code": { "$gte": status_min } });
    }
    if let Some(status_max) = query.status_max {
        clauses.push(doc! { "status_code": { "$lte": status_max } });
    }

    match clauses.len() {
        0 => doc! {},
        1 => clauses.into_iter().next().unwrap_or_default(),
        _ => doc! { "$and": clauses },
    }
}

fn parse_cutoff_expression(expr: &str) -> anyhow::Result<DateTime> {
    let raw = expr.trim();
    let raw = raw.strip_prefix('-').unwrap_or(raw).trim();
    let mut parts = raw.split_whitespace();
    let amount = parts
        .next()
        .context("cutoff expression missing amount")?
        .parse::<i64>()
        .context("invalid cutoff amount")?;
    let unit = parts.next().context("cutoff expression missing unit")?;

    let delta = match unit {
        "minute" | "minutes" => ChronoDuration::minutes(amount),
        "hour" | "hours" => ChronoDuration::hours(amount),
        "day" | "days" => ChronoDuration::days(amount),
        _ => anyhow::bail!("unsupported cutoff unit: {unit}"),
    };

    let cutoff = Utc::now() - delta;
    Ok(DateTime::from_millis(cutoff.timestamp_millis()))
}

fn bson_datetime_before(duration: Duration) -> DateTime {
    let now = SystemTime::now();
    let ts = now.checked_sub(duration).unwrap_or(SystemTime::UNIX_EPOCH);
    DateTime::from_system_time(ts)
}

fn hours_ago(hours: i64) -> DateTime {
    let cutoff = Utc::now() - ChronoDuration::hours(hours);
    DateTime::from_millis(cutoff.timestamp_millis())
}

fn window_duration(window: UsageWindow) -> Duration {
    match window {
        UsageWindow::Minute => Duration::from_secs(60),
        UsageWindow::Day => Duration::from_secs(24 * 60 * 60),
    }
}

pub fn parse_datetime_string(value: &str) -> anyhow::Result<DateTime> {
    let value = value.trim();
    if value.is_empty() {
        anyhow::bail!("datetime string cannot be empty");
    }

    if let Ok(parsed) = ChronoDateTime::parse_from_rfc3339(value) {
        return Ok(DateTime::from_millis(parsed.timestamp_millis()));
    }

    let parsed = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .with_context(|| format!("unsupported datetime format: {value}"))?;
    Ok(DateTime::from_millis(parsed.and_utc().timestamp_millis()))
}

pub fn format_datetime(value: DateTime) -> String {
    let millis = value.timestamp_millis();
    ChronoDateTime::<Utc>::from_timestamp_millis(millis)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| value.try_to_rfc3339_string().unwrap_or_else(|_| millis.to_string()))
}

pub fn format_optional_datetime(value: Option<DateTime>) -> Option<String> {
    value.map(format_datetime)
}

pub fn normalize_name_key(value: &str) -> String {
    value.trim().to_lowercase()
}

pub fn normalize_route_key(ingress_protocol: &str, virtual_model: &str) -> String {
    format!("{}:{}", ingress_protocol.trim().to_lowercase(), virtual_model.trim())
}
