use std::collections::BTreeMap;
use std::str::FromStr;

use anyhow::Context;
use async_trait::async_trait;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool, Transaction};

use crate::domain::{
    BootstrapData, ProtocolFamily, RaceGroup, RaceGroupSummary, RaceKey, RaceKeyPool,
    RaceKeyPoolSummary, RaceModelDescriptor, RaceModelSummary, RaceSettings, RaceTargetEndpoint,
};

use super::{RaceConfigStore, normalize_bootstrap_data};

#[derive(Debug, Clone)]
pub struct SqliteRaceConfigStore {
    pool: SqlitePool,
}

impl SqliteRaceConfigStore {
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

        let store = Self { pool };
        store.initialize_schema().await?;
        Ok(store)
    }

    pub async fn seed_if_empty(&self, data: BootstrapData) -> anyhow::Result<bool> {
        let existing_models = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM race_models")
            .fetch_one(&self.pool)
            .await
            .context("failed to count race_models")?;
        if existing_models > 0 {
            return Ok(false);
        }

        self.replace_all(normalize_bootstrap_data(data)).await?;
        Ok(true)
    }

    pub async fn replace_all(&self, data: BootstrapData) -> anyhow::Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .context("failed to begin sqlite transaction")?;

        sqlx::query("DELETE FROM race_group_candidates")
            .execute(&mut *tx)
            .await
            .context("failed to clear race_group_candidates")?;
        sqlx::query("DELETE FROM race_groups")
            .execute(&mut *tx)
            .await
            .context("failed to clear race_groups")?;
        sqlx::query("DELETE FROM race_model_endpoints")
            .execute(&mut *tx)
            .await
            .context("failed to clear race_model_endpoints")?;
        sqlx::query("DELETE FROM race_models")
            .execute(&mut *tx)
            .await
            .context("failed to clear race_models")?;
        sqlx::query("DELETE FROM race_keys")
            .execute(&mut *tx)
            .await
            .context("failed to clear race_keys")?;
        sqlx::query("DELETE FROM race_key_pools")
            .execute(&mut *tx)
            .await
            .context("failed to clear race_key_pools")?;
        sqlx::query("DELETE FROM race_settings")
            .execute(&mut *tx)
            .await
            .context("failed to clear race_settings")?;

        for pool in data.key_pools {
            self.insert_key_pool(&mut tx, &pool).await?;
        }
        for model in data.models {
            self.insert_model(&mut tx, &model).await?;
        }
        for group in data.groups {
            self.insert_group(&mut tx, &group).await?;
        }
        self.upsert_settings_tx(&mut tx, data.settings.unwrap_or_default())
            .await?;

        tx.commit()
            .await
            .context("failed to commit sqlite bootstrap transaction")?;
        Ok(())
    }

    async fn initialize_schema(&self) -> anyhow::Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS race_models (
                id TEXT PRIMARY KEY,
                display_name TEXT NOT NULL,
                upstream_model TEXT NOT NULL,
                description TEXT NOT NULL,
                enabled INTEGER NOT NULL,
                metadata_json TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await
        .context("failed to create race_models")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS race_model_endpoints (
                model_id TEXT NOT NULL,
                protocol_family TEXT NOT NULL,
                base_url TEXT NOT NULL,
                auth_strategy_json TEXT NOT NULL,
                key_pool_id TEXT NOT NULL,
                request_timeout_ms INTEGER NULL,
                extra_headers_json TEXT NOT NULL,
                extra_query_json TEXT NOT NULL,
                enabled INTEGER NOT NULL,
                PRIMARY KEY (model_id, protocol_family),
                FOREIGN KEY(model_id) REFERENCES race_models(id) ON DELETE CASCADE,
                FOREIGN KEY(key_pool_id) REFERENCES race_key_pools(id)
            )",
        )
        .execute(&self.pool)
        .await
        .context("failed to create race_model_endpoints")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS race_groups (
                id TEXT PRIMARY KEY,
                display_name TEXT NOT NULL,
                fallback_ratio REAL NOT NULL,
                decay_factor REAL NOT NULL,
                penalty_rate REAL NOT NULL,
                recovery_rate REAL NOT NULL,
                race_max_wait_time_ms INTEGER NULL,
                enabled INTEGER NOT NULL
            )",
        )
        .execute(&self.pool)
        .await
        .context("failed to create race_groups")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS race_group_candidates (
                id TEXT PRIMARY KEY,
                group_id TEXT NOT NULL,
                name TEXT NOT NULL,
                candidate_order INTEGER NOT NULL,
                model_id TEXT NULL,
                upstream_model TEXT NOT NULL,
                inline_endpoints_json TEXT NOT NULL,
                initial_weight REAL NOT NULL,
                response_protection_timeout_ms INTEGER NOT NULL,
                enabled INTEGER NOT NULL,
                metadata_json TEXT NOT NULL,
                UNIQUE(group_id, name),
                FOREIGN KEY(group_id) REFERENCES race_groups(id) ON DELETE CASCADE,
                FOREIGN KEY(model_id) REFERENCES race_models(id)
            )",
        )
        .execute(&self.pool)
        .await
        .context("failed to create race_group_candidates")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS race_key_pools (
                id TEXT PRIMARY KEY,
                display_name TEXT NOT NULL,
                auth_strategy_json TEXT NOT NULL,
                selection_strategy TEXT NOT NULL,
                enabled INTEGER NOT NULL
            )",
        )
        .execute(&self.pool)
        .await
        .context("failed to create race_key_pools")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS race_keys (
                id TEXT PRIMARY KEY,
                key_pool_id TEXT NOT NULL,
                secret TEXT NOT NULL,
                enabled INTEGER NOT NULL,
                metadata_json TEXT NOT NULL,
                FOREIGN KEY(key_pool_id) REFERENCES race_key_pools(id) ON DELETE CASCADE
            )",
        )
        .execute(&self.pool)
        .await
        .context("failed to create race_keys")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS race_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await
        .context("failed to create race_settings")?;

        Ok(())
    }

    async fn insert_model(
        &self,
        tx: &mut Transaction<'_, sqlx::Sqlite>,
        model: &RaceModelDescriptor,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO race_models
                (id, display_name, upstream_model, description, enabled, metadata_json)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&model.id)
        .bind(&model.display_name)
        .bind(&model.upstream_model)
        .bind(&model.description)
        .bind(i64::from(model.enabled))
        .bind(serde_json::to_string(&model.metadata).context("serialize model metadata")?)
        .execute(&mut **tx)
        .await
        .with_context(|| format!("failed to insert model '{}'", model.id))?;

        for endpoint in &model.endpoints {
            self.insert_model_endpoint(tx, &model.id, endpoint).await?;
        }

        Ok(())
    }

    async fn insert_model_endpoint(
        &self,
        tx: &mut Transaction<'_, sqlx::Sqlite>,
        model_id: &str,
        endpoint: &RaceTargetEndpoint,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO race_model_endpoints
                (model_id, protocol_family, base_url, auth_strategy_json, key_pool_id, request_timeout_ms, extra_headers_json, extra_query_json, enabled)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(model_id)
        .bind(serde_json::to_string(&endpoint.protocol_family).context("serialize protocol_family")?)
        .bind(&endpoint.base_url)
        .bind(serde_json::to_string(&endpoint.auth_strategy).context("serialize auth_strategy")?)
        .bind(&endpoint.key_pool_id)
        .bind(endpoint.request_timeout_ms.map(|value| value as i64))
        .bind(serde_json::to_string(&endpoint.extra_headers).context("serialize extra_headers")?)
        .bind(serde_json::to_string(&endpoint.extra_query).context("serialize extra_query")?)
        .bind(i64::from(endpoint.enabled))
        .execute(&mut **tx)
        .await
        .with_context(|| format!("failed to insert model endpoint for '{model_id}'"))?;
        Ok(())
    }

    async fn insert_group(
        &self,
        tx: &mut Transaction<'_, sqlx::Sqlite>,
        group: &RaceGroup,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO race_groups
                (id, display_name, fallback_ratio, decay_factor, penalty_rate, recovery_rate, race_max_wait_time_ms, enabled)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&group.id)
        .bind(&group.display_name)
        .bind(group.fallback_ratio)
        .bind(group.decay_factor)
        .bind(group.penalty_rate)
        .bind(group.recovery_rate)
        .bind(group.race_max_wait_time_ms.map(|value| value as i64))
        .bind(i64::from(group.enabled))
        .execute(&mut **tx)
        .await
        .with_context(|| format!("failed to insert group '{}'", group.id))?;

        for (candidate_order, candidate) in group.candidates.iter().enumerate() {
            sqlx::query(
                "INSERT INTO race_group_candidates
                    (id, group_id, name, candidate_order, model_id, upstream_model, inline_endpoints_json, initial_weight, response_protection_timeout_ms, enabled, metadata_json)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&candidate.id)
            .bind(&group.id)
            .bind(&candidate.name)
            .bind(candidate_order as i64)
            .bind(&candidate.model_id)
            .bind(&candidate.upstream_model)
            .bind(
                serde_json::to_string(&candidate.inline_endpoint_overrides)
                    .context("serialize candidate inline_endpoints_json")?,
            )
            .bind(candidate.initial_weight)
            .bind(candidate.response_protection_timeout_ms as i64)
            .bind(i64::from(candidate.enabled))
            .bind(serde_json::to_string(&candidate.metadata).context("serialize candidate metadata")?)
            .execute(&mut **tx)
            .await
            .with_context(|| format!("failed to insert candidate '{}'", candidate.id))?;
        }

        Ok(())
    }

    async fn insert_key_pool(
        &self,
        tx: &mut Transaction<'_, sqlx::Sqlite>,
        pool: &RaceKeyPool,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO race_key_pools
                (id, display_name, auth_strategy_json, selection_strategy, enabled)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&pool.id)
        .bind(&pool.display_name)
        .bind(serde_json::to_string(&pool.auth_strategy).context("serialize pool auth_strategy")?)
        .bind(
            serde_json::to_string(&pool.selection_strategy)
                .context("serialize selection_strategy")?,
        )
        .bind(i64::from(pool.enabled))
        .execute(&mut **tx)
        .await
        .with_context(|| format!("failed to insert key pool '{}'", pool.id))?;

        for key in &pool.keys {
            self.insert_key(tx, key).await?;
        }

        Ok(())
    }

    async fn insert_key(
        &self,
        tx: &mut Transaction<'_, sqlx::Sqlite>,
        key: &RaceKey,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO race_keys
                (id, key_pool_id, secret, enabled, metadata_json)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&key.id)
        .bind(&key.key_pool_id)
        .bind(&key.secret)
        .bind(i64::from(key.enabled))
        .bind(serde_json::to_string(&key.metadata).context("serialize key metadata")?)
        .execute(&mut **tx)
        .await
        .with_context(|| format!("failed to insert key '{}'", key.id))?;
        Ok(())
    }

    async fn upsert_settings_tx(
        &self,
        tx: &mut Transaction<'_, sqlx::Sqlite>,
        settings: RaceSettings,
    ) -> anyhow::Result<()> {
        let settings_json =
            serde_json::to_string(&settings.normalized()).context("serialize settings")?;
        sqlx::query(
            "INSERT INTO race_settings (key, value)
             VALUES ('global', ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(settings_json)
        .execute(&mut **tx)
        .await
        .context("failed to upsert race_settings")?;
        Ok(())
    }

    async fn rebind_group_candidate_model_references(
        &self,
        tx: &mut Transaction<'_, sqlx::Sqlite>,
        previous_model_id: &str,
        new_model_id: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE race_group_candidates
             SET model_id = ?
             WHERE model_id = ?",
        )
        .bind(new_model_id)
        .bind(previous_model_id)
        .execute(&mut **tx)
        .await
        .with_context(|| {
            format!(
                "failed to rebind candidate model references from '{previous_model_id}' to '{new_model_id}'"
            )
        })?;
        Ok(())
    }

    async fn rebind_key_pool_references(
        &self,
        tx: &mut Transaction<'_, sqlx::Sqlite>,
        previous_key_pool_id: &str,
        new_key_pool_id: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE race_model_endpoints
             SET key_pool_id = ?
             WHERE key_pool_id = ?",
        )
        .bind(new_key_pool_id)
        .bind(previous_key_pool_id)
        .execute(&mut **tx)
        .await
        .with_context(|| {
            format!(
                "failed to rebind model endpoint key pool references from '{previous_key_pool_id}' to '{new_key_pool_id}'"
            )
        })?;

        let candidate_rows = sqlx::query(
            "SELECT id, inline_endpoints_json
             FROM race_group_candidates
             WHERE inline_endpoints_json LIKE ?",
        )
        .bind(format!("%{previous_key_pool_id}%"))
        .fetch_all(&mut **tx)
        .await
        .with_context(|| {
            format!(
                "failed to load candidate inline endpoints referencing key pool '{previous_key_pool_id}'"
            )
        })?;

        for row in candidate_rows {
            let candidate_id = row.get::<String, _>("id");
            let raw = row.get::<String, _>("inline_endpoints_json");
            let mut endpoints = serde_json::from_str::<Vec<RaceTargetEndpoint>>(&raw)
                .context("deserialize candidate inline_endpoints_json")?;
            let mut changed = false;
            for endpoint in &mut endpoints {
                if endpoint.key_pool_id == previous_key_pool_id {
                    endpoint.key_pool_id = new_key_pool_id.to_string();
                    changed = true;
                }
            }
            if !changed {
                continue;
            }
            sqlx::query(
                "UPDATE race_group_candidates
                 SET inline_endpoints_json = ?
                 WHERE id = ?",
            )
            .bind(
                serde_json::to_string(&endpoints)
                    .context("serialize candidate inline_endpoints_json")?,
            )
            .bind(&candidate_id)
            .execute(&mut **tx)
            .await
            .with_context(|| {
                format!(
                    "failed to update candidate '{candidate_id}' inline endpoint key pool references"
                )
            })?;
        }

        Ok(())
    }

    async fn fetch_model_endpoints(
        &self,
        model_id: &str,
    ) -> anyhow::Result<Vec<RaceTargetEndpoint>> {
        let rows = sqlx::query(
            "SELECT protocol_family, base_url, auth_strategy_json, key_pool_id, request_timeout_ms, extra_headers_json, extra_query_json, enabled
             FROM race_model_endpoints
             WHERE model_id = ?
             ORDER BY protocol_family ASC",
        )
        .bind(model_id)
        .fetch_all(&self.pool)
        .await
        .with_context(|| format!("failed to fetch endpoints for model '{model_id}'"))?;

        rows.into_iter().map(decode_endpoint_row).collect()
    }

    async fn fetch_candidate_groups(
        &self,
        group_id: &str,
    ) -> anyhow::Result<Vec<crate::domain::RaceCandidate>> {
        let rows = sqlx::query(
            "SELECT id, group_id, name, model_id, upstream_model, inline_endpoints_json, initial_weight, response_protection_timeout_ms, enabled, metadata_json
             FROM race_group_candidates
             WHERE group_id = ?
             ORDER BY candidate_order ASC",
        )
        .bind(group_id)
        .fetch_all(&self.pool)
        .await
        .with_context(|| format!("failed to fetch candidates for group '{group_id}'"))?;

        rows.into_iter()
            .map(|row| {
                Ok(crate::domain::RaceCandidate {
                    id: row.get("id"),
                    group_id: row.get("group_id"),
                    name: row.get("name"),
                    model_id: row.get::<Option<String>, _>("model_id"),
                    upstream_model: row.get("upstream_model"),
                    inline_endpoint_overrides: serde_json::from_str(
                        &row.get::<String, _>("inline_endpoints_json"),
                    )
                    .context("deserialize inline_endpoints_json")?,
                    initial_weight: row.get("initial_weight"),
                    response_protection_timeout_ms: row
                        .get::<i64, _>("response_protection_timeout_ms")
                        as u64,
                    enabled: row.get::<i64, _>("enabled") != 0,
                    metadata: serde_json::from_str(&row.get::<String, _>("metadata_json"))
                        .context("deserialize candidate metadata_json")?,
                })
            })
            .collect()
    }

    async fn summarize_group(&self, group: RaceGroup) -> anyhow::Result<RaceGroupSummary> {
        let mut protocol_families = group
            .candidates
            .iter()
            .flat_map(|candidate| {
                candidate
                    .inline_endpoint_overrides
                    .iter()
                    .filter(|endpoint| endpoint.enabled)
                    .map(|endpoint| endpoint.protocol_family)
            })
            .collect::<std::collections::BTreeSet<ProtocolFamily>>();

        for candidate in &group.candidates {
            let model_ids = if let Some(model_id) = candidate.model_id.as_deref() {
                vec![model_id.to_string()]
            } else if candidate.upstream_model.trim().is_empty() {
                Vec::new()
            } else {
                self.fetch_model_ids_by_upstream_model(candidate.upstream_model.trim())
                    .await?
            };

            for model_id in model_ids {
                for endpoint in self.fetch_model_endpoints(&model_id).await? {
                    if endpoint.enabled {
                        protocol_families.insert(endpoint.protocol_family);
                    }
                }
            }
        }

        Ok(RaceGroupSummary {
            id: group.id.clone(),
            display_name: group.display_name.clone(),
            enabled: group.enabled,
            candidate_count: group.candidates.len(),
            enabled_candidate_count: group
                .candidates
                .iter()
                .filter(|candidate| candidate.enabled)
                .count(),
            protocol_families: protocol_families.into_iter().collect(),
            candidate_names: group
                .candidates
                .iter()
                .map(|candidate| candidate.name.clone())
                .collect(),
        })
    }

    async fn fetch_model_ids_by_upstream_model(
        &self,
        upstream_model: &str,
    ) -> anyhow::Result<Vec<String>> {
        let rows = sqlx::query(
            "SELECT id
             FROM race_models
             WHERE upstream_model = ?
             ORDER BY id ASC",
        )
        .bind(upstream_model)
        .fetch_all(&self.pool)
        .await
        .with_context(|| format!("failed to fetch models by upstream model '{upstream_model}'"))?;

        Ok(rows.into_iter().map(|row| row.get("id")).collect())
    }
}

