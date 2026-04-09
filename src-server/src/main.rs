use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use axum::http::{HeaderValue, Method, header};
use nyro_core::{
    Gateway,
    cache::{
        CacheConfig, CacheStorageKind, ExactCacheConfig, SemanticCacheConfig, VectorStorageKind,
    },
    config::{
        GatewayConfig, GatewayStorageConfig, SqlStorageConfig, SqliteStorageConfig,
        StorageBackendKind,
    },
    logging,
    storage::MemoryStorage,
};
use tower_http::cors::{AllowOrigin, CorsLayer};

mod admin_routes;
mod yaml_config;

#[derive(Parser)]
#[command(name = "nyro-server", about = "Nyro AI Gateway — Server Mode")]
struct Args {
    #[arg(long, default_value = "127.0.0.1")]
    proxy_host: String,

    #[arg(long, default_value = "19530")]
    proxy_port: u16,

    #[arg(long, default_value = "127.0.0.1")]
    admin_host: String,

    #[arg(long, default_value = "19531")]
    admin_port: u16,

    #[arg(long, default_value = "~/.nyro")]
    data_dir: String,

    #[arg(long, help = "Bearer token for admin API authentication")]
    admin_key: Option<String>,

    #[arg(
        long = "admin-cors-origin",
        action = clap::ArgAction::Append,
        help = "Allowed CORS origin for admin API (repeatable, use '*' for any)"
    )]
    admin_cors_origins: Vec<String>,

    #[arg(
        long = "proxy-cors-origin",
        action = clap::ArgAction::Append,
        help = "Allowed CORS origin for proxy API (repeatable, use '*' for any)"
    )]
    proxy_cors_origins: Vec<String>,

    #[arg(long, default_value = "./webui/dist", help = "Path to webui static files")]
    webui_dir: String,

    #[arg(long, value_parser = ["sqlite", "postgres"], default_value = "sqlite")]
    storage_backend: String,

    #[arg(
        long,
        default_value = "NYRO_STORAGE_DSN",
        help = "Environment variable name used to load storage DSN/URI"
    )]
    storage_dsn_env: String,

    #[arg(
        long,
        default_value = "true",
        action = clap::ArgAction::Set,
        help = "Run SQLite migrations on startup (true/false)"
    )]
    sqlite_migrate_on_start: bool,

    #[arg(long, default_value_t = 10)]
    storage_max_connections: u32,

    #[arg(long, default_value_t = 1)]
    storage_min_connections: u32,

    #[arg(long, default_value_t = 10)]
    storage_acquire_timeout_secs: u64,

    #[arg(long, help = "Idle timeout in seconds for SQL backends")]
    storage_idle_timeout_secs: Option<u64>,

    #[arg(long, help = "Max lifetime in seconds for SQL backends")]
    storage_max_lifetime_secs: Option<u64>,

    #[arg(
        long = "config",
        short = 'c',
        help = "Path to YAML config file for standalone mode (no DB, no admin API, no WebUI)"
    )]
    config_file: Option<String>,

    #[arg(long, default_value_t = false)]
    cache_exact_enabled: bool,
    #[arg(long, default_value = "memory")]
    cache_exact_storage: String,
    #[arg(long, default_value_t = 3600)]
    cache_exact_ttl: u64,
    #[arg(long, default_value_t = 1000)]
    cache_exact_max_entries: usize,

    #[arg(long, default_value_t = false)]
    cache_semantic_enabled: bool,
    #[arg(long, default_value = "memory")]
    cache_semantic_storage: String,
    #[arg(long, default_value = "")]
    cache_semantic_route: String,
    #[arg(long, default_value_t = 0.92)]
    cache_semantic_threshold: f64,
    #[arg(long, default_value_t = 1536)]
    cache_semantic_dimensions: usize,
    #[arg(long, default_value_t = 600)]
    cache_semantic_ttl: u64,
    #[arg(long, default_value_t = 500)]
    cache_semantic_max_entries: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("nyro=debug,tower_http=debug")
        .init();

    let args = Args::parse();

    if let Some(ref config_path) = args.config_file {
        return run_standalone(config_path, &args).await;
    }

    run_full(&args).await
}

