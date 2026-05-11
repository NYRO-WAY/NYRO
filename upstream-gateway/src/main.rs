use anyhow::Context;
use tokio::net::TcpListener;
use tracing::info;
use upstream_gateway::app::AppState;
use upstream_gateway::config::AppConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = AppConfig::from_env();
    let state = AppState::new(config.clone()).await?;
    let proxy_router = upstream_gateway::app::build_proxy_router(state.clone());

    let proxy_listener = TcpListener::bind(&config.proxy_bind_addr)
        .await
        .with_context(|| format!("failed to bind proxy {}", config.proxy_bind_addr))?;

    info!(
        "upstream-gateway proxy listening on {}",
        config.proxy_bind_addr
    );

    let proxy_server = async move {
        axum::serve(proxy_listener, proxy_router)
            .await
            .context("proxy server exited unexpectedly")
    };

    if let Some(admin_bind_addr) = &config.admin_bind_addr {
        let admin_router = upstream_gateway::app::build_admin_router(state);
        let admin_listener = TcpListener::bind(admin_bind_addr)
            .await
            .with_context(|| format!("failed to bind admin {}", admin_bind_addr))?;

        info!("upstream-gateway admin listening on {}", admin_bind_addr);

        let admin_server = async move {
            axum::serve(admin_listener, admin_router)
                .await
                .context("admin server exited unexpectedly")
        };

        tokio::try_join!(proxy_server, admin_server)?;
    } else {
        info!("upstream-gateway admin listener disabled");
        proxy_server.await?;
    }

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
