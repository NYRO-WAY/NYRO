use std::collections::HashMap;
use std::time::Duration;
use nyro_core::cache::{CacheBackendKind, CacheConfig, CacheMode, CacheType};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct YamlConfig {
    #[serde(default)]
    pub server: ServerSection,
    #[serde(default)]
    pub providers: Vec<YamlProvider>,
    #[serde(default)]
    pub routes: Vec<YamlRoute>,
    #[serde(default)]
    pub settings: HashMap<String, String>,
    #[serde(default)]
    pub cache: YamlCacheConfig,
}

#[derive(Debug, Deserialize, Default)]
pub struct YamlCacheConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub cache_type: Option<String>,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub default_ttl_secs: Option<u64>,
    #[serde(default)]
    pub max_entries: Option<usize>,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ServerSection {
    #[serde(default = "default_proxy_host")]
    pub proxy_host: String,
    #[serde(default = "default_proxy_port")]
    pub proxy_port: u16,
}

impl Default for ServerSection {
    fn default() -> Self {
        Self {
            proxy_host: default_proxy_host(),
            proxy_port: default_proxy_port(),
        }
    }
}

fn default_proxy_host() -> String {
    "127.0.0.1".to_string()
}
fn default_proxy_port() -> u16 {
    19530
}

#[derive(Debug, Deserialize)]
pub struct YamlProvider {
    pub name: String,
    pub default_protocol: String,
    #[serde(default)]
    pub endpoints: HashMap<String, YamlEndpoint>,
    pub api_key: String,
    #[serde(default)]
    pub use_proxy: bool,
    pub models_source: Option<String>,
    pub capabilities_source: Option<String>,
    pub static_models: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct YamlEndpoint {
    pub base_url: String,
}

#[derive(Debug, Deserialize)]
pub struct YamlRoute {
    pub name: String,
    pub virtual_model: String,
    #[serde(default = "default_strategy")]
    pub strategy: String,
    pub targets: Vec<YamlRouteTarget>,
    #[serde(default)]
    pub access_control: bool,
}

fn default_strategy() -> String {
    "weighted".to_string()
}

#[derive(Debug, Deserialize)]
pub struct YamlRouteTarget {
    pub provider: String,
    pub model: String,
    #[serde(default = "default_weight")]
    pub weight: i32,
    #[serde(default = "default_priority")]
    pub priority: i32,
}

fn default_weight() -> i32 {
    100
}
fn default_priority() -> i32 {
    1
}

impl YamlConfig {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read config file {path}: {e}"))?;
        let config: Self = serde_yaml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("failed to parse YAML config: {e}"))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> anyhow::Result<()> {
        let provider_names: Vec<&str> = self.providers.iter().map(|p| p.name.as_str()).collect();
        for (i, p) in self.providers.iter().enumerate() {
            if p.name.trim().is_empty() {
                anyhow::bail!("providers[{i}]: name is required");
            }
            if p.endpoints.is_empty() {
                anyhow::bail!("providers[{i}] ({}): at least one endpoint is required", p.name);
            }
            if !p.endpoints.contains_key(&p.default_protocol) {
                anyhow::bail!(
                    "providers[{i}] ({}): default_protocol '{}' has no matching endpoint",
                    p.name,
                    p.default_protocol
                );
            }
        }
        for (i, r) in self.routes.iter().enumerate() {
            if r.name.trim().is_empty() {
                anyhow::bail!("routes[{i}]: name is required");
            }
            if r.virtual_model.trim().is_empty() {
                anyhow::bail!("routes[{i}] ({}): virtual_model is required", r.name);
            }
            if r.targets.is_empty() {
                anyhow::bail!("routes[{i}] ({}): at least one target is required", r.name);
            }
            for (j, t) in r.targets.iter().enumerate() {
                if !provider_names.contains(&t.provider.as_str()) {
                    anyhow::bail!(
                        "routes[{i}] ({}): targets[{j}].provider '{}' not found in providers",
                        r.name,
                        t.provider
                    );
                }
            }
        }
        Ok(())
    }
}

