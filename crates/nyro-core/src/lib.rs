pub mod admin;
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
use sqlx::SqlitePool;
use tokio::sync::mpsc;

use config::{
    GatewayConfig, MongoCollectionNames as GatewayMongoCollectionNames,
    MongoStorageConfig as GatewayMongoStorageConfig, SqlStorageConfig,
    StorageBackendKind,
};
use logging::LogEntry;
use storage::mongo::{
    MongoCollectionNames as RuntimeMongoCollectionNames, MongoStorageConfig as RuntimeMongoStorageConfig,
};
use storage::sql::config::SqlBackendConfig;
use storage::{DynStorage, MongoStorage, MySqlStorage, PostgresStorage, SqliteStorage};

#[derive(Clone, Debug)]
pub struct CapabilityCacheEntry {
    pub capabilities: Vec<String>,
    pub cached_at: Instant,
}

#[derive(Clone)]
pub struct Gateway {
    pub config: GatewayConfig,
    pub db: SqlitePool,
    pub storage: DynStorage,
    pub http_client: reqwest::Client,
    pub route_cache: Arc<tokio::sync::RwLock<router::RouteCache>>,
    pub ollama_capability_cache: Arc<tokio::sync::RwLock<HashMap<String, CapabilityCacheEntry>>>,
    pub log_tx: mpsc::Sender<LogEntry>,
}

impl Gateway {
    pub async fn new(config: GatewayConfig) -> anyhow::Result<(Self, mpsc::Receiver<LogEntry>)> {
        let sqlite_storage = if config.storage.sqlite.migrate_on_start {
            SqliteStorage::from_config(&config).await?
        } else {
            let pool = db::init_pool(&config.data_dir).await?;
            SqliteStorage::from_pool(pool)
        };

        let db = sqlite_storage.pool().clone();
        let sqlite_fallback: DynStorage = Arc::new(sqlite_storage.clone());

        let storage: DynStorage = match config.storage.backend {
            StorageBackendKind::Sqlite => sqlite_fallback.clone(),
            StorageBackendKind::Postgres => {
                let backend_config = to_sql_backend_config(&config.storage.postgres, "postgres")?;
                Arc::new(PostgresStorage::connect(backend_config, sqlite_fallback.clone()).await?)
            }
            StorageBackendKind::MySql => {
                let backend_config = to_sql_backend_config(&config.storage.mysql, "mysql")?;
                Arc::new(MySqlStorage::connect(backend_config, sqlite_fallback.clone()).await?)
            }
            StorageBackendKind::Mongo => {
                let backend_config = to_mongo_backend_config(&config.storage.mongo)?;
                Arc::new(MongoStorage::connect(backend_config, sqlite_fallback.clone()).await?)
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

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()?;

        let route_cache = Arc::new(tokio::sync::RwLock::new(
            router::RouteCache::load(storage.snapshots()).await?,
        ));
        let ollama_capability_cache = Arc::new(tokio::sync::RwLock::new(HashMap::new()));

        let (log_tx, log_rx) = mpsc::channel(1024);

        let gw = Self {
            config,
            db,
            storage,
            http_client,
            route_cache,
            ollama_capability_cache,
            log_tx,
        };

        {
            let data_dir = gw.config.data_dir.clone();
            let http_client = gw.http_client.clone();
            tokio::spawn(async move {
                admin::refresh_models_dev_runtime_cache_on_startup(data_dir, http_client).await;
            });
        }

        Ok((gw, log_rx))
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

fn to_mongo_backend_config(config: &GatewayMongoStorageConfig) -> anyhow::Result<RuntimeMongoStorageConfig> {
    let uri = config
        .configured_uri()
        .context("mongo backend selected but storage uri is empty")?;

    Ok(RuntimeMongoStorageConfig {
        uri,
        database: config.database.trim().to_string(),
        collections: to_runtime_mongo_collections(&config.collections),
    })
}

fn to_runtime_mongo_collections(collections: &GatewayMongoCollectionNames) -> RuntimeMongoCollectionNames {
    RuntimeMongoCollectionNames {
        providers: collections.providers.clone(),
        routes: collections.routes.clone(),
        api_keys: collections.api_keys.clone(),
        api_key_routes: collections.api_key_routes.clone(),
        request_logs: collections.request_logs.clone(),
        settings: collections.settings.clone(),
    }
}
