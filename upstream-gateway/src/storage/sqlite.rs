use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context;
use async_trait::async_trait;
use dashmap::DashMap;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool, Transaction};

use crate::config::ModelRateLimitRule;
use crate::provider::{
    GatewayKey, GatewayModelRule, GatewayProvider, GatewayProviderBundle, GatewayProviderSummary,
    ProviderVendor,
};
use crate::runtime::UpstreamRateLimitSummary;

use super::{GatewayConfigStore, SharedProviderBundle, normalize_bundle};

#[derive(Debug, Clone)]
pub struct SqliteGatewayConfigStore {
    pool: SqlitePool,
    provider_cache: DashMap<String, SharedProviderBundle>,
    cache_complete: Arc<AtomicBool>,
}

impl SqliteGatewayConfigStore {
    pub async fn connect(database_url: &str) -> anyhow::Result<Self> {
        let options = SqliteConnectOptions::from_str(database_url)
            .with_context(|| format!("failed to parse sqlite database url: {database_url}"))?
            .create_if_missing(true)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .with_context(|| format!("failed to connect sqlite database: {database_url}"))?;

        let store = Self {
            pool,
            provider_cache: DashMap::new(),
            cache_complete: Arc::new(AtomicBool::new(false)),
        };
        store.initialize_schema().await?;
        Ok(store)
    }

    pub async fn seed_if_empty(&self, bundles: &[GatewayProviderBundle]) -> anyhow::Result<usize> {
        let existing = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM gateway_providers")
            .fetch_one(&self.pool)
            .await
            .context("failed to count sqlite providers")?;
        if existing > 0 {
            return Ok(0);
        }

        self.replace_all_provider_bundles(bundles).await?;
        Ok(bundles.len())
    }

    pub async fn replace_all_provider_bundles(
        &self,
        bundles: &[GatewayProviderBundle],
    ) -> anyhow::Result<()> {
        let normalized_bundles = bundles
            .iter()
            .cloned()
            .map(normalize_bundle)
            .collect::<anyhow::Result<Vec<_>>>()?;
        let mut tx = self
            .pool
            .begin()
            .await
            .context("failed to begin sqlite transaction")?;

        sqlx::query("DELETE FROM gateway_model_rules")
            .execute(&mut *tx)
            .await
            .context("failed to clear gateway_model_rules")?;
        sqlx::query("DELETE FROM gateway_keys")
            .execute(&mut *tx)
            .await
            .context("failed to clear gateway_keys")?;
        sqlx::query("DELETE FROM gateway_providers")
            .execute(&mut *tx)
            .await
            .context("failed to clear gateway_providers")?;

        for bundle in &normalized_bundles {
            self.insert_bundle(&mut tx, bundle).await?;
        }

        tx.commit()
            .await
            .context("failed to commit sqlite provider import")?;
        self.replace_provider_cache(
            normalized_bundles
                .into_iter()
                .map(Arc::new)
                .collect::<Vec<_>>(),
        );
        self.cache_complete.store(true, Ordering::Release);
        Ok(())
    }

