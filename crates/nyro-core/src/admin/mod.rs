use std::time::Instant;

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
            "SELECT id, name, protocol, base_url, api_key, is_active, priority, created_at, updated_at FROM providers ORDER BY priority ASC",
        )
        .fetch_all(&self.gw.db)
        .await?;
        Ok(rows)
    }

    pub async fn get_provider(&self, id: &str) -> anyhow::Result<Provider> {
        let row = sqlx::query_as::<_, Provider>(
            "SELECT id, name, protocol, base_url, api_key, is_active, priority, created_at, updated_at FROM providers WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&self.gw.db)
        .await?;
        Ok(row)
    }

    pub async fn create_provider(&self, input: CreateProvider) -> anyhow::Result<Provider> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO providers (id, name, protocol, base_url, api_key) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&input.name)
        .bind(&input.protocol)
        .bind(&input.base_url)
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
        let api_key = input.api_key.unwrap_or(current.api_key);
        let is_active = input.is_active.unwrap_or(current.is_active);
        let priority = input.priority.unwrap_or(current.priority);

        sqlx::query(
            "UPDATE providers SET name=?, protocol=?, base_url=?, api_key=?, is_active=?, priority=?, updated_at=datetime('now') WHERE id=?",
        )
        .bind(&name)
        .bind(&protocol)
        .bind(&base_url)
        .bind(&api_key)
        .bind(is_active)
        .bind(priority)
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
            let url = format!("{}/v1/messages", provider.base_url.trim_end_matches('/'));
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
            let url = format!("{}/v1/chat/completions", provider.base_url.trim_end_matches('/'));
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

    // ── Routes ──

    pub async fn list_routes(&self) -> anyhow::Result<Vec<Route>> {
        let rows = sqlx::query_as::<_, Route>(
            "SELECT id, name, match_pattern, target_provider, target_model, fallback_provider, fallback_model, is_active, priority, created_at FROM routes ORDER BY priority ASC",
        )
        .fetch_all(&self.gw.db)
        .await?;
        Ok(rows)
    }

    pub async fn create_route(&self, input: CreateRoute) -> anyhow::Result<Route> {
        let id = uuid::Uuid::new_v4().to_string();
        let max_priority: Option<i32> = sqlx::query("SELECT MAX(priority) as mp FROM routes")
            .fetch_one(&self.gw.db)
            .await?
            .try_get("mp")
            .ok();
        let priority = max_priority.unwrap_or(0) + 1;

        sqlx::query(
            "INSERT INTO routes (id, name, match_pattern, target_provider, target_model, fallback_provider, fallback_model, priority) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&input.name)
        .bind(&input.match_pattern)
        .bind(&input.target_provider)
        .bind(&input.target_model)
        .bind(&input.fallback_provider)
        .bind(&input.fallback_model)
        .bind(priority)
        .execute(&self.gw.db)
        .await?;

        let route = sqlx::query_as::<_, Route>(
            "SELECT id, name, match_pattern, target_provider, target_model, fallback_provider, fallback_model, is_active, priority, created_at FROM routes WHERE id = ?",
        )
        .bind(&id)
        .fetch_one(&self.gw.db)
        .await?;

        self.gw.route_cache.write().await.reload(&self.gw.db).await?;
        Ok(route)
    }

    pub async fn update_route(&self, id: &str, input: UpdateRoute) -> anyhow::Result<Route> {
        let current = sqlx::query_as::<_, Route>(
            "SELECT id, name, match_pattern, target_provider, target_model, fallback_provider, fallback_model, is_active, priority, created_at FROM routes WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&self.gw.db)
        .await?;

        let name = input.name.unwrap_or(current.name);
        let match_pattern = input.match_pattern.unwrap_or(current.match_pattern);
        let target_provider = input.target_provider.unwrap_or(current.target_provider);
        let target_model = input.target_model.unwrap_or(current.target_model);
        let fallback_provider = input.fallback_provider.or(current.fallback_provider);
        let fallback_model = input.fallback_model.or(current.fallback_model);
        let is_active = input.is_active.unwrap_or(current.is_active);
        let priority = input.priority.unwrap_or(current.priority);

        sqlx::query(
            "UPDATE routes SET name=?, match_pattern=?, target_provider=?, target_model=?, fallback_provider=?, fallback_model=?, is_active=?, priority=? WHERE id=?",
        )
        .bind(&name)
        .bind(&match_pattern)
        .bind(&target_provider)
        .bind(&target_model)
        .bind(&fallback_provider)
        .bind(&fallback_model)
        .bind(is_active)
        .bind(priority)
        .bind(id)
        .execute(&self.gw.db)
        .await?;

        self.gw.route_cache.write().await.reload(&self.gw.db).await?;

        sqlx::query_as::<_, Route>(
            "SELECT id, name, match_pattern, target_provider, target_model, fallback_provider, fallback_model, is_active, priority, created_at FROM routes WHERE id = ?",
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
            "SELECT id, created_at, ingress_protocol, egress_protocol, request_model, actual_model, provider_name, status_code, duration_ms, input_tokens, output_tokens, is_stream, is_tool_call, error_message, request_preview, response_preview FROM request_logs WHERE {where_sql} ORDER BY created_at DESC LIMIT {limit} OFFSET {offset}"
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
                    api_key: p.api_key,
                    is_active: p.is_active,
                    priority: p.priority,
                })
                .collect(),
            routes: routes
                .into_iter()
                .map(|r| ExportRoute {
                    name: r.name,
                    match_pattern: r.match_pattern,
                    target_provider_name: String::new(),
                    target_model: r.target_model,
                    fallback_provider_name: None,
                    fallback_model: r.fallback_model,
                    is_active: r.is_active,
                    priority: r.priority,
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
                            match_pattern: r.match_pattern.clone(),
                            target_provider: pid,
                            target_model: r.target_model.clone(),
                            fallback_provider: None,
                            fallback_model: r.fallback_model.clone(),
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
}
