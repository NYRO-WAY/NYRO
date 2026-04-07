use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use axum::http::{HeaderValue, Method, header};
use nyro_core::{
    Gateway,
    cache::{CacheBackendKind, CacheConfig, CacheMode, CacheType},
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

    #[arg(long, value_parser = ["sqlite", "postgres", "mysql"], default_value = "sqlite")]
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
    cache_enabled: bool,
    #[arg(long, default_value = "response")]
    cache_type: String,
    #[arg(long, default_value = "in_memory")]
    cache_backend: String,
    #[arg(long, default_value_t = 3600)]
    cache_ttl_secs: u64,
    #[arg(long, default_value_t = 1000)]
    cache_max_entries: usize,
    #[arg(long)]
    cache_namespace: Option<String>,
    #[arg(long, default_value = "default_on")]
    cache_mode: String,
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
        cache: if args.cache_enabled {
            build_cache_config_from_args(args)?
        } else {
            yaml.cache.to_cache_config()
        },
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
    let cache_type = match args.cache_type.trim().to_ascii_lowercase().as_str() {
        "semantic" => CacheType::Semantic,
        "response" => CacheType::Response,
        other => anyhow::bail!("unsupported cache type: {other}"),
    };
    let backend = match args.cache_backend.trim().to_ascii_lowercase().as_str() {
        "database" => CacheBackendKind::Database,
        "in_memory" | "inmemory" => CacheBackendKind::InMemory,
        other => anyhow::bail!("unsupported cache backend: {other}"),
    };
    let mode = match args.cache_mode.trim().to_ascii_lowercase().as_str() {
        "default_off" => CacheMode::DefaultOff,
        "default_on" => CacheMode::DefaultOn,
        other => anyhow::bail!("unsupported cache mode: {other}"),
    };
    Ok(CacheConfig {
        enabled: args.cache_enabled,
        cache_type,
        backend,
        default_ttl: Duration::from_secs(args.cache_ttl_secs.max(1)),
        max_entries: args.cache_max_entries.max(1),
        namespace: args.cache_namespace.clone(),
        mode,
        cache_streaming: true,
        semantic: None,
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
        postgres: sql.clone(),
        mysql: sql,
    })
}

fn parse_storage_backend(value: &str) -> anyhow::Result<StorageBackendKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "sqlite" => Ok(StorageBackendKind::Sqlite),
        "postgres" => Ok(StorageBackendKind::Postgres),
        "mysql" => Ok(StorageBackendKind::MySql),
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
