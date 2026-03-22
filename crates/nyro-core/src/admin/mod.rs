use anyhow::Context;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde_json::Value;

use crate::db::models::*;
use crate::storage::traits::ProviderTestResult;
use crate::Gateway;

const MODELS_DEV_SNAPSHOT: &str = include_str!("../../assets/models.dev.json");
const PROVIDER_PRESETS_SNAPSHOT: &str = include_str!("../../assets/providers.json");
const MODELS_DEV_RUNTIME_FILE: &str = "models.dev.json";
const MODELS_DEV_SOURCE_URL: &str = "https://models.dev/api.json";
const MODELS_DEV_RUNTIME_TTL: Duration = Duration::from_secs(24 * 60 * 60);

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
        self.gw.storage.providers().list().await
    }

    pub async fn list_provider_presets(&self) -> anyhow::Result<Vec<Value>> {
        parse_provider_presets_snapshot()
    }

    pub async fn get_provider(&self, id: &str) -> anyhow::Result<Provider> {
        self.gw
            .storage
            .providers()
            .get(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("provider not found: {id}"))
    }

    pub async fn create_provider(&self, input: CreateProvider) -> anyhow::Result<Provider> {
        let name = normalize_name(&input.name, "provider name")?;
        self.ensure_provider_name_unique(None, &name).await?;
        let vendor = normalize_vendor(input.vendor.as_deref());
        self.gw
            .storage
            .providers()
            .create(CreateProvider {
                name,
                vendor,
                protocol: input.protocol,
                base_url: input.base_url,
                preset_key: input.preset_key,
                channel: input.channel,
                models_endpoint: input.models_endpoint,
                models_source: input.models_source,
                capabilities_source: input.capabilities_source,
                static_models: input.static_models,
                api_key: input.api_key,
            })
            .await
    }

    pub async fn update_provider(
        &self,
        id: &str,
        input: UpdateProvider,
    ) -> anyhow::Result<Provider> {
        let current = self.get_provider(id).await?;
        let current_base_url = current.base_url.clone();
        let models_source_input = input
            .effective_models_source()
            .map(ToString::to_string);

        let name = normalize_name(&input.name.unwrap_or(current.name), "provider name")?;
        self.ensure_provider_name_unique(Some(id), &name).await?;
        let vendor = if input.vendor.is_some() {
            normalize_vendor(input.vendor.as_deref())
        } else {
            normalize_vendor(current.vendor.as_deref())
        };
        let models_source = models_source_input
            .or_else(|| {
                current
                    .models_source
                    .as_deref()
                    .or(current.models_endpoint.as_deref())
                    .map(ToString::to_string)
            });
        let protocol = input.protocol.unwrap_or(current.protocol);
        let base_url = input.base_url.unwrap_or(current.base_url);
        let preset_key = input.preset_key.or(current.preset_key);
        let channel = input.channel.or(current.channel);
        let capabilities_source = input
            .capabilities_source
            .or(current.capabilities_source);
        let static_models = input.static_models.or(current.static_models);
        let api_key = input.api_key.unwrap_or(current.api_key);
        let is_active = input.is_active.unwrap_or(current.is_active);
        let base_url_changed = base_url != current_base_url;

        let provider = self
            .gw
            .storage
            .providers()
            .update(
                id,
                UpdateProvider {
                    name: Some(name),
                    vendor,
                    protocol: Some(protocol),
                    base_url: Some(base_url),
                    preset_key,
                    channel,
                    models_endpoint: models_source.clone(),
                    models_source,
                    capabilities_source,
                    static_models,
                    api_key: Some(api_key),
                    is_active: Some(is_active),
                },
            )
            .await?;

        if base_url_changed {
            self.gw.clear_ollama_capability_cache_for_provider(id).await;
        }

        Ok(provider)
    }

    pub async fn delete_provider(&self, id: &str) -> anyhow::Result<()> {
        self.gw.storage.providers().delete(id).await?;
        self.gw.clear_ollama_capability_cache_for_provider(id).await;
        Ok(())
    }

    pub async fn test_provider(&self, id: &str) -> anyhow::Result<TestResult> {
        let provider = self.get_provider(id).await?;
        self.gw
            .clear_ollama_capability_cache_for_provider(&provider.id)
            .await;
        let start = Instant::now();
        let base_url = provider.base_url.trim();
        let result = if base_url.is_empty() {
            TestResult {
                success: false,
                latency_ms: 0,
                model: None,
                error: Some("Base URL is empty".to_string()),
            }
        } else if reqwest::Url::parse(base_url).is_err() {
            TestResult {
                success: false,
                latency_ms: 0,
                model: None,
                error: Some("Base URL format is invalid".to_string()),
            }
        } else {
            match self
                .gw
                .http_client
                .get(base_url)
                .timeout(Duration::from_secs(10))
                .send()
                .await
            {
            // Any HTTP response means the endpoint is reachable, including 4xx.
            Ok(_) => TestResult {
                success: true,
                latency_ms: start.elapsed().as_millis() as u64,
                model: None,
                error: None,
            },
            Err(e) => TestResult {
                success: false,
                latency_ms: start.elapsed().as_millis() as u64,
                model: None,
                error: Some(format_connectivity_error(&e)),
            },
        }
        };
        self.record_provider_test_result(&provider.id, &result).await?;
        Ok(result)
    }

    async fn record_provider_test_result(
        &self,
        provider_id: &str,
        result: &TestResult,
    ) -> anyhow::Result<()> {
        self.gw
            .storage
            .providers()
            .record_test_result(
                provider_id,
                ProviderTestResult {
                    success: result.success,
                    tested_at: String::new(),
                },
            )
            .await
    }

    pub async fn test_provider_models(&self, id: &str) -> anyhow::Result<Vec<String>> {
        let provider = self.get_provider(id).await?;
        let endpoint = provider
            .effective_models_source()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Model Discovery URL is empty"))?
            .to_string();

        if let Some(models) = lookup_models_dev_models(&self.gw.config.data_dir, &endpoint)? {
            if models.is_empty() {
                anyhow::bail!("Model list format is invalid or empty");
            }
            return Ok(models);
        }

        let mut request = self
            .gw
            .http_client
            .get(&endpoint)
            .headers(build_model_headers(
                &provider.protocol,
                provider.vendor.as_deref(),
                &provider.api_key,
            )?)
            .timeout(Duration::from_secs(10));

        if provider.protocol == "gemini" {
            let separator = if endpoint.contains('?') { '&' } else { '?' };
            request = self
                .gw
                .http_client
                .get(format!("{endpoint}{separator}key={}", provider.api_key))
                .headers(build_model_headers(
                    &provider.protocol,
                    provider.vendor.as_deref(),
                    &provider.api_key,
                )?)
                .timeout(Duration::from_secs(10));
        }

        let resp = request.send().await.map_err(|e| anyhow::anyhow!(format_connectivity_error(&e)))?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            let preview = body.chars().take(200).collect::<String>();
            anyhow::bail!("HTTP {status}: {preview}");
        }

        let json: Value = resp.json().await.unwrap_or_default();
        let models = extract_models_from_response(&provider.protocol, provider.vendor.as_deref(), &json);
        if models.is_empty() {
            anyhow::bail!("Model list format is invalid or empty");
        }

        Ok(models)
    }

    pub async fn get_provider_models(&self, id: &str) -> anyhow::Result<Vec<String>> {
        let provider = self.get_provider(id).await?;

        if let Some(endpoint) = resolve_models_endpoint(&provider) {
            if let Some(models) = lookup_models_dev_models(&self.gw.config.data_dir, &endpoint)? {
                if !models.is_empty() {
                    return Ok(models);
                }
            }

            let mut request = self
                .gw
                .http_client
                .get(&endpoint)
                .headers(build_model_headers(
                    &provider.protocol,
                    provider.vendor.as_deref(),
                    &provider.api_key,
                )?);

            if provider.protocol == "gemini" {
                let separator = if endpoint.contains('?') { '&' } else { '?' };
                request = self
                    .gw
                    .http_client
                    .get(format!("{endpoint}{separator}key={}", provider.api_key))
                    .headers(build_model_headers(
                        &provider.protocol,
                        provider.vendor.as_deref(),
                        &provider.api_key,
                    )?);
            }

            if let Ok(resp) = request.send().await {
                if resp.status().is_success() {
                    let json: Value = resp.json().await.unwrap_or_default();
                    let models = extract_models_from_response(
                        &provider.protocol,
                        provider.vendor.as_deref(),
                        &json,
                    );
                    if !models.is_empty() {
                        return Ok(models);
                    }
                }
            }
        }

        Ok(parse_static_models(provider.static_models.as_deref()))
    }

    pub async fn get_model_capabilities(
        &self,
        provider_id: &str,
        model: &str,
    ) -> anyhow::Result<ModelCapabilities> {
        let provider = self.get_provider(provider_id).await?;
        let trimmed_model = model.trim();
        if trimmed_model.is_empty() {
            anyhow::bail!("model cannot be empty");
        }
        self.resolve_provider_model_capabilities(&provider, trimmed_model).await
    }

    async fn resolve_provider_model_capabilities(
        &self,
        provider: &Provider,
        model: &str,
    ) -> anyhow::Result<ModelCapabilities> {
        let source = provider
            .capabilities_source
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("");

        match parse_source(source) {
            ResolvedSource::ModelsDev(vendor_key) => {
                let matched = lookup_models_dev_capability(&self.gw.config.data_dir, vendor_key, model);
                matched.ok_or_else(|| anyhow::anyhow!("no matched model capabilities found in models.dev"))
            }
            ResolvedSource::Http(url) => {
                if is_ollama_show_endpoint(url) {
                    self.query_ollama_show_capability(url, model).await
                } else {
                    self.query_http_capability(provider, url, model).await
                }
            }
            ResolvedSource::Auto => Ok(
                fuzzy_match_models_dev(&self.gw.config.data_dir, model)
                    .ok_or_else(|| anyhow::anyhow!("no matched model capabilities found in auto mode"))?,
            ),
        }
    }

    async fn query_http_capability(
        &self,
        provider: &Provider,
        url: &str,
        model: &str,
    ) -> anyhow::Result<ModelCapabilities> {
        let mut request = self
            .gw
            .http_client
            .get(url)
            .headers(build_model_headers(
                &provider.protocol,
                provider.vendor.as_deref(),
                &provider.api_key,
            )?)
            .timeout(Duration::from_secs(10));

        if provider.protocol == "gemini" {
            let separator = if url.contains('?') { '&' } else { '?' };
            request = self
                .gw
                .http_client
                .get(format!("{url}{separator}key={}", provider.api_key))
                .headers(build_model_headers(
                    &provider.protocol,
                    provider.vendor.as_deref(),
                    &provider.api_key,
                )?)
                .timeout(Duration::from_secs(10));
        }

        let resp = request
            .send()
            .await
            .map_err(|e| anyhow::anyhow!(format_connectivity_error(&e)))?;
        if !resp.status().is_success() {
            anyhow::bail!("capability source returned status {}", resp.status());
        }
        let json: Value = resp.json().await.unwrap_or_default();
        if let Some(cap) = parse_http_capability(&json, model) {
            return Ok(cap);
        }
        anyhow::bail!("no matched model capabilities found from capability source")
    }

    async fn query_ollama_show_capability(
        &self,
        url: &str,
        model: &str,
    ) -> anyhow::Result<ModelCapabilities> {
        let resp = self
            .gw
            .http_client
            .post(url)
            .json(&serde_json::json!({ "name": model }))
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!(format_connectivity_error(&e)))?;
        if !resp.status().is_success() {
            anyhow::bail!("ollama /api/show returned status {}", resp.status());
        }
        let json: Value = resp.json().await.unwrap_or_default();
        Ok(parse_ollama_capability(&json, model))
    }

    // ── Routes ──

    pub async fn list_routes(&self) -> anyhow::Result<Vec<Route>> {
        self.gw.storage.routes().list().await
    }

    pub async fn create_route(&self, input: CreateRoute) -> anyhow::Result<Route> {
        let name = normalize_name(&input.name, "route name")?;
        self.ensure_route_name_unique(None, &name).await?;
        ensure_protocol(&input.ingress_protocol)?;
        ensure_virtual_model(&input.virtual_model)?;
        self.ensure_route_unique(None, &input.ingress_protocol, &input.virtual_model)
            .await?;

        let route = self
            .gw
            .storage
            .routes()
            .create(CreateRoute {
                name,
                ingress_protocol: input.ingress_protocol,
                virtual_model: input.virtual_model,
                target_provider: input.target_provider,
                target_model: input.target_model,
                access_control: input.access_control,
            })
            .await?;
        self.reload_route_cache().await?;
        Ok(route)
    }

    pub async fn update_route(&self, id: &str, input: UpdateRoute) -> anyhow::Result<Route> {
        let current = self.get_route_by_id(id).await?;

        let name = normalize_name(&input.name.unwrap_or(current.name), "route name")?;
        self.ensure_route_name_unique(Some(id), &name).await?;
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

        self.gw
            .storage
            .routes()
            .update(
                id,
                UpdateRoute {
                    name: Some(name),
                    ingress_protocol: Some(ingress_protocol),
                    virtual_model: Some(virtual_model),
                    target_provider: Some(target_provider),
                    target_model: Some(target_model),
                    access_control: Some(access_control),
                    is_active: Some(is_active),
                },
            )
            .await?;
        self.reload_route_cache().await?;
        self.get_route_by_id(id).await
    }

    pub async fn delete_route(&self, id: &str) -> anyhow::Result<()> {
        self.gw.storage.routes().delete(id).await?;
        self.reload_route_cache().await?;
        Ok(())
    }

    // ── API Keys ──

    pub async fn list_api_keys(&self) -> anyhow::Result<Vec<ApiKeyWithBindings>> {
        self.api_keys_store()?.list().await
    }

    pub async fn get_api_key(&self, id: &str) -> anyhow::Result<ApiKeyWithBindings> {
        self.api_keys_store()?
            .get(id)
            .await?
            .context("api key not found")
    }

    pub async fn create_api_key(&self, input: CreateApiKey) -> anyhow::Result<ApiKeyWithBindings> {
        let name = normalize_name(&input.name, "api key name")?;
        self.ensure_api_key_name_unique(None, &name).await?;
        self.api_keys_store()?
            .create(CreateApiKey {
                name,
                rpm: input.rpm,
                rpd: input.rpd,
                tpm: input.tpm,
                tpd: input.tpd,
                expires_at: input.expires_at,
                route_ids: input.route_ids,
            })
            .await
    }

    pub async fn update_api_key(&self, id: &str, input: UpdateApiKey) -> anyhow::Result<ApiKeyWithBindings> {
        let current = self
            .api_keys_store()?
            .get(id)
            .await?
            .context("api key not found")?;

        let name = normalize_name(&input.name.unwrap_or(current.name), "api key name")?;
        self.ensure_api_key_name_unique(Some(id), &name).await?;
        let rpm = input.rpm.or(current.rpm);
        let rpd = input.rpd.or(current.rpd);
        let tpm = input.tpm.or(current.tpm);
        let tpd = input.tpd.or(current.tpd);
        let status = input.status.unwrap_or(current.status);
        let expires_at = input.expires_at.or(current.expires_at);

        if status != "active" && status != "revoked" {
            anyhow::bail!("invalid key status: {status}");
        }

        self.api_keys_store()?
            .update(
                id,
                UpdateApiKey {
                    name: Some(name),
                    rpm,
                    rpd,
                    tpm,
                    tpd,
                    status: Some(status),
                    expires_at,
                    route_ids: input.route_ids,
                },
            )
            .await
    }

    pub async fn delete_api_key(&self, id: &str) -> anyhow::Result<()> {
        self.api_keys_store()?.delete(id).await?;
        Ok(())
    }

    // ── Logs ──

    pub async fn query_logs(&self, q: LogQuery) -> anyhow::Result<LogPage> {
        let mut q = q;
        q.limit = Some(q.limit.unwrap_or(50).min(500));
        q.offset = Some(q.offset.unwrap_or(0));
        self.gw.storage.logs().query(q).await
    }

    // ── Stats ──

    fn normalize_hours(hours: Option<i32>) -> Option<i32> {
        hours.and_then(|value| (value > 0).then_some(value))
    }

    pub async fn get_stats_overview(&self, hours: Option<i32>) -> anyhow::Result<StatsOverview> {
        self.gw
            .storage
            .logs()
            .stats_overview(Self::normalize_hours(hours).map(i64::from))
            .await
    }

    pub async fn get_stats_hourly(&self, hours: i32) -> anyhow::Result<Vec<StatsHourly>> {
        self.gw
            .storage
            .logs()
            .stats_hourly(i64::from(hours.max(1)))
            .await
    }

    pub async fn get_stats_by_model(&self, hours: Option<i32>) -> anyhow::Result<Vec<ModelStats>> {
        self.gw
            .storage
            .logs()
            .stats_by_model(Self::normalize_hours(hours).map(i64::from))
            .await
    }

    pub async fn get_stats_by_provider(
        &self,
        hours: Option<i32>,
    ) -> anyhow::Result<Vec<ProviderStats>> {
        self.gw
            .storage
            .logs()
            .stats_by_provider(Self::normalize_hours(hours).map(i64::from))
            .await
    }

    // ── Settings ──

    pub async fn get_setting(&self, key: &str) -> anyhow::Result<Option<String>> {
        self.gw.storage.settings().get(key).await
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> anyhow::Result<()> {
        self.gw.storage.settings().set(key, value).await
    }

    // ── Config Import/Export ──

    pub async fn export_config(&self) -> anyhow::Result<ExportData> {
        let providers = self.list_providers().await?;
        let routes = self.list_routes().await?;
        let settings = self.gw.storage.settings().list_all().await?;

        Ok(ExportData {
            version: 1,
            providers: providers
                .into_iter()
                .map(|p| ExportProvider {
                    name: p.name,
                    vendor: p.vendor,
                    protocol: p.protocol,
                    base_url: p.base_url,
                    preset_key: p.preset_key,
                    channel: p.channel,
                    models_endpoint: p.models_endpoint,
                    models_source: p.models_source,
                    capabilities_source: p.capabilities_source,
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
            let exists = self
                .gw
                .storage
                .providers()
                .exists_by_name(&p.name, None)
                .await
                .unwrap_or(false);

            if !exists {
                if self
                    .create_provider(CreateProvider {
                        name: p.name.clone(),
                        vendor: p.vendor.clone(),
                        protocol: p.protocol.clone(),
                        base_url: p.base_url.clone(),
                        preset_key: p.preset_key.clone(),
                        channel: p.channel.clone(),
                        models_endpoint: p.models_endpoint.clone(),
                        models_source: p.models_source.clone(),
                        capabilities_source: p.capabilities_source.clone(),
                        static_models: p.static_models.clone(),
                        api_key: p.api_key.clone(),
                    })
                    .await
                    .is_ok()
                {
                    providers_imported += 1;
                }
            }
        }

        let fallback_provider_id = self
            .list_providers()
            .await?
            .into_iter()
            .next()
            .map(|provider| provider.id);

        for r in &data.routes {
            let exists = self
                .gw
                .storage
                .routes()
                .exists_by_name(&r.name, None)
                .await
                .unwrap_or(false);

            if !exists {
                if let Some(pid) = fallback_provider_id.clone() {
                    if self
                        .create_route(CreateRoute {
                            name: r.name.clone(),
                            ingress_protocol: r.ingress_protocol.clone(),
                            virtual_model: r.virtual_model.clone(),
                            target_provider: pid,
                            target_model: r.target_model.clone(),
                            access_control: Some(r.access_control),
                        })
                        .await
                        .is_ok()
                    {
                        routes_imported += 1;
                    }
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
        if self
            .gw
            .storage
            .routes()
            .exists_by_protocol_model(ingress_protocol, virtual_model, exclude_id)
            .await?
        {
            let normalized_protocol = ingress_protocol.trim().to_lowercase();
            let normalized_model = virtual_model.trim();
            anyhow::bail!("route already exists for protocol={normalized_protocol}, model={normalized_model}");
        }
        Ok(())
    }

    async fn ensure_provider_name_unique(
        &self,
        exclude_id: Option<&str>,
        name: &str,
    ) -> anyhow::Result<()> {
        if self
            .gw
            .storage
            .providers()
            .exists_by_name(name, exclude_id)
            .await?
        {
            return Err(coded_error(
                "PROVIDER_NAME_CONFLICT",
                &format!("provider name already exists: {name}"),
                serde_json::json!({ "name": name }),
            ));
        }
        Ok(())
    }

    async fn ensure_route_name_unique(
        &self,
        exclude_id: Option<&str>,
        name: &str,
    ) -> anyhow::Result<()> {
        if self
            .gw
            .storage
            .routes()
            .exists_by_name(name, exclude_id)
            .await?
        {
            return Err(coded_error(
                "ROUTE_NAME_CONFLICT",
                &format!("route name already exists: {name}"),
                serde_json::json!({ "name": name }),
            ));
        }
        Ok(())
    }

    async fn ensure_api_key_name_unique(
        &self,
        exclude_id: Option<&str>,
        name: &str,
    ) -> anyhow::Result<()> {
        if self.api_keys_store()?.exists_by_name(name, exclude_id).await? {
            return Err(coded_error(
                "API_KEY_NAME_CONFLICT",
                &format!("api key name already exists: {name}"),
                serde_json::json!({ "name": name }),
            ));
        }
        Ok(())
    }

    async fn get_route_by_id(&self, id: &str) -> anyhow::Result<Route> {
        self.gw
            .storage
            .routes()
            .get(id)
            .await?
            .context("route not found")
    }

    async fn reload_route_cache(&self) -> anyhow::Result<()> {
        self.gw
            .route_cache
            .write()
            .await
            .reload(self.gw.storage.snapshots())
            .await
    }

    fn api_keys_store(&self) -> anyhow::Result<&dyn crate::storage::traits::ApiKeyStore> {
        self.gw
            .storage
            .api_keys()
            .context("selected storage backend does not support api key management")
    }
}

fn format_connectivity_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        return "Connection timeout (10s), please check Base URL or network settings".to_string();
    }
    if error.is_connect() {
        return "Unable to connect to the host, please check DNS/network settings".to_string();
    }
    error.to_string()
}

fn coded_error(code: &str, message: &str, params: Value) -> anyhow::Error {
    anyhow::anyhow!(
        "{}",
        serde_json::json!({
            "code": code,
            "message": message,
            "params": params,
        })
    )
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

fn normalize_name(name: &str, field: &str) -> anyhow::Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        anyhow::bail!("{field} cannot be empty");
    }
    Ok(trimmed.to_string())
}

fn normalize_vendor(vendor: Option<&str>) -> Option<String> {
    vendor
        .map(str::trim)
        .filter(|v| !v.is_empty() && *v != "custom")
        .map(|v| v.to_lowercase())
}

fn resolve_models_endpoint(provider: &Provider) -> Option<String> {
    if let Some(endpoint) = provider.effective_models_source() {
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

fn build_model_headers(
    protocol: &str,
    vendor: Option<&str>,
    api_key: &str,
) -> anyhow::Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    let is_google_vendor = vendor
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("google"));
    match protocol {
        "anthropic" => {
            headers.insert("x-api-key", HeaderValue::from_str(api_key)?);
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        }
        "gemini" => {
            // Google providers may expose OpenAI-compatible /v1/models endpoints.
            // Add Bearer auth in addition to Gemini key query param.
            if is_google_vendor {
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(&format!("Bearer {api_key}"))?,
                );
            }
        }
        _ => {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {api_key}"))?,
            );
        }
    }
    Ok(headers)
}

fn extract_models_from_response(protocol: &str, vendor: Option<&str>, json: &Value) -> Vec<String> {
    let is_google_vendor = vendor
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("google"));
    let mut models = json
        .get("data")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("id").and_then(|value| value.as_str()))
        .map(|id| {
            if is_google_vendor {
                id.strip_prefix("models/").unwrap_or(id).to_string()
            } else {
                id.to_string()
            }
        })
        .collect::<Vec<_>>();

    if models.is_empty() && protocol == "gemini" {
        models = json
            .get("models")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("name").and_then(|value| value.as_str()))
            .map(|name| {
                let normalized = name.rsplit('/').next().unwrap_or(name);
                if is_google_vendor {
                    normalized.strip_prefix("models/").unwrap_or(normalized).to_string()
                } else {
                    normalized.to_string()
                }
            })
            .collect::<Vec<_>>();
    }

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