async fn run_standalone(config_path: &str, args: &Args) -> anyhow::Result<()> {
    tracing::info!("standalone mode: loading {config_path}");
    let yaml = yaml_config::YamlConfig::load(config_path)?;

    let proxy_host = if args.proxy_host != "127.0.0.1" {
        args.proxy_host.clone()
    } else {
        yaml.server.proxy_host.clone()
    };
    let proxy_port = if args.proxy_port != 19530 {
        args.proxy_port
    } else {
        yaml.server.proxy_port
    };

    let providers = yaml_config::build_providers(&yaml);
    let routes = yaml_config::build_routes(&yaml, &providers);
    let settings: Vec<(String, String)> = yaml.settings.into_iter().collect();

    tracing::info!(
        "loaded {} providers, {} routes from YAML",
        providers.len(),
        routes.len()
    );

    let storage: nyro_core::storage::DynStorage =
        Arc::new(MemoryStorage::new(providers, routes, settings));

    let data_dir = shellexpand::tilde(&args.data_dir).to_string();
    let proxy_cors_origins = if args.proxy_cors_origins.is_empty() {
        default_local_origins(&[proxy_port])
    } else {
        args.proxy_cors_origins.clone()
    };

    let config = GatewayConfig {
        proxy_host: proxy_host.clone(),
        proxy_port,
        proxy_cors_origins,
        data_dir: PathBuf::from(data_dir),
        storage: GatewayStorageConfig::default(),
        cache: yaml.cache.to_cache_config(),
        ..Default::default()
    };

    let (gateway, log_rx) = Gateway::from_storage(config, storage).await?;
    let storage_for_logs = gateway.storage.clone();

    tokio::spawn(async move {
        logging::run_collector(log_rx, storage_for_logs).await;
    });

    let proxy_addr = format!("{proxy_host}:{proxy_port}");
    tracing::info!("proxy  → http://{proxy_addr}");
    tracing::info!("standalone mode: admin API and WebUI are disabled");

    gateway.start_proxy().await?;
    Ok(())
}

async fn run_full(args: &Args) -> anyhow::Result<()> {
    let data_dir = shellexpand::tilde(&args.data_dir).to_string();
    let admin_key = args.admin_key.clone().filter(|k| !k.trim().is_empty());

    if !is_loopback_host(&args.admin_host) && admin_key.is_none() {
        anyhow::bail!(
            "--admin-key is required when --admin-host is not loopback (localhost/127.0.0.1/::1)"
        );
    }
    let admin_cors_origins = if args.admin_cors_origins.is_empty() {
        default_local_origins(&[args.admin_port])
    } else {
        args.admin_cors_origins.clone()
    };
    let proxy_cors_origins = if args.proxy_cors_origins.is_empty() {
        default_local_origins(&[args.proxy_port, args.admin_port])
    } else {
        args.proxy_cors_origins.clone()
    };

    let config = GatewayConfig {
        proxy_host: args.proxy_host.clone(),
        proxy_port: args.proxy_port,
        proxy_cors_origins,
        data_dir: PathBuf::from(data_dir),
        storage: build_storage_config(args)?,
        cache: build_cache_config_from_args(args)?,
        ..Default::default()
    };

    let (gateway, log_rx) = Gateway::new(config).await?;

    let gw_proxy = gateway.clone();
    let storage_for_logs = gateway.storage.clone();

    tokio::spawn(async move {
        if let Err(e) = gw_proxy.start_proxy().await {
            tracing::error!("proxy server error: {e}");
        }
    });

    tokio::spawn(async move {
        logging::run_collector(log_rx, storage_for_logs).await;
    });

    let admin_router = admin_routes::create_router(gateway, admin_key.clone());

    let index_path = std::path::Path::new(&args.webui_dir).join("index.html");
    let webui_service = tower_http::services::ServeDir::new(&args.webui_dir)
        .fallback(tower_http::services::ServeFile::new(index_path));

    let app = admin_router
        .fallback_service(webui_service)
        .layer(build_cors_layer(&admin_cors_origins));

    let admin_addr = format!("{}:{}", args.admin_host, args.admin_port);
    let listener = tokio::net::TcpListener::bind(&admin_addr).await?;

    let proxy_bind_addr = format!("{}:{}", args.proxy_host, args.proxy_port);
    tracing::info!("proxy  → http://{proxy_bind_addr}");
    tracing::info!("webui  → http://{admin_addr}");

    if admin_key.is_none() {
        tracing::warn!("admin API auth disabled: set --admin-key for production");
    }
    axum::serve(listener, app).await?;
    Ok(())
}

