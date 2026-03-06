use std::path::PathBuf;

use clap::Parser;
use nyro_core::{Gateway, config::GatewayConfig, logging};

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

    #[arg(long, help = "Bearer token for proxy API authentication")]
    proxy_key: Option<String>,

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
    let proxy_key = args.proxy_key.filter(|k| !k.trim().is_empty());

    if !is_loopback_host(&args.admin_host) && admin_key.is_none() {
        anyhow::bail!(
            "--admin-key is required when --admin-host is not loopback (localhost/127.0.0.1/::1)"
        );
    }

    let config = GatewayConfig {
        proxy_host: args.proxy_host.clone(),
        proxy_port: args.proxy_port,
        data_dir: PathBuf::from(data_dir),
        auth_key: proxy_key.clone(),
        ..Default::default()
    };

    let (gateway, log_rx) = Gateway::new(config).await?;

    let gw_proxy = gateway.clone();
    let db_for_logs = gateway.db.clone();

    tokio::spawn(async move {
        if let Err(e) = gw_proxy.start_proxy().await {
            tracing::error!("proxy server error: {e}");
        }
    });

    tokio::spawn(async move {
        logging::run_collector(log_rx, db_for_logs).await;
    });

    let admin_router = admin_routes::create_router(gateway, admin_key.clone());

    let index_path = std::path::Path::new(&args.webui_dir).join("index.html");
    let webui_service = tower_http::services::ServeDir::new(&args.webui_dir)
        .fallback(tower_http::services::ServeFile::new(index_path));

    let app = admin_router
        .fallback_service(webui_service)
        .layer(tower_http::cors::CorsLayer::permissive());

    let admin_addr = format!("{}:{}", args.admin_host, args.admin_port);
    let listener = tokio::net::TcpListener::bind(&admin_addr).await?;

    let proxy_bind_addr = format!("{}:{}", args.proxy_host, args.proxy_port);
    tracing::info!("proxy  → http://{proxy_bind_addr}");
    tracing::info!("webui  → http://{admin_addr}");

    if admin_key.is_none() {
        tracing::warn!("admin API auth disabled: set --admin-key for production");
    }
    if proxy_key.is_none() {
        tracing::warn!("proxy API auth disabled: set --proxy-key for production");
    }

    axum::serve(listener, app).await?;
    Ok(())
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}