    async fn initialize_schema(&self) -> anyhow::Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS gateway_providers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                vendor TEXT NOT NULL,
                base_url TEXT NOT NULL,
                auth_strategy_json TEXT NOT NULL,
                daily_reset_json TEXT NOT NULL,
                enabled INTEGER NOT NULL
            )",
        )
        .execute(&self.pool)
        .await
        .context("failed to create gateway_providers")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS gateway_keys (
                provider_id TEXT NOT NULL,
                id TEXT NOT NULL,
                display_name TEXT NULL,
                api_key TEXT NOT NULL,
                enabled INTEGER NOT NULL,
                weight INTEGER NULL,
                PRIMARY KEY (provider_id, id),
                FOREIGN KEY(provider_id) REFERENCES gateway_providers(id) ON DELETE CASCADE
            )",
        )
        .execute(&self.pool)
        .await
        .context("failed to create gateway_keys")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS gateway_model_rules (
                provider_id TEXT NOT NULL,
                model TEXT NOT NULL,
                rule_json TEXT NOT NULL,
                PRIMARY KEY (provider_id, model),
                FOREIGN KEY(provider_id) REFERENCES gateway_providers(id) ON DELETE CASCADE
            )",
        )
        .execute(&self.pool)
        .await
        .context("failed to create gateway_model_rules")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS gateway_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await
        .context("failed to create gateway_settings")?;

        Ok(())
    }

    async fn insert_bundle(
        &self,
        tx: &mut Transaction<'_, sqlx::Sqlite>,
        bundle: &GatewayProviderBundle,
    ) -> anyhow::Result<()> {
        let vendor = provider_vendor_to_db(bundle.provider.vendor);
        let auth_strategy_json = serde_json::to_string(&bundle.provider.auth_strategy)
            .context("serialize auth strategy")?;
        let daily_reset_json =
            serde_json::to_string(&bundle.daily_reset).context("serialize daily_reset")?;

        sqlx::query(
            "INSERT INTO gateway_providers
                (id, name, vendor, base_url, auth_strategy_json, daily_reset_json, enabled)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&bundle.provider.id)
        .bind(&bundle.provider.name)
        .bind(vendor)
        .bind(&bundle.provider.base_url)
        .bind(auth_strategy_json)
        .bind(daily_reset_json)
        .bind(i64::from(bundle.provider.enabled))
        .execute(&mut **tx)
        .await
        .with_context(|| format!("failed to insert provider '{}'", bundle.provider.id))?;

        for key in &bundle.keys {
            sqlx::query(
                "INSERT INTO gateway_keys
                    (provider_id, id, display_name, api_key, enabled, weight)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&key.provider_id)
            .bind(&key.id)
            .bind(&key.display_name)
            .bind(&key.api_key)
            .bind(i64::from(key.enabled))
            .bind(key.weight.map(i64::from))
            .execute(&mut **tx)
            .await
            .with_context(|| {
                format!(
                    "failed to insert key '{}' for provider '{}'",
                    key.id, key.provider_id
                )
            })?;
        }

        for rule in &bundle.model_rules {
            let rule_json =
                serde_json::to_string(&rule.rule).context("serialize model rate limit rule")?;
            sqlx::query(
                "INSERT INTO gateway_model_rules
                    (provider_id, model, rule_json)
                 VALUES (?, ?, ?)",
            )
            .bind(&rule.provider_id)
            .bind(&rule.rule.model)
            .bind(rule_json)
            .execute(&mut **tx)
            .await
            .with_context(|| {
                format!(
                    "failed to insert model rule '{}' for provider '{}'",
                    rule.rule.model, rule.provider_id
                )
            })?;
        }

        Ok(())
    }

    fn cached_bundle(&self, provider_id: &str) -> Option<GatewayProviderBundle> {
        self.provider_cache
            .get(provider_id)
            .map(|entry| entry.as_ref().clone())
    }

    fn cached_bundle_shared(&self, provider_id: &str) -> Option<SharedProviderBundle> {
        self.provider_cache
            .get(provider_id)
            .map(|entry| entry.clone())
    }

    fn cache_bundle(&self, bundle: GatewayProviderBundle) -> SharedProviderBundle {
        let bundle = Arc::new(bundle);
        self.provider_cache
            .insert(bundle.provider.id.clone(), bundle.clone());
        bundle
    }

    fn replace_provider_cache<I>(&self, bundles: I)
    where
        I: IntoIterator<Item = SharedProviderBundle>,
    {
        self.provider_cache.clear();
        for bundle in bundles {
            self.provider_cache
                .insert(bundle.provider.id.clone(), bundle);
        }
    }

    fn cached_bundles_shared(&self) -> Option<Vec<SharedProviderBundle>> {
        if !self.cache_complete.load(Ordering::Acquire) {
            return None;
        }

        let mut bundles = self
            .provider_cache
            .iter()
            .map(|entry| entry.clone())
            .collect::<Vec<_>>();
        bundles.sort_by(|left, right| left.provider.id.cmp(&right.provider.id));
        Some(bundles)
    }

    async fn get_cached_or_fetch_bundle(
        &self,
        provider_id: &str,
    ) -> anyhow::Result<Option<GatewayProviderBundle>> {
        if let Some(bundle) = self.cached_bundle(provider_id) {
            return Ok(Some(bundle));
        }

        let Some(bundle) = self.fetch_bundle(provider_id).await? else {
            return Ok(None);
        };

        self.cache_bundle(bundle.clone());
        Ok(Some(bundle))
    }

    async fn get_cached_or_fetch_bundle_shared(
        &self,
        provider_id: &str,
    ) -> anyhow::Result<Option<SharedProviderBundle>> {
        if let Some(bundle) = self.cached_bundle_shared(provider_id) {
            return Ok(Some(bundle));
        }

        let Some(bundle) = self.fetch_bundle(provider_id).await? else {
            return Ok(None);
        };

        Ok(Some(self.cache_bundle(bundle)))
    }

    async fn fetch_bundle(
        &self,
        provider_id: &str,
    ) -> anyhow::Result<Option<GatewayProviderBundle>> {
        let Some(provider_row) = sqlx::query(
            "SELECT id, name, vendor, base_url, auth_strategy_json, daily_reset_json, enabled
             FROM gateway_providers
             WHERE id = ?",
        )
        .bind(provider_id)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("failed to fetch provider '{provider_id}'"))?
        else {
            return Ok(None);
        };

        let daily_reset = serde_json::from_str(&provider_row.get::<String, _>("daily_reset_json"))
            .context("failed to deserialize daily_reset_json")?;

        let provider = GatewayProvider {
            id: provider_row.get::<String, _>("id"),
            name: provider_row.get::<String, _>("name"),
            vendor: provider_vendor_from_db(&provider_row.get::<String, _>("vendor"))?,
            base_url: provider_row.get::<String, _>("base_url"),
            auth_strategy: serde_json::from_str(
                &provider_row.get::<String, _>("auth_strategy_json"),
            )
            .context("failed to deserialize auth_strategy_json")?,
            enabled: provider_row.get::<i64, _>("enabled") != 0,
        };

        let key_rows = sqlx::query(
            "SELECT provider_id, id, display_name, api_key, enabled, weight
             FROM gateway_keys
             WHERE provider_id = ?
             ORDER BY id ASC",
        )
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await
        .with_context(|| format!("failed to fetch keys for provider '{provider_id}'"))?;
        let keys = key_rows
            .into_iter()
            .map(|row| -> anyhow::Result<GatewayKey> {
                Ok(GatewayKey {
                    id: row.get::<String, _>("id"),
                    provider_id: row.get::<String, _>("provider_id"),
                    display_name: row.get::<Option<String>, _>("display_name"),
                    api_key: row.get::<String, _>("api_key"),
                    enabled: row.get::<i64, _>("enabled") != 0,
                    weight: row
                        .get::<Option<i64>, _>("weight")
                        .map(u32::try_from)
                        .transpose()
                        .context("gateway_keys.weight exceeds u32 range")?,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let rule_rows = sqlx::query(
            "SELECT provider_id, rule_json
             FROM gateway_model_rules
             WHERE provider_id = ?
             ORDER BY model ASC",
        )
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await
        .with_context(|| format!("failed to fetch model rules for provider '{provider_id}'"))?;
        let model_rules = rule_rows
            .into_iter()
            .map(|row| -> anyhow::Result<GatewayModelRule> {
                let rule =
                    serde_json::from_str::<ModelRateLimitRule>(&row.get::<String, _>("rule_json"))
                        .context("failed to deserialize model rule json")?;
                Ok(GatewayModelRule {
                    provider_id: row.get::<String, _>("provider_id"),
                    rule,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let bundle = GatewayProviderBundle {
            provider,
            keys,
            model_rules,
            daily_reset,
            normalized_rate_limit_config_cache: None,
        };

        Ok(Some(normalize_bundle(bundle)?))
    }

    async fn fetch_all_bundles_shared(&self) -> anyhow::Result<Vec<SharedProviderBundle>> {
        let provider_rows = sqlx::query(
            "SELECT id, name, vendor, base_url, auth_strategy_json, daily_reset_json, enabled
             FROM gateway_providers
             ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to fetch providers for bulk bundle load")?;

        let mut bundles = Vec::with_capacity(provider_rows.len());
        let mut provider_index = std::collections::HashMap::with_capacity(provider_rows.len());

        for provider_row in provider_rows {
            let daily_reset =
                serde_json::from_str(&provider_row.get::<String, _>("daily_reset_json"))
                    .context("failed to deserialize daily_reset_json")?;

            let provider = GatewayProvider {
                id: provider_row.get::<String, _>("id"),
                name: provider_row.get::<String, _>("name"),
                vendor: provider_vendor_from_db(&provider_row.get::<String, _>("vendor"))?,
                base_url: provider_row.get::<String, _>("base_url"),
                auth_strategy: serde_json::from_str(
                    &provider_row.get::<String, _>("auth_strategy_json"),
                )
                .context("failed to deserialize auth_strategy_json")?,
                enabled: provider_row.get::<i64, _>("enabled") != 0,
            };

            provider_index.insert(provider.id.clone(), bundles.len());
            bundles.push(GatewayProviderBundle {
                provider,
                keys: Vec::new(),
                model_rules: Vec::new(),
                daily_reset,
                normalized_rate_limit_config_cache: None,
            });
        }

        let key_rows = sqlx::query(
            "SELECT provider_id, id, display_name, api_key, enabled, weight
             FROM gateway_keys
             ORDER BY provider_id ASC, id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to fetch keys for bulk bundle load")?;
        for row in key_rows {
            let provider_id = row.get::<String, _>("provider_id");
            let Some(&bundle_idx) = provider_index.get(&provider_id) else {
                continue;
            };
            bundles[bundle_idx].keys.push(GatewayKey {
                id: row.get::<String, _>("id"),
                provider_id,
                display_name: row.get::<Option<String>, _>("display_name"),
                api_key: row.get::<String, _>("api_key"),
                enabled: row.get::<i64, _>("enabled") != 0,
                weight: row
                    .get::<Option<i64>, _>("weight")
                    .map(u32::try_from)
                    .transpose()
                    .context("gateway_keys.weight exceeds u32 range")?,
            });
        }

        let rule_rows = sqlx::query(
            "SELECT provider_id, rule_json
             FROM gateway_model_rules
             ORDER BY provider_id ASC, model ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to fetch model rules for bulk bundle load")?;
        for row in rule_rows {
            let provider_id = row.get::<String, _>("provider_id");
            let Some(&bundle_idx) = provider_index.get(&provider_id) else {
                continue;
            };
            let rule =
                serde_json::from_str::<ModelRateLimitRule>(&row.get::<String, _>("rule_json"))
                    .context("failed to deserialize model rule json")?;
            bundles[bundle_idx]
                .model_rules
                .push(GatewayModelRule { provider_id, rule });
        }

        let normalized = bundles
            .into_iter()
            .map(normalize_bundle)
            .collect::<anyhow::Result<Vec<_>>>()?;
        let shared = normalized.into_iter().map(Arc::new).collect::<Vec<_>>();
        self.replace_provider_cache(shared.clone());
        self.cache_complete.store(true, Ordering::Release);
        Ok(shared)
    }
}

#[async_trait]
impl GatewayConfigStore for SqliteGatewayConfigStore {
    async fn get_provider_bundle(
        &self,
        provider_id: &str,
    ) -> anyhow::Result<Option<GatewayProviderBundle>> {
        self.get_cached_or_fetch_bundle(provider_id).await
    }

    async fn get_provider_bundle_shared(
        &self,
        provider_id: &str,
    ) -> anyhow::Result<Option<SharedProviderBundle>> {
        self.get_cached_or_fetch_bundle_shared(provider_id).await
    }

    async fn list_provider_bundles(&self) -> anyhow::Result<Vec<GatewayProviderBundle>> {
        Ok(self
            .fetch_all_bundles_shared()
            .await?
            .into_iter()
            .map(|bundle| bundle.as_ref().clone())
            .collect())
    }

    async fn list_provider_bundles_shared(&self) -> anyhow::Result<Vec<SharedProviderBundle>> {
        if let Some(bundles) = self.cached_bundles_shared() {
            return Ok(bundles);
        }
        self.fetch_all_bundles_shared().await
    }

    async fn list_provider_summaries(&self) -> anyhow::Result<Vec<GatewayProviderSummary>> {
        let rows = sqlx::query(
            "SELECT
                p.id,
                p.name,
                p.vendor,
                p.base_url,
                p.enabled,
                COALESCE(k.key_count, 0) AS key_count,
                COALESCE(k.enabled_key_count, 0) AS enabled_key_count,
                COALESCE(m.model_rule_count, 0) AS model_rule_count,
                COALESCE(m.has_tpm, 0) AS has_tpm,
                COALESCE(m.has_rpm, 0) AS has_rpm,
                COALESCE(m.has_rpd, 0) AS has_rpd
             FROM gateway_providers p
             LEFT JOIN (
                SELECT
                    provider_id,
                    COUNT(*) AS key_count,
                    SUM(CASE WHEN enabled != 0 THEN 1 ELSE 0 END) AS enabled_key_count
                FROM gateway_keys
                GROUP BY provider_id
             ) k ON k.provider_id = p.id
             LEFT JOIN (
                SELECT
                    provider_id,
                    COUNT(*) AS model_rule_count,
                    MAX(CASE WHEN COALESCE(CAST(json_extract(rule_json, '$.tpm') AS INTEGER), 0) > 0 THEN 1 ELSE 0 END) AS has_tpm,
                    MAX(CASE WHEN COALESCE(CAST(json_extract(rule_json, '$.rpm') AS INTEGER), 0) > 0 THEN 1 ELSE 0 END) AS has_rpm,
                    MAX(CASE WHEN COALESCE(CAST(json_extract(rule_json, '$.rpd') AS INTEGER), 0) > 0 THEN 1 ELSE 0 END) AS has_rpd
                FROM gateway_model_rules
                GROUP BY provider_id
             ) m ON m.provider_id = p.id
             ORDER BY p.id ASC",
        )
            .fetch_all(&self.pool)
            .await
            .context("failed to list sqlite providers")?;

        Ok(rows
            .into_iter()
            .map(|row| GatewayProviderSummary {
                id: row.get::<String, _>("id"),
                name: row.get::<String, _>("name"),
                vendor: provider_vendor_from_db(&row.get::<String, _>("vendor"))
                    .expect("sqlite provider vendor should always be valid"),
                enabled: row.get::<i64, _>("enabled") != 0,
                base_url: row.get::<String, _>("base_url"),
                rate_limit: UpstreamRateLimitSummary {
                    key_count: row.get::<i64, _>("key_count") as u32,
                    enabled_key_count: row.get::<i64, _>("enabled_key_count") as u32,
                    model_rule_count: row.get::<i64, _>("model_rule_count") as u32,
                    has_tpm: row.get::<i64, _>("has_tpm") != 0,
                    has_rpm: row.get::<i64, _>("has_rpm") != 0,
                    has_rpd: row.get::<i64, _>("has_rpd") != 0,
                },
            })
            .collect())
    }

    async fn put_provider_bundle(
        &self,
        bundle: GatewayProviderBundle,
    ) -> anyhow::Result<GatewayProviderBundle> {
        let normalized = normalize_bundle(bundle)?;
        let provider_id = normalized.provider.id.clone();

        let mut tx = self
            .pool
            .begin()
            .await
            .context("failed to begin sqlite provider upsert transaction")?;

        sqlx::query("DELETE FROM gateway_providers WHERE id = ?")
            .bind(&provider_id)
            .execute(&mut *tx)
            .await
            .with_context(|| format!("failed to delete provider '{provider_id}' before upsert"))?;

        self.insert_bundle(&mut tx, &normalized).await?;

        tx.commit()
            .await
            .with_context(|| format!("failed to commit provider upsert '{provider_id}'"))?;

        self.cache_bundle(normalized.clone());
        if self.cache_complete.load(Ordering::Acquire) {
            self.cache_complete.store(true, Ordering::Release);
        }
        Ok(normalized)
    }

    async fn delete_provider(&self, provider_id: &str) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM gateway_providers WHERE id = ?")
            .bind(provider_id)
            .execute(&self.pool)
            .await
            .with_context(|| format!("failed to delete provider '{provider_id}'"))?;
        let deleted = result.rows_affected() > 0;
        if deleted {
            self.provider_cache.remove(provider_id);
        }
        Ok(deleted)
    }
}

fn provider_vendor_to_db(vendor: ProviderVendor) -> &'static str {
    match vendor {
        ProviderVendor::OpenAI => "openai",
        ProviderVendor::Anthropic => "anthropic",
        ProviderVendor::Gemini => "gemini",
    }
}

fn provider_vendor_from_db(raw: &str) -> anyhow::Result<ProviderVendor> {
    match raw {
        "openai" => Ok(ProviderVendor::OpenAI),
        "anthropic" => Ok(ProviderVendor::Anthropic),
        "gemini" => Ok(ProviderVendor::Gemini),
        other => anyhow::bail!("unsupported provider vendor in sqlite: {other}"),
    }
}
#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::config::{DailyResetConfig, ModelRateLimitRule, TpmMode};
    use crate::provider::{
        GatewayAuthStrategy, GatewayKey, GatewayModelRule, GatewayProvider, GatewayProviderBundle,
        ProviderVendor,
    };
    use crate::storage::GatewayConfigStore;

    use super::SqliteGatewayConfigStore;

    #[tokio::test]
    async fn sqlite_store_round_trips_provider_bundle() {
        let store = SqliteGatewayConfigStore::connect(&temp_db_url("roundtrip"))
            .await
            .unwrap();
        let bundle = sample_bundle();

        store
            .replace_all_provider_bundles(std::slice::from_ref(&bundle))
            .await
            .unwrap();

        let loaded = store
            .get_provider_bundle("gemini-prod")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(loaded.provider.id, bundle.provider.id);
        assert_eq!(loaded.keys.len(), 1);
        assert_eq!(
            loaded.model_rules[0].rule.tpm_mode,
            Some(TpmMode::InputOnly)
        );
        assert!(loaded.normalized_rate_limit_config_cache.is_some());
    }

    #[tokio::test]
    async fn sqlite_seed_if_empty_does_not_overwrite_existing_rows() {
        let store = SqliteGatewayConfigStore::connect(&temp_db_url("seed-once"))
            .await
            .unwrap();
        let bundle = sample_bundle();

        let seeded = store
            .seed_if_empty(std::slice::from_ref(&bundle))
            .await
            .unwrap();
        assert_eq!(seeded, 1);

        let skipped = store
            .seed_if_empty(std::slice::from_ref(&bundle))
            .await
            .unwrap();
        assert_eq!(skipped, 0);
    }

    #[tokio::test]
    async fn sqlite_store_populates_cache_on_first_read_after_restart() {
        let db_url = temp_db_url("cache-read");
        let writer = SqliteGatewayConfigStore::connect(&db_url).await.unwrap();
        let bundle = sample_bundle();

        writer
            .replace_all_provider_bundles(std::slice::from_ref(&bundle))
            .await
            .unwrap();

        let store = SqliteGatewayConfigStore::connect(&db_url).await.unwrap();
        assert_eq!(store.provider_cache.len(), 0);

        let loaded = store
            .get_provider_bundle("gemini-prod")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(loaded.provider.name, "Gemini Prod");
        assert_eq!(store.provider_cache.len(), 1);
    }

    #[tokio::test]
    async fn sqlite_list_provider_summaries_uses_aggregated_metadata() {
        let store = SqliteGatewayConfigStore::connect(&temp_db_url("summary-agg"))
            .await
            .unwrap();
        let mut bundle = sample_bundle();
        bundle.keys.push(GatewayKey {
            id: "key-b".to_string(),
            provider_id: "gemini-prod".to_string(),
            display_name: Some("Key B".to_string()),
            api_key: "gem-test-key-b".to_string(),
            enabled: false,
            weight: Some(2),
        });
        bundle.model_rules.push(GatewayModelRule {
            provider_id: "gemini-prod".to_string(),
            rule: ModelRateLimitRule {
                model: "gemini-2.5-flash".to_string(),
                rpm: Some(10),
                rpd: Some(100),
                tpm: None,
                tpm_mode: None,
                tokenizer_encoding: None,
                tokenizer_model: Some("gemini-2.5-flash".to_string()),
            },
        });

        store
            .replace_all_provider_bundles(std::slice::from_ref(&bundle))
            .await
            .unwrap();

        let summaries = store.list_provider_summaries().await.unwrap();

        assert_eq!(summaries.len(), 1);
        let summary = &summaries[0];
        assert_eq!(summary.id, "gemini-prod");
        assert_eq!(summary.rate_limit.key_count, 2);
        assert_eq!(summary.rate_limit.enabled_key_count, 1);
        assert_eq!(summary.rate_limit.model_rule_count, 2);
        assert!(summary.rate_limit.has_tpm);
        assert!(summary.rate_limit.has_rpm);
        assert!(summary.rate_limit.has_rpd);
    }

    #[tokio::test]
    async fn sqlite_list_provider_bundles_bulk_loads_and_refreshes_cache() {
        let store = SqliteGatewayConfigStore::connect(&temp_db_url("bundle-list"))
            .await
            .unwrap();
        let mut second = sample_bundle();
        second.provider.id = "gemini-dr".to_string();
        second.provider.name = "Gemini DR".to_string();
        second.keys[0].id = "key-z".to_string();
        second.keys[0].provider_id = "gemini-dr".to_string();
        second.model_rules[0].provider_id = "gemini-dr".to_string();

        store
            .replace_all_provider_bundles(&[sample_bundle(), second])
            .await
            .unwrap();
        store.provider_cache.clear();

        let bundles = store.list_provider_bundles().await.unwrap();

        assert_eq!(bundles.len(), 2);
        assert_eq!(bundles[0].provider.id, "gemini-dr");
        assert_eq!(bundles[1].provider.id, "gemini-prod");
        assert_eq!(store.provider_cache.len(), 2);
    }

    #[tokio::test]
    async fn sqlite_put_provider_refreshes_cached_bundle() {
        let store = SqliteGatewayConfigStore::connect(&temp_db_url("cache-put"))
            .await
            .unwrap();
        let bundle = sample_bundle();

        store
            .replace_all_provider_bundles(std::slice::from_ref(&bundle))
            .await
            .unwrap();

        let _ = store.get_provider_bundle("gemini-prod").await.unwrap();

        let mut updated = sample_bundle();
        updated.provider.name = "Gemini Prod Updated".to_string();
        updated.keys.push(GatewayKey {
            id: "key-b".to_string(),
            provider_id: "gemini-prod".to_string(),
            display_name: Some("Key B".to_string()),
            api_key: "gem-test-key-b".to_string(),
            enabled: false,
            weight: Some(3),
        });

        store.put_provider_bundle(updated).await.unwrap();

        let cached = store.provider_cache.get("gemini-prod").unwrap();
        assert_eq!(cached.provider.name, "Gemini Prod Updated");
        assert_eq!(cached.keys.len(), 2);
    }

    #[tokio::test]
    async fn sqlite_delete_provider_invalidates_cached_bundle() {
        let store = SqliteGatewayConfigStore::connect(&temp_db_url("cache-delete"))
            .await
            .unwrap();
        let bundle = sample_bundle();

        store
            .replace_all_provider_bundles(std::slice::from_ref(&bundle))
            .await
            .unwrap();

        let _ = store.get_provider_bundle("gemini-prod").await.unwrap();
        assert_eq!(store.provider_cache.len(), 1);

        let deleted = store.delete_provider("gemini-prod").await.unwrap();

        assert!(deleted);
        assert_eq!(store.provider_cache.len(), 0);
        assert!(
            store
                .get_provider_bundle("gemini-prod")
                .await
                .unwrap()
                .is_none()
        );
    }

    fn temp_db_url(prefix: &str) -> String {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{nonce}.sqlite"));
        let normalized = path.to_string_lossy().replace('\\', "/");
        format!("sqlite://{normalized}")
    }

    fn sample_bundle() -> GatewayProviderBundle {
        GatewayProviderBundle {
            provider: GatewayProvider {
                id: "gemini-prod".to_string(),
                name: "Gemini Prod".to_string(),
                vendor: ProviderVendor::Gemini,
                base_url: "https://generativelanguage.googleapis.com".to_string(),
                auth_strategy: GatewayAuthStrategy::QueryApiKey {
                    parameter_name: "key".to_string(),
                },
                enabled: true,
            },
            keys: vec![GatewayKey {
                id: "key-a".to_string(),
                provider_id: "gemini-prod".to_string(),
                display_name: Some("Key A".to_string()),
                api_key: "gem-test-key-a".to_string(),
                enabled: true,
                weight: Some(1),
            }],
            model_rules: vec![GatewayModelRule {
                provider_id: "gemini-prod".to_string(),
                rule: ModelRateLimitRule {
                    model: "*".to_string(),
                    rpm: Some(60),
                    rpd: Some(1000),
                    tpm: Some(20000),
                    tpm_mode: Some(TpmMode::InputOnly),
                    tokenizer_encoding: None,
                    tokenizer_model: Some("gemini-2.5-pro".to_string()),
                },
            }],
            daily_reset: DailyResetConfig {
                timezone: "+08:00".to_string(),
                hour: 4,
                minute: 0,
            },
            normalized_rate_limit_config_cache: None,
        }
    }
}