fn build_cache_config_from_args(args: &Args) -> anyhow::Result<CacheConfig> {
    let exact_storage = match args.cache_exact_storage.trim().to_ascii_lowercase().as_str() {
        "memory" | "in_memory" | "inmemory" => CacheStorageKind::Memory,
        "database" => CacheStorageKind::Database,
        other => anyhow::bail!("unsupported exact cache storage: {other}"),
    };
    let semantic_storage = match args
        .cache_semantic_storage
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "memory" => VectorStorageKind::Memory,
        other => anyhow::bail!("unsupported semantic cache storage: {other}"),
    };
    Ok(CacheConfig {
        exact: ExactCacheConfig {
            enabled: args.cache_exact_enabled,
            storage: exact_storage,
            default_ttl: Duration::from_secs(args.cache_exact_ttl.max(1)),
            max_entries: args.cache_exact_max_entries.max(1),
        },
        semantic: SemanticCacheConfig {
            enabled: args.cache_semantic_enabled,
            storage: semantic_storage,
            embedding_route: args.cache_semantic_route.trim().to_string(),
            similarity_threshold: args.cache_semantic_threshold,
            vector_dimensions: args.cache_semantic_dimensions.max(1),
            default_ttl: Duration::from_secs(args.cache_semantic_ttl.max(1)),
            max_entries: args.cache_semantic_max_entries.max(1),
        },
    })
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

fn default_local_origins(ports: &[u16]) -> Vec<String> {
    let mut origins = vec!["tauri://localhost".to_string(), "http://tauri.localhost".to_string()];
    for port in ports {
        origins.push(format!("http://127.0.0.1:{port}"));
        origins.push(format!("http://localhost:{port}"));
    }
    origins
}

fn parse_allow_origin(origins: &[String]) -> AllowOrigin {
    if origins.iter().any(|o| o.trim() == "*") {
        return AllowOrigin::any();
    }

    let values = origins
        .iter()
        .filter_map(|o| HeaderValue::from_str(o.trim()).ok())
        .collect::<Vec<_>>();

    if values.is_empty() {
        AllowOrigin::any()
    } else {
        AllowOrigin::list(values)
    }
}

fn build_cors_layer(origins: &[String]) -> CorsLayer {
    CorsLayer::new()
        .allow_origin(parse_allow_origin(origins))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ACCEPT,
            header::HeaderName::from_static("x-api-key"),
            header::HeaderName::from_static("anthropic-version"),
        ])
}

fn build_storage_config(args: &Args) -> anyhow::Result<GatewayStorageConfig> {
    let backend = parse_storage_backend(&args.storage_backend)?;
    let storage_dsn = resolve_storage_dsn(args, backend)?;
    let sql = SqlStorageConfig {
        url: storage_dsn.clone(),
        max_connections: args.storage_max_connections,
        min_connections: args.storage_min_connections,
        acquire_timeout: Duration::from_secs(args.storage_acquire_timeout_secs),
        idle_timeout: args.storage_idle_timeout_secs.map(Duration::from_secs),
        max_lifetime: args.storage_max_lifetime_secs.map(Duration::from_secs),
    };

    Ok(GatewayStorageConfig {
        backend,
        sqlite: SqliteStorageConfig {
            migrate_on_start: args.sqlite_migrate_on_start,
        },
        postgres: sql,
    })
}

fn parse_storage_backend(value: &str) -> anyhow::Result<StorageBackendKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "sqlite" => Ok(StorageBackendKind::Sqlite),
        "postgres" => Ok(StorageBackendKind::Postgres),
        other => anyhow::bail!("unsupported storage backend: {other}"),
    }
}

fn resolve_storage_dsn(
    args: &Args,
    backend: StorageBackendKind,
) -> anyhow::Result<Option<String>> {
    if matches!(backend, StorageBackendKind::Sqlite) {
        return Ok(None);
    }

    let env_name = args.storage_dsn_env.trim();
    if env_name.is_empty() {
        anyhow::bail!("--storage-dsn-env cannot be empty");
    }

    std::env::var(env_name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(Some)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "storage backend {} requires env {}",
                args.storage_backend,
                env_name
            )
        })
}
