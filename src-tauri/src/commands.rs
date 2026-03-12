use nyro_core::db::models::*;
use nyro_core::Gateway;
use tauri::State;

// ── Providers ──

#[tauri::command]
pub async fn get_providers(gw: State<'_, Gateway>) -> Result<Vec<Provider>, String> {
    gw.admin().list_providers().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_provider(gw: State<'_, Gateway>, id: String) -> Result<Provider, String> {
    gw.admin().get_provider(&id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_provider(
    gw: State<'_, Gateway>,
    input: CreateProvider,
) -> Result<Provider, String> {
    gw.admin().create_provider(input).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_provider(
    gw: State<'_, Gateway>,
    id: String,
    input: UpdateProvider,
) -> Result<Provider, String> {
    gw.admin().update_provider(&id, input).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_provider(gw: State<'_, Gateway>, id: String) -> Result<(), String> {
    gw.admin().delete_provider(&id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_provider(gw: State<'_, Gateway>, id: String) -> Result<TestResult, String> {
    gw.admin().test_provider(&id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_provider_models(
    gw: State<'_, Gateway>,
    id: String,
) -> Result<Vec<String>, String> {
    gw.admin()
        .get_provider_models(&id)
        .await
        .map_err(|e| e.to_string())
}

// ── Routes ──

#[tauri::command]
pub async fn list_routes(gw: State<'_, Gateway>) -> Result<Vec<Route>, String> {
    gw.admin().list_routes().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_route(
    gw: State<'_, Gateway>,
    input: CreateRoute,
) -> Result<Route, String> {
    gw.admin().create_route(input).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_route(
    gw: State<'_, Gateway>,
    id: String,
    input: UpdateRoute,
) -> Result<Route, String> {
    gw.admin().update_route(&id, input).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_route(gw: State<'_, Gateway>, id: String) -> Result<(), String> {
    gw.admin().delete_route(&id).await.map_err(|e| e.to_string())
}

// ── Logs ──

#[tauri::command]
pub async fn query_logs(gw: State<'_, Gateway>, query: LogQuery) -> Result<LogPage, String> {
    gw.admin().query_logs(query).await.map_err(|e| e.to_string())
}

// ── Stats ──

#[tauri::command]
pub async fn get_stats_overview(gw: State<'_, Gateway>) -> Result<StatsOverview, String> {
    gw.admin().get_stats_overview().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_stats_hourly(
    gw: State<'_, Gateway>,
    hours: Option<i32>,
) -> Result<Vec<StatsHourly>, String> {
    gw.admin()
        .get_stats_hourly(hours.unwrap_or(24))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_stats_by_model(gw: State<'_, Gateway>) -> Result<Vec<ModelStats>, String> {
    gw.admin().get_stats_by_model().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_stats_by_provider(
    gw: State<'_, Gateway>,
) -> Result<Vec<ProviderStats>, String> {
    gw.admin().get_stats_by_provider().await.map_err(|e| e.to_string())
}

// ── Settings ──

#[tauri::command]
pub async fn get_setting(gw: State<'_, Gateway>, key: String) -> Result<Option<String>, String> {
    gw.admin().get_setting(&key).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_setting(
    gw: State<'_, Gateway>,
    key: String,
    value: String,
) -> Result<(), String> {
    gw.admin().set_setting(&key, &value).await.map_err(|e| e.to_string())
}

// ── Status ──

#[tauri::command]
pub async fn get_gateway_status(gw: State<'_, Gateway>) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "status": "running",
        "proxy_port": gw.config.proxy_port,
    }))
}

// ── Config Import/Export ──

#[tauri::command]
pub async fn export_config(gw: State<'_, Gateway>) -> Result<ExportData, String> {
    gw.admin().export_config().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_config(
    gw: State<'_, Gateway>,
    data: ExportData,
) -> Result<ImportResult, String> {
    gw.admin().import_config(data).await.map_err(|e| e.to_string())
}
