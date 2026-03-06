pub mod admin;
pub mod config;
pub mod crypto;
pub mod db;
pub mod logging;
pub mod protocol;
pub mod proxy;
pub mod router;

use std::sync::Arc;

use sqlx::SqlitePool;
use tokio::sync::mpsc;

use config::GatewayConfig;
use logging::LogEntry;

#[derive(Clone)]
pub struct Gateway {
    pub config: GatewayConfig,
    pub db: SqlitePool,
    pub http_client: reqwest::Client,
    pub route_cache: Arc<tokio::sync::RwLock<router::RouteCache>>,
    pub log_tx: mpsc::Sender<LogEntry>,
}

impl Gateway {
    pub async fn new(config: GatewayConfig) -> anyhow::Result<(Self, mpsc::Receiver<LogEntry>)> {
        let db = db::init_pool(&config.data_dir).await?;
        db::migrate(&db).await?;

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()?;

        let route_cache = Arc::new(tokio::sync::RwLock::new(
            router::RouteCache::load(&db).await?,
        ));

        let (log_tx, log_rx) = mpsc::channel(1024);

        let gw = Self {
            config,
            db,
            http_client,
            route_cache,
            log_tx,
        };

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
}
