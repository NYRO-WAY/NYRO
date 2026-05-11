use anyhow::Context;
use tokio::net::TcpListener;
use tracing::info;

use race_gateway::app::{AppState, build_admin_router, build_proxy_router};
use race_gateway::config::AppConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = AppConfig::from_env();
    let state = AppState::new(config.clone()).await?;
    let proxy_router = build_proxy_router(state.clone());
    let admin_router = build_admin_router(state);

    let proxy_listener = TcpListener::bind(&config.proxy_bind_addr)
        .await
        .with_context(|| format!("failed to bind proxy {}", config.proxy_bind_addr))?;
    let admin_listener = TcpListener::bind(&config.admin_bind_addr)
        .await
        .with_context(|| format!("failed to bind admin {}", config.admin_bind_addr))?;

    info!("race-gateway proxy listening on {}", config.proxy_bind_addr);
    info!("race-gateway admin listening on {}", config.admin_bind_addr);

    let proxy_server = async move {
        axum::serve(proxy_listener, proxy_router)
            .await
            .context("proxy server exited unexpectedly")
    };
    let admin_server = async move {
        axum::serve(admin_listener, admin_router)
            .await
            .context("admin server exited unexpectedly")
    };

    tokio::try_join!(proxy_server, admin_server)?;

    Ok(())
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();
}
