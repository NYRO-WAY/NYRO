use std::time::Instant;

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde_json::Value;
use sqlx::Row;

use crate::db::models::*;
use crate::Gateway;

#[derive(Clone)]
pub struct AdminService {
    gw: Gateway,
}

impl AdminService {
    pub fn new(gw: Gateway) -> Self {
        Self { gw }
    }

    // ── Providers ──

    pub async fn list_providers(&self) -> anyhow::Result<Vec<Provider>> {
        let rows = sqlx::query_as::<_, Provider>(
            "SELECT id, name, protocol, base_url, preset_key, COALESCE(channel, region) AS channel, models_endpoint, static_models, api_key, is_active, created_at, updated_at FROM providers ORDER BY created_at DESC",
        )
        .fetch_all(&self.gw.db)
        .await?;
        Ok(rows)
    }

    pub async fn get_provider(&self, id: &str) -> anyhow::Result<Provider> {
        let row = sqlx::query_as::<_, Provider>(
            "SELECT id, name, protocol, base_url, preset_key, COALESCE(channel, region) AS channel, models_endpoint, static_models, api_key, is_active, created_at, updated_at FROM providers WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&self.gw.db)
        .await?;
        Ok(row)
    }

    pub async fn create_provider(&self, input: CreateProvider) -> anyhow::Result<Provider> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO providers (id, name, protocol, base_url, preset_key, channel, models_endpoint, static_models, api_key) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&input.name)
        .bind(&input.protocol)
        .bind(&input.base_url)
        .bind(&input.preset_key)
        .bind(&input.channel)
        .bind(&input.models_endpoint)
        .bind(&input.static_models)
        .bind(&input.api_key)
        .execute(&self.gw.db)
        .await?;