impl YamlCacheConfig {
    pub fn to_cache_config(&self) -> CacheConfig {
        let cache_type = match self
            .cache_type
            .as_deref()
            .unwrap_or("response")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "semantic" => CacheType::Semantic,
            _ => CacheType::Response,
        };
        let backend = match self
            .backend
            .as_deref()
            .unwrap_or("in_memory")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "database" => CacheBackendKind::Database,
            _ => CacheBackendKind::InMemory,
        };
        let mode = match self
            .mode
            .as_deref()
            .unwrap_or("default_on")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "default_off" => CacheMode::DefaultOff,
            _ => CacheMode::DefaultOn,
        };
        CacheConfig {
            enabled: self.enabled,
            cache_type,
            backend,
            default_ttl: Duration::from_secs(self.default_ttl_secs.unwrap_or(3600)),
            max_entries: self.max_entries.unwrap_or(1000),
            namespace: self.namespace.clone(),
            mode,
            cache_streaming: true,
            semantic: None,
        }
    }
}

use nyro_core::db::models::{Provider, Route, RouteTarget};

pub fn build_providers(yaml: &YamlConfig) -> Vec<Provider> {
    yaml.providers
        .iter()
        .enumerate()
        .map(|(i, yp)| {
            let id = format!("yaml-provider-{i}");
            let default_ep = yp.endpoints.get(&yp.default_protocol);
            let base_url = default_ep.map(|e| e.base_url.clone()).unwrap_or_default();
            let endpoints_json: HashMap<String, serde_json::Value> = yp
                .endpoints
                .iter()
                .map(|(proto, ep)| {
                    (
                        proto.clone(),
                        serde_json::json!({ "base_url": ep.base_url }),
                    )
                })
                .collect();
            let now = chrono::Utc::now().to_rfc3339();
            Provider {
                id,
                name: yp.name.clone(),
                vendor: None,
                protocol: yp.default_protocol.clone(),
                base_url,
                default_protocol: yp.default_protocol.clone(),
                protocol_endpoints: serde_json::to_string(&endpoints_json).unwrap_or_default(),
                preset_key: None,
                channel: None,
                models_source: yp.models_source.clone(),
                capabilities_source: yp.capabilities_source.clone(),
                static_models: yp.static_models.as_ref().map(|v| v.join("\n")),
                api_key: yp.api_key.clone(),
                use_proxy: yp.use_proxy,
                last_test_success: None,
                last_test_at: None,
                is_active: true,
                created_at: now.clone(),
                updated_at: now,
            }
        })
        .collect()
}

pub fn build_routes(yaml: &YamlConfig, providers: &[Provider]) -> Vec<Route> {
    let name_to_id: HashMap<&str, &str> = providers
        .iter()
        .map(|p| (p.name.as_str(), p.id.as_str()))
        .collect();

    yaml.routes
        .iter()
        .enumerate()
        .map(|(i, yr)| {
            let route_id = format!("yaml-route-{i}");
            let now = chrono::Utc::now().to_rfc3339();

            let targets: Vec<RouteTarget> = yr
                .targets
                .iter()
                .enumerate()
                .map(|(j, yt)| {
                    let provider_id = name_to_id
                        .get(yt.provider.as_str())
                        .unwrap_or(&"")
                        .to_string();
                    RouteTarget {
                        id: format!("{route_id}-target-{j}"),
                        route_id: route_id.clone(),
                        provider_id,
                        model: yt.model.clone(),
                        weight: yt.weight,
                        priority: yt.priority,
                        created_at: now.clone(),
                    }
                })
                .collect();

            let primary = targets.first();
            Route {
                id: route_id,
                name: yr.name.clone(),
                virtual_model: yr.virtual_model.clone(),
                strategy: yr.strategy.clone(),
                target_provider: primary.map(|t| t.provider_id.clone()).unwrap_or_default(),
                target_model: primary.map(|t| t.model.clone()).unwrap_or_default(),
                access_control: yr.access_control,
                route_type: "chat".to_string(),
                cache_enabled: None,
                cache_ttl: None,
                is_active: true,
                created_at: now,
                targets,
            }
        })
        .collect()
}
