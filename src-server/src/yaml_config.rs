use nyro_core::cache::{CacheConfig, ExactCacheConfig, SemanticCacheConfig};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

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
    pub exact: YamlExactCacheConfig,
    #[serde(default)]
    pub semantic: YamlSemanticCacheConfig,
}

#[derive(Debug, Deserialize, Default)]
pub struct YamlExactCacheConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub default_ttl: Option<u64>,
    #[serde(default)]
    pub max_entries: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub struct YamlSemanticCacheConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub embedding_route: Option<String>,
    #[serde(default)]
    pub similarity_threshold: Option<f64>,
    #[serde(default)]
    pub vector_dimensions: Option<usize>,
    #[serde(default)]
    pub default_ttl: Option<u64>,
    #[serde(default)]
    pub max_entries: Option<usize>,
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
    #[serde(alias = "vmodel")]
    pub virtual_model: String,
    #[serde(default = "default_strategy")]
    pub strategy: String,
    #[serde(default = "default_route_type", alias = "type")]
    pub route_type: String,
    pub targets: Vec<YamlRouteTarget>,
    #[serde(default)]
    pub access_control: bool,
}

fn default_strategy() -> String {
    "weighted".to_string()
}

fn default_route_type() -> String {
    "chat".to_string()
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
                anyhow::bail!(
                    "providers[{i}] ({}): at least one endpoint is required",
                    p.name
                );
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
            parse_route_type(&r.route_type).map_err(|_| {
                anyhow::anyhow!(
                    "routes[{i}] ({}): unsupported route type '{}', expected chat|embedding",
                    r.name,
                    r.route_type
                )
            })?;
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
        CacheConfig {
            exact: ExactCacheConfig {
                enabled: self.exact.enabled,
                default_ttl: Duration::from_secs(self.exact.default_ttl.unwrap_or(3600)),
                max_entries: self.exact.max_entries.unwrap_or(1000),
                stream_replay_tps: 100,
                expose_headers: true,
            },
            semantic: SemanticCacheConfig {
                enabled: self.semantic.enabled,
                embedding_route: self.semantic.embedding_route.clone().unwrap_or_default(),
                similarity_threshold: self.semantic.similarity_threshold.unwrap_or(0.92),
                vector_dimensions: self.semantic.vector_dimensions.unwrap_or(1536),
                default_ttl: Duration::from_secs(self.semantic.default_ttl.unwrap_or(600)),
                max_entries: self.semantic.max_entries.unwrap_or(500),
                stream_replay_tps: 100,
                expose_headers: true,
            },
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
                is_enabled: true,
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
                route_type: parse_route_type(&yr.route_type)
                    .unwrap_or("chat")
                    .to_string(),
                cache_exact_ttl: None,
                cache_semantic_ttl: None,
                cache_semantic_threshold: None,
                cache: None,
                is_enabled: true,
                created_at: now,
                targets,
            }
        })
        .collect()
}

fn parse_route_type(raw: &str) -> anyhow::Result<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "chat" => Ok("chat"),
        "embedding" => Ok("embedding"),
        _ => anyhow::bail!("unsupported route type"),
    }
}