#[derive(Debug, Clone, Copy)]
enum ResolvedSource<'a> {
    Http(&'a str),
    ModelsDev(&'a str),
    Auto,
}

fn parse_source(uri: &str) -> ResolvedSource<'_> {
    let trimmed = uri.trim();
    if trimmed.is_empty() {
        ResolvedSource::Auto
    } else if trimmed.eq_ignore_ascii_case("ai://models.dev") {
        ResolvedSource::ModelsDev("")
    } else if let Some(key) = trimmed.strip_prefix("ai://models.dev/") {
        ResolvedSource::ModelsDev(key)
    } else {
        ResolvedSource::Http(trimmed)
    }
}

fn is_ollama_show_endpoint(url: &str) -> bool {
    url.trim_end_matches('/').ends_with("/api/show")
}

fn parse_ollama_capability(json: &Value, model: &str) -> ModelCapabilities {
    let caps = json
        .get("capabilities")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let has_vision = caps.iter().any(|c| c.eq_ignore_ascii_case("vision"));
    let context_window = json
        .get("model_info")
        .and_then(Value::as_object)
        .and_then(extract_ollama_context_window)
        .unwrap_or(8 * 1024);

    ModelCapabilities {
        provider: "ollama".to_string(),
        model_id: model.to_string(),
        context_window,
        output_max_tokens: None,
        tool_call: caps.iter().any(|c| c == "tools"),
        reasoning: caps.iter().any(|c| c == "thinking"),
        input_modalities: if has_vision {
            vec!["text".to_string(), "image".to_string()]
        } else {
            vec!["text".to_string()]
        },
        output_modalities: vec!["text".to_string()],
        input_cost: Some(0.0),
        output_cost: Some(0.0),
    }
}