#[async_trait]
impl RaceConfigStore for SqliteRaceConfigStore {
    async fn load_models_full(&self) -> anyhow::Result<Vec<RaceModelDescriptor>> {
        let rows = sqlx::query(
            "SELECT id, display_name, upstream_model, description, enabled, metadata_json
             FROM race_models
             ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to load race_models")?;

        let endpoint_rows = sqlx::query(
            "SELECT model_id, protocol_family, base_url, auth_strategy_json, key_pool_id, request_timeout_ms, extra_headers_json, extra_query_json, enabled
             FROM race_model_endpoints
             ORDER BY model_id ASC, protocol_family ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to load race_model_endpoints")?;

        let mut endpoints_by_model = BTreeMap::<String, Vec<RaceTargetEndpoint>>::new();
        for row in endpoint_rows {
            endpoints_by_model
                .entry(row.get::<String, _>("model_id"))
                .or_default()
                .push(decode_endpoint_row(row)?);
        }

        rows.into_iter()
            .map(|row| {
                let id = row.get::<String, _>("id");
                Ok(RaceModelDescriptor {
                    endpoints: endpoints_by_model.remove(&id).unwrap_or_default(),
                    id,
                    display_name: row.get("display_name"),
                    upstream_model: row.get("upstream_model"),
                    description: row.get("description"),
                    enabled: row.get::<i64, _>("enabled") != 0,
                    metadata: serde_json::from_str(&row.get::<String, _>("metadata_json"))
                        .context("deserialize model metadata_json")?,
                })
            })
            .collect()
    }

    async fn list_models(&self) -> anyhow::Result<Vec<RaceModelSummary>> {
        let rows = sqlx::query(
            "SELECT id, display_name, upstream_model, enabled
             FROM race_models
             ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to list race_models")?;

        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let id = row.get::<String, _>("id");
            let endpoints = self.fetch_model_endpoints(&id).await?;
            let model = RaceModelDescriptor {
                id,
                display_name: row.get("display_name"),
                upstream_model: row.get("upstream_model"),
                description: String::new(),
                enabled: row.get::<i64, _>("enabled") != 0,
                endpoints,
                metadata: serde_json::Value::Object(Default::default()),
            };
            items.push(model.summary());
        }
        Ok(items)
    }

    async fn get_model(&self, model_id: &str) -> anyhow::Result<Option<RaceModelDescriptor>> {
        let Some(row) = sqlx::query(
            "SELECT id, display_name, upstream_model, description, enabled, metadata_json
             FROM race_models
             WHERE id = ?",
        )
        .bind(model_id)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("failed to fetch model '{model_id}'"))?
        else {
            return Ok(None);
        };

        Ok(Some(RaceModelDescriptor {
            id: row.get("id"),
            display_name: row.get("display_name"),
            upstream_model: row.get("upstream_model"),
            description: row.get("description"),
            enabled: row.get::<i64, _>("enabled") != 0,
            endpoints: self.fetch_model_endpoints(model_id).await?,
            metadata: serde_json::from_str(&row.get::<String, _>("metadata_json"))
                .context("deserialize model metadata_json")?,
        }))
    }

    async fn put_model(
        &self,
        previous_model_id: Option<&str>,
        model: RaceModelDescriptor,
    ) -> anyhow::Result<RaceModelDescriptor> {
        let mut tx = self
            .pool
            .begin()
            .await
            .context("failed to begin put_model transaction")?;

        sqlx::query("DELETE FROM race_model_endpoints WHERE model_id = ?")
            .bind(&model.id)
            .execute(&mut *tx)
            .await
            .with_context(|| format!("failed to clear endpoints for model '{}'", model.id))?;

        sqlx::query(
            "INSERT INTO race_models (id, display_name, upstream_model, description, enabled, metadata_json)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                display_name = excluded.display_name,
                upstream_model = excluded.upstream_model,
                description = excluded.description,
                enabled = excluded.enabled,
                metadata_json = excluded.metadata_json",
        )
        .bind(&model.id)
        .bind(&model.display_name)
        .bind(&model.upstream_model)
        .bind(&model.description)
        .bind(i64::from(model.enabled))
        .bind(serde_json::to_string(&model.metadata).context("serialize model metadata")?)
        .execute(&mut *tx)
        .await
        .with_context(|| format!("failed to upsert model '{}'", model.id))?;

        for endpoint in &model.endpoints {
            self.insert_model_endpoint(&mut tx, &model.id, endpoint)
                .await?;
        }

        if let Some(previous_model_id) = previous_model_id
            && previous_model_id != model.id
        {
            self.rebind_group_candidate_model_references(&mut tx, previous_model_id, &model.id)
                .await?;
            sqlx::query("DELETE FROM race_models WHERE id = ?")
                .bind(previous_model_id)
                .execute(&mut *tx)
                .await
                .with_context(|| {
                    format!("failed to delete previous model '{previous_model_id}'")
                })?;
        }

        tx.commit()
            .await
            .context("failed to commit put_model transaction")?;
        Ok(model)
    }

    async fn delete_model(&self, model_id: &str) -> anyhow::Result<bool> {
        Ok(sqlx::query("DELETE FROM race_models WHERE id = ?")
            .bind(model_id)
            .execute(&self.pool)
            .await
            .with_context(|| format!("failed to delete model '{model_id}'"))?
            .rows_affected()
            > 0)
    }

    async fn load_groups_full(&self) -> anyhow::Result<Vec<RaceGroup>> {
        let rows = sqlx::query(
            "SELECT id, display_name, fallback_ratio, decay_factor, penalty_rate, recovery_rate, race_max_wait_time_ms, enabled
             FROM race_groups
             ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to load race_groups")?;

        let candidate_rows = sqlx::query(
            "SELECT id, group_id, name, model_id, upstream_model, inline_endpoints_json, initial_weight, response_protection_timeout_ms, enabled, metadata_json
             FROM race_group_candidates
             ORDER BY group_id ASC, candidate_order ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to load race_group_candidates")?;

        let mut candidates_by_group = BTreeMap::<String, Vec<crate::domain::RaceCandidate>>::new();
        for row in candidate_rows {
            let group_id = row.get::<String, _>("group_id");
            candidates_by_group
                .entry(group_id.clone())
                .or_default()
                .push(crate::domain::RaceCandidate {
                    id: row.get("id"),
                    group_id,
                    name: row.get("name"),
                    model_id: row.get::<Option<String>, _>("model_id"),
                    upstream_model: row.get("upstream_model"),
                    inline_endpoint_overrides: serde_json::from_str(
                        &row.get::<String, _>("inline_endpoints_json"),
                    )
                    .context("deserialize inline_endpoints_json")?,
                    initial_weight: row.get("initial_weight"),
                    response_protection_timeout_ms: row
                        .get::<i64, _>("response_protection_timeout_ms")
                        as u64,
                    enabled: row.get::<i64, _>("enabled") != 0,
                    metadata: serde_json::from_str(&row.get::<String, _>("metadata_json"))
                        .context("deserialize candidate metadata_json")?,
                });
        }

        rows.into_iter()
            .map(|row| {
                let id = row.get::<String, _>("id");
                Ok(RaceGroup {
                    candidates: candidates_by_group.remove(&id).unwrap_or_default(),
                    id,
                    display_name: row.get("display_name"),
                    fallback_ratio: row.get("fallback_ratio"),
                    decay_factor: row.get("decay_factor"),
                    penalty_rate: row.get("penalty_rate"),
                    recovery_rate: row.get("recovery_rate"),
                    race_max_wait_time_ms: row
                        .get::<Option<i64>, _>("race_max_wait_time_ms")
                        .map(|value| value as u64),
                    enabled: row.get::<i64, _>("enabled") != 0,
                })
            })
            .collect()
    }

    async fn list_groups(&self) -> anyhow::Result<Vec<RaceGroupSummary>> {
        let rows = sqlx::query(
            "SELECT id
             FROM race_groups
             ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to list race_groups")?;

        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            if let Some(group) = self.get_group(&row.get::<String, _>("id")).await? {
                items.push(self.summarize_group(group).await?);
            }
        }
        Ok(items)
    }

    async fn get_group(&self, group_id: &str) -> anyhow::Result<Option<RaceGroup>> {
        let Some(row) = sqlx::query(
            "SELECT id, display_name, fallback_ratio, decay_factor, penalty_rate, recovery_rate, race_max_wait_time_ms, enabled
             FROM race_groups
             WHERE id = ?",
        )
        .bind(group_id)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("failed to fetch group '{group_id}'"))?
        else {
            return Ok(None);
        };

        Ok(Some(RaceGroup {
            id: row.get("id"),
            display_name: row.get("display_name"),
            fallback_ratio: row.get("fallback_ratio"),
            decay_factor: row.get("decay_factor"),
            penalty_rate: row.get("penalty_rate"),
            recovery_rate: row.get("recovery_rate"),
            race_max_wait_time_ms: row
                .get::<Option<i64>, _>("race_max_wait_time_ms")
                .map(|value| value as u64),
            enabled: row.get::<i64, _>("enabled") != 0,
            candidates: self.fetch_candidate_groups(group_id).await?,
        }))
    }

    async fn put_group(
        &self,
        previous_group_id: Option<&str>,
        group: RaceGroup,
    ) -> anyhow::Result<RaceGroup> {
        let mut tx = self
            .pool
            .begin()
            .await
            .context("failed to begin put_group transaction")?;

        if let Some(previous_group_id) = previous_group_id
            && previous_group_id != group.id
        {
            sqlx::query("DELETE FROM race_groups WHERE id = ?")
                .bind(previous_group_id)
                .execute(&mut *tx)
                .await
                .with_context(|| {
                    format!("failed to delete previous group '{previous_group_id}'")
                })?;
        }

        sqlx::query(
            "INSERT INTO race_groups (id, display_name, fallback_ratio, decay_factor, penalty_rate, recovery_rate, race_max_wait_time_ms, enabled)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                display_name = excluded.display_name,
                fallback_ratio = excluded.fallback_ratio,
                decay_factor = excluded.decay_factor,
                penalty_rate = excluded.penalty_rate,
                recovery_rate = excluded.recovery_rate,
                race_max_wait_time_ms = excluded.race_max_wait_time_ms,
                enabled = excluded.enabled",
        )
        .bind(&group.id)
        .bind(&group.display_name)
        .bind(group.fallback_ratio)
        .bind(group.decay_factor)
        .bind(group.penalty_rate)
        .bind(group.recovery_rate)
        .bind(group.race_max_wait_time_ms.map(|value| value as i64))
        .bind(i64::from(group.enabled))
        .execute(&mut *tx)
        .await
        .with_context(|| format!("failed to upsert group '{}'", group.id))?;

        sqlx::query("DELETE FROM race_group_candidates WHERE group_id = ?")
            .bind(&group.id)
            .execute(&mut *tx)
            .await
            .with_context(|| format!("failed to clear candidates for group '{}'", group.id))?;

        for (candidate_order, candidate) in group.candidates.iter().enumerate() {
            sqlx::query(
                "INSERT INTO race_group_candidates
                    (id, group_id, name, candidate_order, model_id, upstream_model, inline_endpoints_json, initial_weight, response_protection_timeout_ms, enabled, metadata_json)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&candidate.id)
            .bind(&group.id)
            .bind(&candidate.name)
            .bind(candidate_order as i64)
            .bind(&candidate.model_id)
            .bind(&candidate.upstream_model)
            .bind(
                serde_json::to_string(&candidate.inline_endpoint_overrides)
                    .context("serialize candidate inline_endpoints_json")?,
            )
            .bind(candidate.initial_weight)
            .bind(candidate.response_protection_timeout_ms as i64)
            .bind(i64::from(candidate.enabled))
            .bind(serde_json::to_string(&candidate.metadata).context("serialize candidate metadata")?)
            .execute(&mut *tx)
            .await
            .with_context(|| format!("failed to upsert candidate '{}'", candidate.id))?;
        }

        tx.commit()
            .await
            .context("failed to commit put_group transaction")?;
        Ok(group)
    }

    async fn delete_group(&self, group_id: &str) -> anyhow::Result<bool> {
        Ok(sqlx::query("DELETE FROM race_groups WHERE id = ?")
            .bind(group_id)
            .execute(&self.pool)
            .await
            .with_context(|| format!("failed to delete group '{group_id}'"))?
            .rows_affected()
            > 0)
    }

    async fn load_key_pools_full(&self) -> anyhow::Result<Vec<RaceKeyPool>> {
        let rows = sqlx::query(
            "SELECT id, display_name, auth_strategy_json, selection_strategy, enabled
             FROM race_key_pools
             ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to load race_key_pools")?;

        let key_rows = sqlx::query(
            "SELECT id, key_pool_id, secret, enabled, metadata_json
             FROM race_keys
             ORDER BY key_pool_id ASC, id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to load race_keys")?;

        let mut keys_by_pool = BTreeMap::<String, Vec<RaceKey>>::new();
        for key_row in key_rows {
            let key_pool_id = key_row.get::<String, _>("key_pool_id");
            keys_by_pool
                .entry(key_pool_id.clone())
                .or_default()
                .push(RaceKey {
                    id: key_row.get("id"),
                    key_pool_id,
                    secret: key_row.get("secret"),
                    enabled: key_row.get::<i64, _>("enabled") != 0,
                    metadata: serde_json::from_str(&key_row.get::<String, _>("metadata_json"))
                        .context("deserialize key metadata_json")?,
                });
        }

        rows.into_iter()
            .map(|row| {
                let id = row.get::<String, _>("id");
                Ok(RaceKeyPool {
                    keys: keys_by_pool.remove(&id).unwrap_or_default(),
                    id,
                    display_name: row.get("display_name"),
                    auth_strategy: serde_json::from_str(
                        &row.get::<String, _>("auth_strategy_json"),
                    )
                    .context("deserialize auth_strategy_json")?,
                    selection_strategy: serde_json::from_str(
                        &row.get::<String, _>("selection_strategy"),
                    )
                    .context("deserialize selection_strategy")?,
                    enabled: row.get::<i64, _>("enabled") != 0,
                })
            })
            .collect()
    }

    async fn list_key_pools(&self) -> anyhow::Result<Vec<RaceKeyPoolSummary>> {
        let rows = sqlx::query(
            "SELECT id, display_name, auth_strategy_json, selection_strategy, enabled
             FROM race_key_pools
             ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to list race_key_pools")?;

        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let id = row.get::<String, _>("id");
            if let Some(pool) = self.get_key_pool(&id).await? {
                items.push(pool.summary());
            }
        }
        Ok(items)
    }

    async fn get_key_pool(&self, key_pool_id: &str) -> anyhow::Result<Option<RaceKeyPool>> {
        let Some(row) = sqlx::query(
            "SELECT id, display_name, auth_strategy_json, selection_strategy, enabled
             FROM race_key_pools
             WHERE id = ?",
        )
        .bind(key_pool_id)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("failed to fetch key pool '{key_pool_id}'"))?
        else {
            return Ok(None);
        };

        let key_rows = sqlx::query(
            "SELECT id, key_pool_id, secret, enabled, metadata_json
             FROM race_keys
             WHERE key_pool_id = ?
             ORDER BY id ASC",
        )
        .bind(key_pool_id)
        .fetch_all(&self.pool)
        .await
        .with_context(|| format!("failed to fetch keys for key pool '{key_pool_id}'"))?;

        let keys = key_rows
            .into_iter()
            .map(|key_row| {
                Ok(RaceKey {
                    id: key_row.get("id"),
                    key_pool_id: key_row.get("key_pool_id"),
                    secret: key_row.get("secret"),
                    enabled: key_row.get::<i64, _>("enabled") != 0,
                    metadata: serde_json::from_str(&key_row.get::<String, _>("metadata_json"))
                        .context("deserialize key metadata_json")?,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        Ok(Some(RaceKeyPool {
            id: row.get("id"),
            display_name: row.get("display_name"),
            auth_strategy: serde_json::from_str(&row.get::<String, _>("auth_strategy_json"))
                .context("deserialize auth_strategy_json")?,
            selection_strategy: serde_json::from_str(&row.get::<String, _>("selection_strategy"))
                .context("deserialize selection_strategy")?,
            enabled: row.get::<i64, _>("enabled") != 0,
            keys,
        }))
    }

    async fn put_key_pool(
        &self,
        previous_key_pool_id: Option<&str>,
        pool: RaceKeyPool,
    ) -> anyhow::Result<RaceKeyPool> {
        let mut tx = self
            .pool
            .begin()
            .await
            .context("failed to begin put_key_pool transaction")?;

        sqlx::query(
            "INSERT INTO race_key_pools (id, display_name, auth_strategy_json, selection_strategy, enabled)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                display_name = excluded.display_name,
                auth_strategy_json = excluded.auth_strategy_json,
                selection_strategy = excluded.selection_strategy,
                enabled = excluded.enabled",
        )
        .bind(&pool.id)
        .bind(&pool.display_name)
        .bind(serde_json::to_string(&pool.auth_strategy).context("serialize pool auth_strategy")?)
        .bind(serde_json::to_string(&pool.selection_strategy).context("serialize selection_strategy")?)
        .bind(i64::from(pool.enabled))
        .execute(&mut *tx)
        .await
        .with_context(|| format!("failed to upsert key pool '{}'", pool.id))?;

        if let Some(previous_key_pool_id) = previous_key_pool_id
            && previous_key_pool_id != pool.id
        {
            self.rebind_key_pool_references(&mut tx, previous_key_pool_id, &pool.id)
                .await?;
            sqlx::query("DELETE FROM race_key_pools WHERE id = ?")
                .bind(previous_key_pool_id)
                .execute(&mut *tx)
                .await
                .with_context(|| {
                    format!("failed to delete previous key pool '{previous_key_pool_id}'")
                })?;
        }

        sqlx::query("DELETE FROM race_keys WHERE key_pool_id = ?")
            .bind(&pool.id)
            .execute(&mut *tx)
            .await
            .with_context(|| format!("failed to clear keys for key pool '{}'", pool.id))?;

        for key in &pool.keys {
            self.insert_key(&mut tx, key).await?;
        }

        tx.commit()
            .await
            .context("failed to commit put_key_pool transaction")?;
        Ok(pool)
    }

    async fn delete_key_pool(&self, key_pool_id: &str) -> anyhow::Result<bool> {
        Ok(sqlx::query("DELETE FROM race_key_pools WHERE id = ?")
            .bind(key_pool_id)
            .execute(&self.pool)
            .await
            .with_context(|| format!("failed to delete key pool '{key_pool_id}'"))?
            .rows_affected()
            > 0)
    }

    async fn get_settings(&self) -> anyhow::Result<RaceSettings> {
        let row = sqlx::query("SELECT value FROM race_settings WHERE key = 'global'")
            .fetch_optional(&self.pool)
            .await
            .context("failed to fetch race_settings")?;
        match row {
            Some(row) => serde_json::from_str::<RaceSettings>(&row.get::<String, _>("value"))
                .map(RaceSettings::normalized)
                .context("deserialize race_settings value"),
            None => Ok(RaceSettings::default()),
        }
    }

    async fn put_settings(&self, settings: RaceSettings) -> anyhow::Result<RaceSettings> {
        let settings = settings.normalized();
        let mut tx = self
            .pool
            .begin()
            .await
            .context("failed to begin put_settings transaction")?;
        self.upsert_settings_tx(&mut tx, settings.clone()).await?;
        tx.commit()
            .await
            .context("failed to commit put_settings transaction")?;
        Ok(settings)
    }
}

fn decode_endpoint_row(row: sqlx::sqlite::SqliteRow) -> anyhow::Result<RaceTargetEndpoint> {
    Ok(RaceTargetEndpoint {
        protocol_family: serde_json::from_str(&row.get::<String, _>("protocol_family"))
            .context("deserialize protocol_family")?,
        base_url: row.get("base_url"),
        auth_strategy: serde_json::from_str(&row.get::<String, _>("auth_strategy_json"))
            .context("deserialize auth_strategy_json")?,
        key_pool_id: row.get("key_pool_id"),
        request_timeout_ms: row
            .get::<Option<i64>, _>("request_timeout_ms")
            .map(|value| value as u64),
        extra_headers: serde_json::from_str(&row.get::<String, _>("extra_headers_json"))
            .context("deserialize extra_headers_json")?,
        extra_query: serde_json::from_str(&row.get::<String, _>("extra_query_json"))
            .context("deserialize extra_query_json")?,
        enabled: row.get::<i64, _>("enabled") != 0,
    })
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::*;
    use crate::domain::{
        AuthStrategy, BootstrapData, KeySelectionStrategy, ProtocolFamily, RaceCandidate,
    };

    fn temp_database_url() -> String {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_millis();
        format!("sqlite://./target/race-gateway-test-{millis}.db")
    }

    #[tokio::test]
    async fn sqlite_round_trip() {
        let store = SqliteRaceConfigStore::connect(&temp_database_url())
            .await
            .expect("connect sqlite");

        let data = BootstrapData {
            models: vec![RaceModelDescriptor {
                id: "model-a".to_string(),
                display_name: "Model A".to_string(),
                upstream_model: "vendor/model-a".to_string(),
                description: "desc".to_string(),
                enabled: true,
                endpoints: vec![RaceTargetEndpoint {
                    protocol_family: ProtocolFamily::OpenAi,
                    base_url: "https://example.com/v1".to_string(),
                    auth_strategy: AuthStrategy::Bearer,
                    key_pool_id: "pool-a".to_string(),
                    request_timeout_ms: Some(15_000),
                    extra_headers: Default::default(),
                    extra_query: Default::default(),
                    enabled: true,
                }],
                metadata: json!({}),
            }],
            groups: vec![RaceGroup {
                id: "group-a".to_string(),
                display_name: "Group A".to_string(),
                fallback_ratio: 0.5,
                decay_factor: 0.8,
                penalty_rate: 5.0,
                recovery_rate: 0.1,
                race_max_wait_time_ms: Some(15_000),
                enabled: true,
                candidates: vec![RaceCandidate {
                    id: "cand-a".to_string(),
                    group_id: "group-a".to_string(),
                    name: "A".to_string(),
                    model_id: Some("model-a".to_string()),
                    upstream_model: "vendor/model-a".to_string(),
                    inline_endpoint_overrides: vec![],
                    initial_weight: 100.0,
                    response_protection_timeout_ms: 5_000,
                    enabled: true,
                    metadata: json!({}),
                }],
            }],
            key_pools: vec![RaceKeyPool {
                id: "pool-a".to_string(),
                display_name: "Pool A".to_string(),
                auth_strategy: AuthStrategy::Bearer,
                selection_strategy: KeySelectionStrategy::Random,
                enabled: true,
                keys: vec![RaceKey {
                    id: "key-a".to_string(),
                    key_pool_id: "pool-a".to_string(),
                    secret: "secret-a".to_string(),
                    enabled: true,
                    metadata: json!({}),
                }],
            }],
            settings: Some(RaceSettings::default()),
        };

        store.replace_all(data).await.expect("replace all");

        let model = store.get_model("model-a").await.expect("get model");
        let group = store.get_group("group-a").await.expect("get group");
        let pool = store.get_key_pool("pool-a").await.expect("get key pool");

        assert!(model.is_some());
        assert!(group.is_some());
        assert!(pool.is_some());
        assert_eq!(store.list_models().await.expect("list models").len(), 1);
        assert_eq!(store.list_groups().await.expect("list groups").len(), 1);
        assert_eq!(
            store.list_key_pools().await.expect("list key pools").len(),
            1
        );
    }

    #[tokio::test]
    async fn put_group_supports_group_id_rename() {
        let store = SqliteRaceConfigStore::connect(&temp_database_url())
            .await
            .expect("connect sqlite");

        store
            .replace_all(BootstrapData {
                models: vec![RaceModelDescriptor {
                    id: "model-a".to_string(),
                    display_name: "Model A".to_string(),
                    upstream_model: "vendor/model-a".to_string(),
                    description: "desc".to_string(),
                    enabled: true,
                    endpoints: vec![RaceTargetEndpoint {
                        protocol_family: ProtocolFamily::OpenAi,
                        base_url: "https://example.com/v1".to_string(),
                        auth_strategy: AuthStrategy::Bearer,
                        key_pool_id: "pool-a".to_string(),
                        request_timeout_ms: Some(15_000),
                        extra_headers: Default::default(),
                        extra_query: Default::default(),
                        enabled: true,
                    }],
                    metadata: json!({}),
                }],
                groups: vec![RaceGroup {
                    id: "group-a".to_string(),
                    display_name: "Group A".to_string(),
                    fallback_ratio: 0.5,
                    decay_factor: 0.8,
                    penalty_rate: 5.0,
                    recovery_rate: 0.1,
                    race_max_wait_time_ms: Some(15_000),
                    enabled: true,
                    candidates: vec![RaceCandidate {
                        id: "group-a-primary".to_string(),
                        group_id: "group-a".to_string(),
                        name: "Primary".to_string(),
                        model_id: Some("model-a".to_string()),
                        upstream_model: "vendor/model-a".to_string(),
                        inline_endpoint_overrides: vec![],
                        initial_weight: 100.0,
                        response_protection_timeout_ms: 5_000,
                        enabled: true,
                        metadata: json!({}),
                    }],
                }],
                key_pools: vec![RaceKeyPool {
                    id: "pool-a".to_string(),
                    display_name: "Pool A".to_string(),
                    auth_strategy: AuthStrategy::Bearer,
                    selection_strategy: KeySelectionStrategy::Random,
                    enabled: true,
                    keys: vec![RaceKey {
                        id: "key-a".to_string(),
                        key_pool_id: "pool-a".to_string(),
                        secret: "secret-a".to_string(),
                        enabled: true,
                        metadata: json!({}),
                    }],
                }],
                settings: Some(RaceSettings::default()),
            })
            .await
            .expect("replace all");

        let renamed = RaceGroup {
            id: "group-b".to_string(),
            display_name: "Group B".to_string(),
            fallback_ratio: 0.55,
            decay_factor: 0.85,
            penalty_rate: 4.0,
            recovery_rate: 0.2,
            race_max_wait_time_ms: Some(20_000),
            enabled: true,
            candidates: vec![RaceCandidate {
                id: "group-a-primary".to_string(),
                group_id: "group-b".to_string(),
                name: "Primary".to_string(),
                model_id: Some("model-a".to_string()),
                upstream_model: "vendor/model-a".to_string(),
                inline_endpoint_overrides: vec![],
                initial_weight: 100.0,
                response_protection_timeout_ms: 6_000,
                enabled: true,
                metadata: json!({ "renamed": true }),
            }],
        };

        store
            .put_group(Some("group-a"), renamed.clone())
            .await
            .expect("rename group");

        assert!(
            store
                .get_group("group-a")
                .await
                .expect("get old group")
                .is_none()
        );
        let saved = store
            .get_group("group-b")
            .await
            .expect("get renamed group")
            .expect("renamed group exists");
        assert_eq!(saved, renamed);
    }
}
