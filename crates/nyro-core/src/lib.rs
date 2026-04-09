pub mod admin;
pub mod cache;
pub mod config;
pub mod crypto;
pub mod db;
pub mod logging;
pub mod protocol;
pub mod proxy;
pub mod router;
pub mod storage;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use dashmap::DashMap;
use tokio::sync::mpsc;
use tokio::sync::broadcast;

use config::{
    GatewayConfig, SqlStorageConfig, StorageBackendKind,
};
use logging::LogEntry;
use storage::sql::config::SqlBackendConfig;
use storage::{DynStorage, PostgresStorage, SqliteStorage};
use crate::router::health::HealthRegistry;
use crate::cache::{
    CacheBackend, CacheConfig, CacheStorageKind, DatabaseCacheBackend, InMemoryCacheBackend,
    MemoryVectorStore, VectorStore, VectorStorageKind,
};

#[derive(Clone, Debug)]
pub struct CapabilityCacheEntry {
    pub capabilities: Vec<String>,
    pub cached_at: Instant,
}

#[derive(Clone)]
pub struct Gateway {
    pub config: GatewayConfig,
    pub storage: DynStorage,
    pub http_client: reqwest::Client,
    proxy_client_cache: Arc<tokio::sync::RwLock<Option<ProxyClientCache>>>,
    pub route_cache: Arc<tokio::sync::RwLock<router::RouteCache>>,
    pub health_registry: Arc<HealthRegistry>,
    pub ollama_capability_cache: Arc<tokio::sync::RwLock<HashMap<String, CapabilityCacheEntry>>>,
    pub log_tx: mpsc::Sender<LogEntry>,
    pub runtime_cache_config: Arc<tokio::sync::RwLock<CacheConfig>>,
    pub cache_backend: Arc<tokio::sync::RwLock<Option<Arc<dyn CacheBackend>>>>,
    pub vector_store: Arc<tokio::sync::RwLock<Option<Arc<dyn VectorStore>>>>,
    pub cache_in_flight: Arc<DashMap<String, broadcast::Sender<Vec<u8>>>>,
}

#[derive(Clone)]
struct ProxyClientCache {
    cache_key: String,
    client: reqwest::Client,
}

impl Gateway {
    pub async fn new(config: GatewayConfig) -> anyhow::Result<(Self, mpsc::Receiver<LogEntry>)> {
        let sqlite_storage = if config.storage.sqlite.migrate_on_start {
            SqliteStorage::from_config(&config).await?
        } else {
            let pool = db::init_pool(&config.data_dir).await?;
            SqliteStorage::from_pool(pool)
        };

        let sqlite_fallback: DynStorage = Arc::new(sqlite_storage.clone());

        let storage: DynStorage = match config.storage.backend {
            StorageBackendKind::Sqlite => sqlite_fallback.clone(),
            StorageBackendKind::Postgres => {
                let backend_config = to_sql_backend_config(&config.storage.postgres, "postgres")?;
                Arc::new(PostgresStorage::connect(backend_config, sqlite_fallback.clone()).await?)
            }
        };

        storage.bootstrap().init().await?;
        if !matches!(config.storage.backend, StorageBackendKind::Sqlite) {
            storage.bootstrap().migrate().await?;
        }
        let health = storage.bootstrap().health().await?;
        if !health.can_connect {
            anyhow::bail!("selected storage backend is not reachable");
        }

        Self::from_storage(config, storage).await
    }

    pub async fn from_storage(
        config: GatewayConfig,
        storage: DynStorage,
    ) -> anyhow::Result<(Self, mpsc::Receiver<LogEntry>)> {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()?;

        let route_cache = Arc::new(tokio::sync::RwLock::new(
            router::RouteCache::load(storage.snapshots()).await?,
        ));
        let health_registry = Arc::new(HealthRegistry::new());
        let ollama_capability_cache = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
        let bootstrap_cache = config.cache.clone();

        let (log_tx, log_rx) = mpsc::channel(1024);

        let gw = Self {
            config,
            storage,
            http_client,
            proxy_client_cache: Arc::new(tokio::sync::RwLock::new(None)),
            route_cache,
            health_registry,
            ollama_capability_cache,
            log_tx,
            runtime_cache_config: Arc::new(tokio::sync::RwLock::new(bootstrap_cache)),
            cache_backend: Arc::new(tokio::sync::RwLock::new(None)),
            vector_store: Arc::new(tokio::sync::RwLock::new(None)),
            cache_in_flight: Arc::new(DashMap::new()),
        };

        let runtime_cache = gw
            .storage
            .settings()
            .get("cache_settings")
            .await?
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .and_then(|value| CacheConfig::from_admin_json(&value))
            .unwrap_or_else(|| gw.config.cache.clone());
        gw.reload_cache_runtime(runtime_cache).await?;

        {
            let data_dir = gw.config.data_dir.clone();
            let http_client = gw.http_client.clone();
            tokio::spawn(async move {
                admin::refresh_models_dev_runtime_cache_on_startup(data_dir, http_client).await;
            });
        }

        // Memory vector store is ephemeral across restarts; no fingerprint check needed.

        Ok((gw, log_rx))
    }

    pub async fn effective_cache_config(&self) -> CacheConfig {
        self.runtime_cache_config.read().await.clone()
    }