fn extract_ollama_context_window(model_info: &serde_json::Map<String, Value>) -> Option<u64> {
    let arch = model_info.get("general.architecture")?.as_str()?;
    let key = format!("{arch}.context_length");
    model_info
        .get(&key)
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
}

pub async fn refresh_models_dev_runtime_cache_if_stale(
    data_dir: PathBuf,
    http_client: reqwest::Client,
) {
    if let Err(err) = refresh_models_dev_runtime_cache_inner(&data_dir, &http_client, false).await {
        tracing::warn!("models.dev runtime refresh skipped: {err}");
    }
}

pub async fn refresh_models_dev_runtime_cache_on_startup(
    data_dir: PathBuf,
    http_client: reqwest::Client,
) {
    if let Err(err) = refresh_models_dev_runtime_cache_inner(&data_dir, &http_client, true).await {
        tracing::warn!("models.dev startup refresh failed, fallback to local cache/snapshot: {err}");
    }
}

fn models_dev_runtime_cache_path(data_dir: &Path) -> PathBuf {
    data_dir.join(MODELS_DEV_RUNTIME_FILE)
}

async fn refresh_models_dev_runtime_cache_inner(
    data_dir: &Path,
    http_client: &reqwest::Client,
    force_refresh: bool,
) -> anyhow::Result<()> {
    let cache_path = models_dev_runtime_cache_path(data_dir);
    if !force_refresh {
        if let Ok(meta) = std::fs::metadata(&cache_path) {
            if let Ok(modified_at) = meta.modified() {
                if let Ok(elapsed) = modified_at.elapsed() {
                    if elapsed < MODELS_DEV_RUNTIME_TTL {
                        return Ok(());
                    }
                }
            }
        }
    }

    let resp = http_client
        .get(MODELS_DEV_SOURCE_URL)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("request models.dev failed: {e}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("models.dev returned status {}", resp.status());
    }
    let body = resp
        .text()
        .await
        .map_err(|e| anyhow::anyhow!("read models.dev body failed: {e}"))?;

    // Validate payload shape before replacing local cache.
    let _: HashMap<String, ModelsDevVendor> = serde_json::from_str(&body)
        .map_err(|e| anyhow::anyhow!("invalid models.dev payload: {e}"))?;

    std::fs::create_dir_all(data_dir)?;
    let tmp_path = data_dir.join(format!("{MODELS_DEV_RUNTIME_FILE}.tmp"));
    std::fs::write(&tmp_path, body.as_bytes())?;
    std::fs::rename(&tmp_path, &cache_path)?;
    Ok(())
}

