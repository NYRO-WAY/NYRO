pub mod models;

use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::Row;
use sqlx::SqlitePool;

pub async fn init_pool(data_dir: &Path) -> anyhow::Result<SqlitePool> {
    std::fs::create_dir_all(data_dir)?;
    let db_path = data_dir.join("gateway.db");

    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    Ok(pool)
}

pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::raw_sql(INIT_SQL).execute(pool).await?;
    ensure_provider_column(pool, "preset_key", "TEXT").await?;
    ensure_provider_column(pool, "region", "TEXT").await?;
    ensure_provider_column(pool, "channel", "TEXT").await?;
    ensure_provider_column(pool, "models_endpoint", "TEXT").await?;
    ensure_provider_column(pool, "static_models", "TEXT").await?;
    ensure_route_column(pool, "ingress_protocol", "TEXT").await?;
    ensure_route_column(pool, "virtual_model", "TEXT").await?;
    ensure_route_column(pool, "access_control", "INTEGER DEFAULT 0").await?;
    ensure_request_log_column(pool, "api_key_id", "TEXT").await?;
    ensure_api_key_tables(pool).await?;
    backfill_provider_channel(pool).await?;
    backfill_route_fields(pool).await?;
    Ok(())
}

async fn backfill_provider_channel(pool: &SqlitePool) -> anyhow::Result<()> {
    if column_exists(pool, "providers", "region").await? && column_exists(pool, "providers", "channel").await? {
        sqlx::query("UPDATE providers SET channel = region WHERE (channel IS NULL OR channel = '') AND region IS NOT NULL AND region != ''")
            .execute(pool)
            .await?;
    }

    Ok(())
}

async fn ensure_provider_column(
    pool: &SqlitePool,
    column_name: &str,
    definition: &str,
) -> anyhow::Result<()> {
    if !column_exists(pool, "providers", column_name).await? {
        let sql = format!("ALTER TABLE providers ADD COLUMN {column_name} {definition}");
        sqlx::query(&sql).execute(pool).await?;
    }

    Ok(())
}

async fn ensure_route_column(
    pool: &SqlitePool,
    column_name: &str,
    definition: &str,
) -> anyhow::Result<()> {
    if !column_exists(pool, "routes", column_name).await? {
        let sql = format!("ALTER TABLE routes ADD COLUMN {column_name} {definition}");
        sqlx::query(&sql).execute(pool).await?;
    }

    Ok(())
}

async fn ensure_request_log_column(
    pool: &SqlitePool,
    column_name: &str,
    definition: &str,
) -> anyhow::Result<()> {
    if !column_exists(pool, "request_logs", column_name).await? {
        let sql = format!("ALTER TABLE request_logs ADD COLUMN {column_name} {definition}");
        sqlx::query(&sql).execute(pool).await?;
    }

    Ok(())
}

async fn ensure_api_key_tables(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS api_keys (
            id          TEXT PRIMARY KEY,
            key         TEXT NOT NULL UNIQUE,
            name        TEXT NOT NULL,
            rpm         INTEGER,
            tpm         INTEGER,
            tpd         INTEGER,
            status      TEXT NOT NULL DEFAULT 'active',
            expires_at  TEXT,
            created_at  TEXT DEFAULT (datetime('now')),
            updated_at  TEXT DEFAULT (datetime('now'))
        )"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS api_key_routes (
            api_key_id  TEXT NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
            route_id    TEXT NOT NULL REFERENCES routes(id) ON DELETE CASCADE,
            PRIMARY KEY (api_key_id, route_id)
        )"#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_api_keys_key ON api_keys(key)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_api_key_routes_route_id ON api_key_routes(route_id)")
        .execute(pool)
        .await?;

    Ok(())
}

