mod cache;

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use axum::{
    Router,
    extract::{MatchedPath, Request, State},
    middleware::{self, Next},
    response::Response,
};
use tower_http::trace::TraceLayer;

use crate::config::AppConfig;
use crate::downstream::ReqwestDownstreamDispatcher;
use crate::observability::Observability;
use crate::runtime::RuntimeRegistry;
use crate::storage::{SharedRaceConfigStore, SqliteRaceConfigStore, load_bootstrap_data};

pub use cache::ConfigSnapshotCache;

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub store: SharedRaceConfigStore,
    pub config_cache: ConfigSnapshotCache,
    pub runtime: RuntimeRegistry,
    pub dispatcher: ReqwestDownstreamDispatcher,
    pub observability: Observability,
}

impl AppState {
    pub async fn new(config: AppConfig) -> anyhow::Result<Self> {
        let sqlite_store = SqliteRaceConfigStore::connect(&config.database_url).await?;
        if let Some(path) = &config.bootstrap_json_path {
            let data = load_bootstrap_data(Path::new(path))
                .with_context(|| format!("failed to load bootstrap data from {path}"))?;
            let seeded = sqlite_store.seed_if_empty(data).await?;
            if seeded {
                tracing::info!("seeded sqlite config store from {}", path);
            }
        }

        let store: SharedRaceConfigStore = Arc::new(sqlite_store);
        let config_cache = ConfigSnapshotCache::load_from_store(store.as_ref()).await?;

        Ok(Self {
            config,
            store,
            config_cache,
            runtime: RuntimeRegistry::new(),
            dispatcher: ReqwestDownstreamDispatcher::new(),
            observability: Observability::new()?,
        })
    }
}

pub fn build_proxy_router(state: AppState) -> Router {
    let metrics_state = HttpMetricsState::new(state.observability.clone(), "proxy");
    crate::data_plane::router(state)
        .layer(middleware::from_fn_with_state(
            metrics_state,
            track_http_metrics,
        ))
        .layer(TraceLayer::new_for_http())
}

pub fn build_admin_router(state: AppState) -> Router {
    let metrics_state = HttpMetricsState::new(state.observability.clone(), "admin");
    crate::control_plane::router(state)
        .layer(middleware::from_fn_with_state(
            metrics_state,
            track_http_metrics,
        ))
        .layer(TraceLayer::new_for_http())
}

#[derive(Clone)]
struct HttpMetricsState {
    observability: Observability,
    surface: &'static str,
}

impl HttpMetricsState {
    fn new(observability: Observability, surface: &'static str) -> Self {
        Self {
            observability,
            surface,
        }
    }
}

async fn track_http_metrics(
    State(state): State<HttpMetricsState>,
    request: Request,
    next: Next,
) -> Response {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or("unknown")
        .to_string();
    let method = request.method().as_str().to_string();
    let start = Instant::now();
    let response = next.run(request).await;
    state.observability.observe_http(
        state.surface,
        &route,
        &method,
        response.status().as_u16(),
        start.elapsed(),
    );
    response
}