fn parse_provider_presets_snapshot() -> anyhow::Result<Vec<Value>> {
    let parsed = serde_json::from_str::<Value>(PROVIDER_PRESETS_SNAPSHOT)
        .map_err(|e| anyhow::anyhow!("invalid providers preset snapshot: {e}"))?;
    let Some(items) = parsed.as_array() else {
        anyhow::bail!("invalid providers preset snapshot: root must be array");
    };
    Ok(items.clone())
}

fn parse_models_dev_data(data_dir: &Path) -> anyhow::Result<HashMap<String, ModelsDevVendor>> {
    let cache_path = models_dev_runtime_cache_path(data_dir);
    if let Ok(content) = std::fs::read_to_string(&cache_path) {
        if let Ok(parsed) = serde_json::from_str::<HashMap<String, ModelsDevVendor>>(&content) {
            return Ok(parsed);
        }
        tracing::warn!(
            "invalid models.dev runtime cache at {}, fallback to embedded snapshot",
            cache_path.display()
        );
    }
    parse_models_dev_snapshot()
}

fn lookup_models_dev_models(data_dir: &Path, source: &str) -> anyhow::Result<Option<Vec<String>>> {
    let ResolvedSource::ModelsDev(vendor_key) = parse_source(source) else {
        return Ok(None);
    };
    let data = parse_models_dev_data(data_dir)?;
    if vendor_key.trim().is_empty() {
        let mut models = data
            .values()
            .flat_map(|vendor| vendor.models.keys().cloned())
            .collect::<Vec<_>>();
        models.sort();
        models.dedup();
        return Ok(Some(models));
    }
    let Some(vendor) = data.get(vendor_key) else {
        return Ok(Some(Vec::new()));
    };
    let mut models = vendor.models.keys().cloned().collect::<Vec<_>>();
    models.sort();
    Ok(Some(models))
}