        self.get_provider(&id).await
    }

    pub async fn update_provider(
        &self,
        id: &str,
        input: UpdateProvider,
    ) -> anyhow::Result<Provider> {
        let current = self.get_provider(id).await?;

        let name = input.name.unwrap_or(current.name);
        let protocol = input.protocol.unwrap_or(current.protocol);
        let base_url = input.base_url.unwrap_or(current.base_url);
        let preset_key = input.preset_key.or(current.preset_key);
        let channel = input.channel.or(current.channel);
        let models_endpoint = input.models_endpoint.or(current.models_endpoint);
        let static_models = input.static_models.or(current.static_models);
        let api_key = input.api_key.unwrap_or(current.api_key);
        let is_active = input.is_active.unwrap_or(current.is_active);

        sqlx::query(
            "UPDATE providers SET name=?, protocol=?, base_url=?, preset_key=?, channel=?, models_endpoint=?, static_models=?, api_key=?, is_active=?, updated_at=datetime('now') WHERE id=?",
        )
        .bind(&name)
        .bind(&protocol)
        .bind(&base_url)
        .bind(&preset_key)
        .bind(&channel)
        .bind(&models_endpoint)
        .bind(&static_models)
        .bind(&api_key)
        .bind(is_active)
        .bind(id)
        .execute(&self.gw.db)
        .await?;

        self.get_provider(id).await
    }

    pub async fn delete_provider(&self, id: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM providers WHERE id = ?")
            .bind(id)
            .execute(&self.gw.db)
            .await?;
        Ok(())
    }

    pub async fn test_provider(&self, id: &str) -> anyhow::Result<TestResult> {
        let provider = self.get_provider(id).await?;
        let start = Instant::now();

        let req = if provider.protocol == "anthropic" {
            let url = build_provider_url(&provider.base_url, "/v1/messages", &provider.protocol);
            let body = serde_json::json!({
                "model": "claude-3-haiku-20240307",
                "messages": [{"role": "user", "content": "Hi"}],
                "max_tokens": 1,
            });
            self.gw.http_client
                .post(url)
                .header("x-api-key", &provider.api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&body)
        } else {
            let url = build_provider_url(&provider.base_url, "/v1/chat/completions", &provider.protocol);
            let body = serde_json::json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "Hi"}],
                "max_tokens": 1,
            });
            self.gw.http_client
                .post(url)
                .header("Authorization", format!("Bearer {}", provider.api_key))
                .json(&body)
        };

        match req.send().await {
            Ok(resp) => {
                let latency = start.elapsed().as_millis() as u64;
                let status = resp.status();
                if status.is_success() {
                    let json: serde_json::Value = resp.json().await.unwrap_or_default();
                    let model = json.get("model").and_then(|v| v.as_str()).map(String::from);
                    Ok(TestResult {
                        success: true,
                        latency_ms: latency,
                        model,
                        error: None,
                    })
                } else {
                    let err_text = resp.text().await.unwrap_or_default();
                    Ok(TestResult {
                        success: false,
                        latency_ms: latency,
                        model: None,
                        error: Some(format!("HTTP {}: {}", status.as_u16(), err_text.chars().take(200).collect::<String>())),
                    })
                }
            }
            Err(e) => Ok(TestResult {
                success: false,
                latency_ms: start.elapsed().as_millis() as u64,
                model: None,
                error: Some(e.to_string()),
            }),
        }
    }

    pub async fn get_provider_models(&self, id: &str) -> anyhow::Result<Vec<String>> {
        let provider = self.get_provider(id).await?;

        if let Some(endpoint) = resolve_models_endpoint(&provider) {
            let mut request = self
                .gw
                .http_client
                .get(&endpoint)
                .headers(build_model_headers(&provider.protocol, &provider.api_key)?);

            if provider.protocol == "gemini" {
                let separator = if endpoint.contains('?') { '&' } else { '?' };
                request = self
                    .gw
                    .http_client
                    .get(format!("{endpoint}{separator}key={}", provider.api_key));
            }

            if let Ok(resp) = request.send().await {
                if resp.status().is_success() {
                    let json: Value = resp.json().await.unwrap_or_default();
                    let models = extract_models_from_response(&provider.protocol, &json);
                    if !models.is_empty() {
                        return Ok(models);
                    }
                }
            }
        }

        Ok(parse_static_models(provider.static_models.as_deref()))
    }

    // ── Routes ──

    pub async fn list_routes(&self) -> anyhow::Result<Vec<Route>> {
        let rows = sqlx::query_as::<_, Route>(
            "SELECT id, name, COALESCE(ingress_protocol, 'openai') AS ingress_protocol, COALESCE(NULLIF(virtual_model, ''), match_pattern) AS virtual_model, target_provider, target_model, COALESCE(access_control, 0) AS access_control, is_active, created_at FROM routes ORDER BY created_at DESC",
        )
        .fetch_all(&self.gw.db)
        .await?;
        Ok(rows)
    }

    pub async fn create_route(&self, input: CreateRoute) -> anyhow::Result<Route> {
        ensure_protocol(&input.ingress_protocol)?;
        ensure_virtual_model(&input.virtual_model)?;
        self.ensure_route_unique(None, &input.ingress_protocol, &input.virtual_model)
            .await?;

        let id = uuid::Uuid::new_v4().to_string();

        sqlx::query(
            "INSERT INTO routes (id, name, ingress_protocol, virtual_model, match_pattern, target_provider, target_model, access_control) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&input.name)
        .bind(input.ingress_protocol.trim().to_lowercase())
        .bind(input.virtual_model.trim())
        .bind(input.virtual_model.trim())
        .bind(&input.target_provider)
        .bind(&input.target_model)
        .bind(input.access_control.unwrap_or(false))
        .execute(&self.gw.db)
        .await?;

        let route = sqlx::query_as::<_, Route>(
            "SELECT id, name, COALESCE(ingress_protocol, 'openai') AS ingress_protocol, COALESCE(NULLIF(virtual_model, ''), match_pattern) AS virtual_model, target_provider, target_model, COALESCE(access_control, 0) AS access_control, is_active, created_at FROM routes WHERE id = ?",
        )
        .bind(&id)
        .fetch_one(&self.gw.db)
        .await?;

        self.gw.route_cache.write().await.reload(&self.gw.db).await?;
        Ok(route)
    }

    pub async fn update_route(&self, id: &str, input: UpdateRoute) -> anyhow::Result<Route> {
        let current = sqlx::query_as::<_, Route>(
            "SELECT id, name, COALESCE(ingress_protocol, 'openai') AS ingress_protocol, COALESCE(NULLIF(virtual_model, ''), match_pattern) AS virtual_model, target_provider, target_model, COALESCE(access_control, 0) AS access_control, is_active, created_at FROM routes WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&self.gw.db)
        .await?;

        let name = input.name.unwrap_or(current.name);
        let ingress_protocol = input.ingress_protocol.unwrap_or(current.ingress_protocol);
        let virtual_model = input.virtual_model.unwrap_or(current.virtual_model);
        let target_provider = input.target_provider.unwrap_or(current.target_provider);
        let target_model = input.target_model.unwrap_or(current.target_model);
        let access_control = input.access_control.unwrap_or(current.access_control);
        let is_active = input.is_active.unwrap_or(current.is_active);
        ensure_protocol(&ingress_protocol)?;
        ensure_virtual_model(&virtual_model)?;
        self.ensure_route_unique(Some(id), &ingress_protocol, &virtual_model)
            .await?;

        sqlx::query(
            "UPDATE routes SET name=?, ingress_protocol=?, virtual_model=?, match_pattern=?, target_provider=?, target_model=?, access_control=?, is_active=? WHERE id=?",
        )
        .bind(&name)
        .bind(ingress_protocol.trim().to_lowercase())
        .bind(virtual_model.trim())
        .bind(virtual_model.trim())
        .bind(&target_provider)
        .bind(&target_model)
        .bind(access_control)
        .bind(is_active)
        .bind(id)
        .execute(&self.gw.db)
        .await?;

        self.gw.route_cache.write().await.reload(&self.gw.db).await?;

        sqlx::query_as::<_, Route>(
            "SELECT id, name, COALESCE(ingress_protocol, 'openai') AS ingress_protocol, COALESCE(NULLIF(virtual_model, ''), match_pattern) AS virtual_model, target_provider, target_model, COALESCE(access_control, 0) AS access_control, is_active, created_at FROM routes WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&self.gw.db)
        .await
        .map_err(Into::into)
    }

    pub async fn delete_route(&self, id: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM routes WHERE id = ?")
            .bind(id)
            .execute(&self.gw.db)
            .await?;
        self.gw.route_cache.write().await.reload(&self.gw.db).await?;
        Ok(())
    }

    // ── API Keys ──

    pub async fn list_api_keys(&self) -> anyhow::Result<Vec<ApiKeyWithBindings>> {
        let rows = sqlx::query_as::<_, ApiKey>(
            "SELECT id, key, name, rpm, tpm, tpd, status, expires_at, created_at, updated_at FROM api_keys ORDER BY created_at DESC",
        )
        .fetch_all(&self.gw.db)
        .await?;

        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let route_ids = self.list_api_key_route_ids(&row.id).await?;
            items.push(ApiKeyWithBindings {
                id: row.id,
                key: row.key,
                name: row.name,
                rpm: row.rpm,
                tpm: row.tpm,
                tpd: row.tpd,
                status: row.status,
                expires_at: row.expires_at,
                created_at: row.created_at,
                updated_at: row.updated_at,
                route_ids,
            });
        }
        Ok(items)
    }

    pub async fn get_api_key(&self, id: &str) -> anyhow::Result<ApiKeyWithBindings> {
        let row = sqlx::query_as::<_, ApiKey>(
            "SELECT id, key, name, rpm, tpm, tpd, status, expires_at, created_at, updated_at FROM api_keys WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&self.gw.db)
        .await?;
        let route_ids = self.list_api_key_route_ids(id).await?;
        Ok(ApiKeyWithBindings {
            id: row.id,
            key: row.key,
            name: row.name,
            rpm: row.rpm,
            tpm: row.tpm,
            tpd: row.tpd,
            status: row.status,
            expires_at: row.expires_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
            route_ids,
        })
    }

    pub async fn create_api_key(&self, input: CreateApiKey) -> anyhow::Result<ApiKeyWithBindings> {
        let id = uuid::Uuid::new_v4().to_string();
        let raw_key = format!("sk-nyro-{}", uuid::Uuid::new_v4().simple());
        let key = raw_key.chars().take(28).collect::<String>();

        sqlx::query(
            "INSERT INTO api_keys (id, key, name, rpm, tpm, tpd, status, expires_at) VALUES (?, ?, ?, ?, ?, ?, 'active', ?)",
        )
        .bind(&id)
        .bind(&key)
        .bind(input.name.trim())
        .bind(input.rpm)
        .bind(input.tpm)
        .bind(input.tpd)
        .bind(input.expires_at.as_ref().map(|v| v.trim()).filter(|v| !v.is_empty()))
        .execute(&self.gw.db)
        .await?;

        self.replace_api_key_routes(&id, &input.route_ids).await?;
        self.get_api_key(&id).await
    }

    pub async fn update_api_key(&self, id: &str, input: UpdateApiKey) -> anyhow::Result<ApiKeyWithBindings> {
        let current = sqlx::query_as::<_, ApiKey>(
            "SELECT id, key, name, rpm, tpm, tpd, status, expires_at, created_at, updated_at FROM api_keys WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&self.gw.db)
        .await?;

        let name = input.name.unwrap_or(current.name);
        let rpm = input.rpm.or(current.rpm);
        let tpm = input.tpm.or(current.tpm);
        let tpd = input.tpd.or(current.tpd);
        let status = input.status.unwrap_or(current.status);
        let expires_at = input.expires_at.or(current.expires_at);

        if status != "active" && status != "revoked" {
            anyhow::bail!("invalid key status: {status}");
        }

        sqlx::query(
            "UPDATE api_keys SET name=?, rpm=?, tpm=?, tpd=?, status=?, expires_at=?, updated_at=datetime('now') WHERE id=?",
        )
        .bind(name.trim())
        .bind(rpm)
        .bind(tpm)
        .bind(tpd)
        .bind(status)
        .bind(expires_at.as_ref().map(|v| v.trim()).filter(|v| !v.is_empty()))
        .bind(id)
        .execute(&self.gw.db)
        .await?;

        if let Some(route_ids) = input.route_ids {
            self.replace_api_key_routes(id, &route_ids).await?;
        }

        self.get_api_key(id).await
    }

    pub async fn delete_api_key(&self, id: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM api_keys WHERE id = ?")
            .bind(id)
            .execute(&self.gw.db)
            .await?;
        Ok(())
    }

    // ── Logs ──

    pub async fn query_logs(&self, q: LogQuery) -> anyhow::Result<LogPage> {
        let limit = q.limit.unwrap_or(50).min(500);
        let offset = q.offset.unwrap_or(0);

        let mut where_clauses = vec!["1=1".to_string()];
        if let Some(ref p) = q.provider {
            where_clauses.push(format!("provider_name = '{}'", p.replace('\'', "''")));
        }
        if let Some(ref m) = q.model {
            where_clauses.push(format!("actual_model = '{}'", m.replace('\'', "''")));
        }
        if let Some(min) = q.status_min {
            where_clauses.push(format!("status_code >= {min}"));
        }
        if let Some(max) = q.status_max {
            where_clauses.push(format!("status_code <= {max}"));
        }
        let where_sql = where_clauses.join(" AND ");

        let count_sql = format!("SELECT COUNT(*) as cnt FROM request_logs WHERE {where_sql}");
        let total: i64 = sqlx::query(&count_sql)
            .fetch_one(&self.gw.db)
            .await?
            .try_get("cnt")
            .unwrap_or(0);

        let data_sql = format!(
            "SELECT id, created_at, api_key_id, ingress_protocol, egress_protocol, request_model, actual_model, provider_name, status_code, duration_ms, input_tokens, output_tokens, is_stream, is_tool_call, error_message, request_preview, response_preview FROM request_logs WHERE {where_sql} ORDER BY created_at DESC LIMIT {limit} OFFSET {offset}"
        );
        let items = sqlx::query_as::<_, RequestLog>(&data_sql)
            .fetch_all(&self.gw.db)
            .await?;

        Ok(LogPage { items, total })
    }

    // ── Stats ──

    pub async fn get_stats_overview(&self) -> anyhow::Result<StatsOverview> {
        let row = sqlx::query_as::<_, StatsOverview>(
            r#"SELECT
                COUNT(*) as total_requests,
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(AVG(duration_ms), 0) as avg_duration_ms,
                COALESCE(SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END), 0) as error_count
            FROM request_logs"#,
        )
        .fetch_one(&self.gw.db)
        .await?;
        Ok(row)
    }

    pub async fn get_stats_hourly(&self, hours: i32) -> anyhow::Result<Vec<StatsHourly>> {
        let rows = sqlx::query_as::<_, StatsHourly>(
            r#"SELECT
                strftime('%Y-%m-%d %H:00', created_at) as hour,
                COUNT(*) as request_count,
                SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END) as error_count,
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(AVG(duration_ms), 0) as avg_duration_ms
            FROM request_logs
            WHERE created_at >= datetime('now', ? || ' hours')
            GROUP BY hour
            ORDER BY hour ASC"#,
        )
        .bind(format!("-{hours}"))
        .fetch_all(&self.gw.db)
        .await?;
        Ok(rows)
    }

    pub async fn get_stats_by_model(&self) -> anyhow::Result<Vec<ModelStats>> {
        let rows = sqlx::query_as::<_, ModelStats>(
            r#"SELECT
                COALESCE(actual_model, 'unknown') as model,
                COUNT(*) as request_count,
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(AVG(duration_ms), 0) as avg_duration_ms
            FROM request_logs
            GROUP BY actual_model
            ORDER BY request_count DESC"#,
        )
        .fetch_all(&self.gw.db)
        .await?;
        Ok(rows)
    }

    pub async fn get_stats_by_provider(&self) -> anyhow::Result<Vec<ProviderStats>> {
        let rows = sqlx::query_as::<_, ProviderStats>(
            r#"SELECT
                COALESCE(provider_name, 'unknown') as provider,
                COUNT(*) as request_count,
                SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END) as error_count,
                COALESCE(AVG(duration_ms), 0) as avg_duration_ms
            FROM request_logs
            GROUP BY provider_name
            ORDER BY request_count DESC"#,
        )
        .fetch_all(&self.gw.db)
        .await?;
        Ok(rows)
    }

    // ── Settings ──

    pub async fn get_setting(&self, key: &str) -> anyhow::Result<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.gw.db)
            .await?;
        Ok(row.map(|r| r.0))
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, datetime('now')) ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
        )
        .bind(key)
        .bind(value)
        .execute(&self.gw.db)
        .await?;
        Ok(())
    }

    // ── Config Import/Export ──

    pub async fn export_config(&self) -> anyhow::Result<ExportData> {
        let providers = self.list_providers().await?;
        let routes = self.list_routes().await?;
        let settings: Vec<(String, String)> =
            sqlx::query_as("SELECT key, value FROM settings")
                .fetch_all(&self.gw.db)
                .await?;

        Ok(ExportData {
            version: 1,
            providers: providers
                .into_iter()
                .map(|p| ExportProvider {
                    name: p.name,
                    protocol: p.protocol,
                    base_url: p.base_url,
                    preset_key: p.preset_key,
                    channel: p.channel,
                    models_endpoint: p.models_endpoint,
                    static_models: p.static_models,
                    api_key: p.api_key,
                    is_active: p.is_active,
                })
                .collect(),
            routes: routes
                .into_iter()
                .map(|r| ExportRoute {
                    name: r.name,
                    ingress_protocol: r.ingress_protocol,
                    virtual_model: r.virtual_model,
                    target_provider_name: String::new(),
                    target_model: r.target_model,
                    access_control: r.access_control,
                    is_active: r.is_active,
                })
                .collect(),
            settings: settings.into_iter().collect(),
        })
    }

    pub async fn import_config(&self, data: ExportData) -> anyhow::Result<ImportResult> {
        let mut providers_imported = 0u32;
        let mut routes_imported = 0u32;
        let mut settings_imported = 0u32;

        for p in &data.providers {
            let exists = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM providers WHERE name = ?",
            )
            .bind(&p.name)
            .fetch_one(&self.gw.db)
            .await
            .unwrap_or(0);

            if exists == 0 {
                let _ = self
                    .create_provider(CreateProvider {
                        name: p.name.clone(),
                        protocol: p.protocol.clone(),
                        base_url: p.base_url.clone(),
                        preset_key: p.preset_key.clone(),
                        channel: p.channel.clone(),
                        models_endpoint: p.models_endpoint.clone(),
                        static_models: p.static_models.clone(),
                        api_key: p.api_key.clone(),
                    })
                    .await;
                providers_imported += 1;
            }
        }

        for r in &data.routes {
            let exists =
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM routes WHERE name = ?")
                    .bind(&r.name)
                    .fetch_one(&self.gw.db)
                    .await
                    .unwrap_or(0);

            if exists == 0 {
                let provider_id = sqlx::query_scalar::<_, String>(
                    "SELECT id FROM providers LIMIT 1",
                )
                .fetch_optional(&self.gw.db)
                .await?;

                if let Some(pid) = provider_id {
                    let _ = self
                        .create_route(CreateRoute {
                            name: r.name.clone(),
                            ingress_protocol: r.ingress_protocol.clone(),
                            virtual_model: r.virtual_model.clone(),
                            target_provider: pid,
                            target_model: r.target_model.clone(),
                            access_control: Some(r.access_control),
                        })
                        .await;
                    routes_imported += 1;
                }
            }
        }

        for (key, value) in &data.settings {
            self.set_setting(key, value).await?;
            settings_imported += 1;
        }

        Ok(ImportResult {
            providers_imported,
            routes_imported,
            settings_imported,
        })
    }

    async fn ensure_route_unique(
        &self,
        exclude_id: Option<&str>,
        ingress_protocol: &str,
        virtual_model: &str,
    ) -> anyhow::Result<()> {
        let normalized_protocol = ingress_protocol.trim().to_lowercase();
        let normalized_model = virtual_model.trim();
        let sql = if exclude_id.is_some() {
            "SELECT id FROM routes WHERE COALESCE(ingress_protocol, 'openai') = ? AND COALESCE(NULLIF(virtual_model, ''), match_pattern) = ? AND id != ? LIMIT 1"
        } else {
            "SELECT id FROM routes WHERE COALESCE(ingress_protocol, 'openai') = ? AND COALESCE(NULLIF(virtual_model, ''), match_pattern) = ? LIMIT 1"
        };

        let exists = if let Some(route_id) = exclude_id {
            sqlx::query_scalar::<_, String>(sql)
                .bind(&normalized_protocol)
                .bind(normalized_model)
                .bind(route_id)
                .fetch_optional(&self.gw.db)
                .await?
        } else {
            sqlx::query_scalar::<_, String>(sql)
                .bind(&normalized_protocol)
                .bind(normalized_model)
                .fetch_optional(&self.gw.db)
                .await?
        };

        if exists.is_some() {
            anyhow::bail!("route already exists for protocol={normalized_protocol}, model={normalized_model}");
        }
        Ok(())
    }

    async fn list_api_key_route_ids(&self, api_key_id: &str) -> anyhow::Result<Vec<String>> {
        let route_ids = sqlx::query_scalar::<_, String>(
            "SELECT route_id FROM api_key_routes WHERE api_key_id = ? ORDER BY route_id ASC",
        )
        .bind(api_key_id)
        .fetch_all(&self.gw.db)
        .await?;
        Ok(route_ids)
    }

    async fn replace_api_key_routes(&self, api_key_id: &str, route_ids: &[String]) -> anyhow::Result<()> {
        let mut tx = self.gw.db.begin().await?;
        sqlx::query("DELETE FROM api_key_routes WHERE api_key_id = ?")
            .bind(api_key_id)
            .execute(&mut *tx)
            .await?;

        for route_id in route_ids.iter().filter(|id| !id.trim().is_empty()) {
            sqlx::query("INSERT OR IGNORE INTO api_key_routes (api_key_id, route_id) VALUES (?, ?)")
                .bind(api_key_id)
                .bind(route_id.trim())
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}

fn ensure_protocol(protocol: &str) -> anyhow::Result<()> {
    match protocol.trim().to_lowercase().as_str() {
        "openai" | "anthropic" | "gemini" => Ok(()),
        _ => anyhow::bail!("unsupported ingress protocol: {protocol}"),
    }
}

fn ensure_virtual_model(model: &str) -> anyhow::Result<()> {
    if model.trim().is_empty() {
        anyhow::bail!("virtual_model cannot be empty");
    }
    Ok(())
}

fn build_provider_url(base_url: &str, path: &str, protocol: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if protocol == "openai" {
        let has_base_path = reqwest::Url::parse(base)
            .ok()
            .map(|url| {
                let pathname = url.path().trim_end_matches('/');
                !pathname.is_empty() && pathname != "/"
            })
            .unwrap_or(false);

        if has_base_path && path.starts_with("/v1/") {
            return format!("{base}{}", &path[3..]);
        }
    }

    format!("{base}{path}")
}

fn resolve_models_endpoint(provider: &Provider) -> Option<String> {
    if let Some(endpoint) = provider.models_endpoint.as_ref() {
        let trimmed = endpoint.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let base = provider.base_url.trim_end_matches('/');
    match provider.protocol.as_str() {
        "openai" | "anthropic" => {
            let has_base_path = reqwest::Url::parse(base)
                .ok()
                .map(|url| {
                    let pathname = url.path().trim_end_matches('/');
                    !pathname.is_empty() && pathname != "/"
                })
                .unwrap_or(false);
            if has_base_path {
                Some(format!("{base}/models"))
            } else {
                Some(format!("{base}/v1/models"))
            }
        }
        "gemini" => Some(format!("{base}/v1beta/models")),
        _ => None,
    }
}

fn build_model_headers(protocol: &str, api_key: &str) -> anyhow::Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    match protocol {
        "anthropic" => {
            headers.insert("x-api-key", HeaderValue::from_str(api_key)?);
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        }
        "gemini" => {}
        _ => {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {api_key}"))?,
            );
        }
    }
    Ok(headers)
}

fn extract_models_from_response(protocol: &str, json: &Value) -> Vec<String> {
    let mut models = match protocol {
        "gemini" => json
            .get("models")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("name").and_then(|value| value.as_str()))
            .map(|name| name.rsplit('/').next().unwrap_or(name).to_string())
            .collect::<Vec<_>>(),
        _ => json
            .get("data")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("id").and_then(|value| value.as_str()))
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
    };

    models.sort();
    models.dedup();
    models
}

fn parse_static_models(raw: Option<&str>) -> Vec<String> {
    let mut models = raw
        .unwrap_or("")
        .lines()
        .flat_map(|line| line.split(','))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    models
}