    pub async fn reload_cache_runtime(&self, mut next: CacheConfig) -> anyhow::Result<()> {
        let current = self.runtime_cache_config.read().await.clone();

        let exact_needs_rebuild = current.exact.enabled != next.exact.enabled
            || current.exact.storage != next.exact.storage
            || current.exact.max_entries != next.exact.max_entries;
        let next_cache_backend: Option<Arc<dyn CacheBackend>> = if exact_needs_rebuild {
            if next.exact.enabled {
                match next.exact.storage {
                    CacheStorageKind::Memory => {
                        Some(Arc::new(InMemoryCacheBackend::new(next.exact.max_entries)))
                    }
                    CacheStorageKind::Database => {
                        Some(Arc::new(DatabaseCacheBackend::new(self.storage.clone())))
                    }
                }
            } else {
                None
            }
        } else {
            self.cache_backend.read().await.clone()
        };

        let semantic_needs_rebuild = current.semantic.enabled != next.semantic.enabled
            || current.semantic.storage != next.semantic.storage
            || current.semantic.max_entries != next.semantic.max_entries
            || current.semantic.embedding_route != next.semantic.embedding_route
            || current.semantic.vector_dimensions != next.semantic.vector_dimensions;
        let next_vector_store: Option<Arc<dyn VectorStore>> = if semantic_needs_rebuild {
            if next.semantic.enabled {
                let embedding_route = next.semantic.embedding_route.trim();
                if embedding_route.is_empty() {
                    tracing::warn!(
                        "semantic cache enabled but embedding_route is empty; semantic cache disabled"
                    );
                    next.semantic.enabled = false;
                    None
                } else {
                    let route_valid = {
                        let route_cache = self.route_cache.read().await;
                        route_cache
                            .match_route(embedding_route)
                            .map(|route| route.is_embedding_route())
                            .unwrap_or(false)
                    };
                    if !route_valid {
                        tracing::warn!(
                            "semantic cache embedding route '{}' not found or not type=embedding; semantic cache disabled",
                            embedding_route
                        );
                        next.semantic.enabled = false;
                        None
                    } else {
                        match next.semantic.storage {
                            VectorStorageKind::Memory => {
                                Some(Arc::new(MemoryVectorStore::new(next.semantic.max_entries)))
                            }
                        }
                    }
                }
            } else {
                None
            }
        } else {
            self.vector_store.read().await.clone()
        };

        *self.cache_backend.write().await = next_cache_backend;
        *self.vector_store.write().await = next_vector_store;
        *self.runtime_cache_config.write().await = next;
        Ok(())
    }

    pub async fn start_proxy(&self) -> anyhow::Result<()> {
        let router = proxy::server::create_router(self.clone());
        let addr = format!("{}:{}", self.config.proxy_host, self.config.proxy_port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        tracing::info!("proxy listening on {}", addr);
        axum::serve(listener, router).await?;
        Ok(())
    }

    pub fn admin(&self) -> admin::AdminService {
        admin::AdminService::new(self.clone())
    }

    pub async fn http_client_for_provider(&self, use_proxy: bool) -> anyhow::Result<reqwest::Client> {
        if !use_proxy {
            return Ok(self.http_client.clone());
        }

        let enabled = self
            .storage
            .settings()
            .get("proxy_enabled")
            .await?
            .as_deref()
            .map(parse_bool_setting)
            .unwrap_or(false);
        if !enabled {
            anyhow::bail!("proxy is disabled in settings");
        }

        let proxy_url = self
            .storage
            .settings()
            .get("proxy_url")
            .await?
            .unwrap_or_default()
            .trim()
            .to_string();
        if proxy_url.is_empty() {
            anyhow::bail!("proxy_url is empty");
        }

        let force_http1 = self
            .storage
            .settings()
            .get("proxy_force_http1")
            .await?
            .as_deref()
            .map(parse_bool_setting)
            .unwrap_or(false);

        let cache_key = format!("{proxy_url}|{force_http1}");
        if let Some(cached) = self.proxy_client_cache.read().await.clone() {
            if cached.cache_key == cache_key {
                return Ok(cached.client);
            }
        }

        let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(300));
        if force_http1 {
            builder = builder.http1_only();
        }
        let client = builder.proxy(reqwest::Proxy::all(&proxy_url)?).build()?;

        *self.proxy_client_cache.write().await = Some(ProxyClientCache {
            cache_key,
            client: client.clone(),
        });
        Ok(client)
    }

    pub async fn get_ollama_capabilities_cached(
        &self,
        provider_id: &str,
        model: &str,
        ttl: Duration,
    ) -> Option<Vec<String>> {
        let key = format!("{provider_id}:{model}");
        let cache = self.ollama_capability_cache.read().await;
        cache.get(&key).and_then(|entry| {
            if entry.cached_at.elapsed() < ttl {
                Some(entry.capabilities.clone())
            } else {
                None
            }
        })
    }

    pub async fn set_ollama_capabilities_cache(
        &self,
        provider_id: &str,
        model: &str,
        capabilities: Vec<String>,
    ) {
        let key = format!("{provider_id}:{model}");
        let mut cache = self.ollama_capability_cache.write().await;
        cache.insert(
            key,
            CapabilityCacheEntry {
                capabilities,
                cached_at: Instant::now(),
            },
        );
    }

    pub async fn clear_ollama_capability_cache_for_provider(&self, provider_id: &str) {
        let prefix = format!("{provider_id}:");
        let mut cache = self.ollama_capability_cache.write().await;
        cache.retain(|k, _| !k.starts_with(&prefix));
    }
}

fn parse_bool_setting(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

fn to_sql_backend_config(config: &SqlStorageConfig, backend: &str) -> anyhow::Result<SqlBackendConfig> {
    let url = config
        .configured_url()
        .with_context(|| format!("{backend} backend selected but storage url is empty"))?;
    Ok(SqlBackendConfig {
        url,
        max_connections: config.max_connections,
        min_connections: config.min_connections,
        acquire_timeout: config.acquire_timeout,
        idle_timeout: config.idle_timeout,
        max_lifetime: config.max_lifetime,
    })
}
