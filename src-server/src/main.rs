use std::path::PathBuf;

use clap::Parser;
use axum::http::{HeaderValue, Method, header};
use nyro_core::{Gateway, config::GatewayConfig, logging};
use tower_http::cors::{AllowOrigin, CorsLayer};

mod admin_routes;

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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("nyro=debug,tower_http=debug")
        .init();

    let args = Args::parse();

    let data_dir = shellexpand::tilde(&args.data_dir).to_string();
    let admin_key = args.admin_key.filter(|k| !k.trim().is_empty());

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