fn lookup_models_dev_capability(
    data_dir: &Path,
    vendor_key: &str,
    model: &str,
) -> Option<ModelCapabilities> {
    let data = parse_models_dev_data(data_dir).ok()?;
    match_models_dev_capability(&data, vendor_key, model)
}

fn fuzzy_match_models_dev(data_dir: &Path, model: &str) -> Option<ModelCapabilities> {
    let data = parse_models_dev_data(data_dir).ok()?;
    match_models_dev_capability(&data, "", model)
}

fn match_models_dev_capability(
    data: &HashMap<String, ModelsDevVendor>,
    vendor_key: &str,
    model: &str,
) -> Option<ModelCapabilities> {
    let needle = model.trim().to_lowercase();
    if needle.is_empty() {
        return None;
    }

    if vendor_key.trim().is_empty() {
        for (vk, vendor) in data {
            for (model_id, entry) in &vendor.models {
                if model_id.eq_ignore_ascii_case(model) {
                    return Some(to_models_dev_capability(vk, entry));
                }
            }
        }
        let mut best: Option<(usize, ModelCapabilities)> = None;
        for (vk, vendor) in data {
            for (model_id, entry) in &vendor.models {
                if model_id.to_lowercase().contains(&needle) {
                    let cap = to_models_dev_capability(vk, entry);
                    let len = model_id.len();
                    let replace = best.as_ref().map(|(prev_len, _)| len < *prev_len).unwrap_or(true);
                    if replace {
                        best = Some((len, cap));
                    }
                }
            }
        }
        return best.map(|(_, cap)| cap);
    }

    let vendor = data.get(vendor_key)?;
    for (model_id, entry) in &vendor.models {
        if model_id.eq_ignore_ascii_case(model) {
            return Some(to_models_dev_capability(vendor_key, entry));
        }
    }
    let mut best: Option<(usize, ModelCapabilities)> = None;
    for (model_id, entry) in &vendor.models {
        if model_id.to_lowercase().contains(&needle) {
            let cap = to_models_dev_capability(vendor_key, entry);
            let len = model_id.len();
            let replace = best.as_ref().map(|(prev_len, _)| len < *prev_len).unwrap_or(true);
            if replace {
                best = Some((len, cap));
            }
        }
    }
    best.map(|(_, cap)| cap)
}