async fn backfill_route_fields(pool: &SqlitePool) -> anyhow::Result<()> {
    if column_exists(pool, "routes", "virtual_model").await?
        && column_exists(pool, "routes", "match_pattern").await?
    {
        sqlx::query(
            "UPDATE routes SET virtual_model = match_pattern WHERE (virtual_model IS NULL OR virtual_model = '') AND match_pattern IS NOT NULL AND match_pattern != ''",
        )
        .execute(pool)
        .await?;
    }

    if column_exists(pool, "routes", "ingress_protocol").await? {
        sqlx::query(
            "UPDATE routes SET ingress_protocol = 'openai' WHERE ingress_protocol IS NULL OR ingress_protocol = ''",
        )
        .execute(pool)
        .await?;
    }

    Ok(())
}

async fn column_exists(pool: &SqlitePool, table_name: &str, column_name: &str) -> anyhow::Result<bool> {
    let pragma = format!("PRAGMA table_info({table_name})");
    let rows = sqlx::query(&pragma).fetch_all(pool).await?;
    Ok(rows
        .iter()
        .any(|row| row.try_get::<String, _>("name").map(|name| name == column_name).unwrap_or(false)))
}

const INIT_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS providers (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    protocol    TEXT NOT NULL,
    base_url    TEXT NOT NULL,
    preset_key  TEXT,
    region      TEXT,
    channel     TEXT,
    models_endpoint TEXT,
    static_models TEXT,
    api_key     TEXT NOT NULL,
    is_active   INTEGER DEFAULT 1,
    priority    INTEGER DEFAULT 0,
    created_at  TEXT DEFAULT (datetime('now')),
    updated_at  TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS routes (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    match_pattern     TEXT NOT NULL,
    ingress_protocol  TEXT,
    virtual_model     TEXT,
    target_provider   TEXT NOT NULL REFERENCES providers(id),
    target_model      TEXT NOT NULL,
    fallback_provider TEXT REFERENCES providers(id),
    fallback_model    TEXT,
    access_control    INTEGER DEFAULT 0,
    is_active         INTEGER DEFAULT 1,
    priority          INTEGER DEFAULT 0,
    created_at        TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS request_logs (
    id                TEXT PRIMARY KEY,
    created_at        TEXT DEFAULT (datetime('now')),
    api_key_id        TEXT,
    ingress_protocol  TEXT,
    egress_protocol   TEXT,
    request_model     TEXT,
    actual_model      TEXT,
    provider_name     TEXT,
    status_code       INTEGER,
    duration_ms       REAL,
    input_tokens      INTEGER DEFAULT 0,
    output_tokens     INTEGER DEFAULT 0,
    is_stream         INTEGER DEFAULT 0,
    is_tool_call      INTEGER DEFAULT 0,
    error_message     TEXT,
    request_preview   TEXT,
    response_preview  TEXT
);

CREATE INDEX IF NOT EXISTS idx_logs_created_at ON request_logs(created_at);
CREATE INDEX IF NOT EXISTS idx_logs_provider ON request_logs(provider_name);
CREATE INDEX IF NOT EXISTS idx_logs_status ON request_logs(status_code);
CREATE INDEX IF NOT EXISTS idx_logs_model ON request_logs(actual_model);

CREATE TABLE IF NOT EXISTS models (
    id          TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    model_name  TEXT NOT NULL,
    display_name TEXT,
    is_custom   INTEGER DEFAULT 0,
    created_at  TEXT DEFAULT (datetime('now')),
    UNIQUE(provider_id, model_name)
);

CREATE TABLE IF NOT EXISTS stats_hourly (
    hour                TEXT,
    provider            TEXT,
    model               TEXT,
    request_count       INTEGER DEFAULT 0,
    error_count         INTEGER DEFAULT 0,
    total_input_tokens  INTEGER DEFAULT 0,
    total_output_tokens INTEGER DEFAULT 0,
    avg_duration_ms     REAL DEFAULT 0,
    PRIMARY KEY (hour, provider, model)
);

CREATE TABLE IF NOT EXISTS settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT DEFAULT (datetime('now'))
);
"#;