fn parse_http_capability(json: &Value, model: &str) -> Option<ModelCapabilities> {
    let arr = json.get("data").and_then(Value::as_array)?;
    let item = arr.iter().find(|entry| {
        entry
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id.eq_ignore_ascii_case(model))
    })?;

    let model_id = item.get("id").and_then(Value::as_str).unwrap_or(model);
    let context_window = item
        .get("context_length")
        .and_then(Value::as_u64)
        .filter(|v| *v > 0)
        .unwrap_or(128 * 1024);
    let output_max_tokens = item
        .get("top_provider")
        .and_then(Value::as_object)
        .and_then(|obj| obj.get("max_completion_tokens"))
        .and_then(Value::as_u64)
        .filter(|v| *v > 0);
    let supported_parameters = item
        .get("supported_parameters")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let input_modalities = item
        .get("architecture")
        .and_then(Value::as_object)
        .and_then(|obj| obj.get("input_modalities"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec!["text".to_string()]);
    let output_modalities = item
        .get("architecture")
        .and_then(Value::as_object)
        .and_then(|obj| obj.get("output_modalities"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec!["text".to_string()]);
    let input_cost = item
        .get("pricing")
        .and_then(Value::as_object)
        .and_then(|obj| obj.get("prompt"))
        .and_then(parse_maybe_price_per_token);
    let output_cost = item
        .get("pricing")
        .and_then(Value::as_object)
        .and_then(|obj| obj.get("completion"))
        .and_then(parse_maybe_price_per_token);
    let tool_call = supported_parameters.iter().any(|v| v.as_str() == Some("tools"));
    let model_lower = model_id.to_lowercase();
    let reasoning = model_lower.contains("reason")
        || model_lower.contains("thinking")
        || model_lower.contains("o1")
        || model_lower.contains("o3")
        || model_lower.contains("o4");

    Some(ModelCapabilities {
        provider: "openrouter".to_string(),
        model_id: model_id.to_string(),
        context_window,
        output_max_tokens,
        tool_call,
        reasoning,
        input_modalities,
        output_modalities,
        input_cost,
        output_cost,
    })
}

fn parse_maybe_price_per_token(value: &Value) -> Option<f64> {
    let parsed = if let Some(v) = value.as_f64() {
        Some(v)
    } else if let Some(s) = value.as_str() {
        s.parse::<f64>().ok()
    } else {
        None
    }?;
    if parsed <= 0.0 {
        return None;
    }
    Some(parsed * 1_000_000.0)
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ModelsDevVendor {
    #[serde(default)]
    models: HashMap<String, ModelsDevModelEntry>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ModelsDevModelEntry {
    id: String,
    #[serde(default)]
    reasoning: bool,
    #[serde(default)]
    tool_call: bool,
    #[serde(default)]
    modalities: ModelsDevModalities,
    #[serde(default)]
    cost: ModelsDevCost,
    #[serde(default)]
    limit: ModelsDevLimit,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
struct ModelsDevModalities {
    #[serde(default)]
    input: Vec<String>,
    #[serde(default)]
    output: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
struct ModelsDevCost {
    input: Option<f64>,
    output: Option<f64>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
struct ModelsDevLimit {
    context: Option<u64>,
    output: Option<u64>,
}

fn parse_models_dev_snapshot() -> anyhow::Result<HashMap<String, ModelsDevVendor>> {
    let parsed = serde_json::from_str::<HashMap<String, ModelsDevVendor>>(MODELS_DEV_SNAPSHOT)
        .map_err(|e| anyhow::anyhow!("failed to parse models.dev snapshot: {e}"))?;
    Ok(parsed)
}

fn to_models_dev_capability(vendor_key: &str, model: &ModelsDevModelEntry) -> ModelCapabilities {
    let input_modalities = if model.modalities.input.is_empty() {
        vec!["text".to_string()]
    } else {
        model.modalities.input.clone()
    };
    let output_modalities = if model.modalities.output.is_empty() {
        vec!["text".to_string()]
    } else {
        model.modalities.output.clone()
    };

    ModelCapabilities {
        provider: vendor_key.to_string(),
        model_id: model.id.clone(),
        context_window: model.limit.context.filter(|v| *v > 0).unwrap_or(128 * 1024),
        output_max_tokens: model.limit.output.filter(|v| *v > 0),
        tool_call: model.tool_call,
        reasoning: model.reasoning,
        input_modalities,
        output_modalities,
        input_cost: model.cost.input,
        output_cost: model.cost.output,
    }
}
