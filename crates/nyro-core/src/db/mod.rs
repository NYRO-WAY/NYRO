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
    ensure_provider_column(pool, "vendor", "TEXT").await?;
    ensure_provider_column(pool, "preset_key", "TEXT").await?;
    ensure_provider_column(pool, "channel", "TEXT").await?;
    ensure_provider_column(pool, "models_source", "TEXT").await?;
    ensure_provider_column(pool, "capabilities_source", "TEXT").await?;
    ensure_provider_column(pool, "static_models", "TEXT").await?;
    ensure_provider_column(pool, "last_test_success", "INTEGER").await?;
    ensure_provider_column(pool, "last_test_at", "TEXT").await?;
    ensure_provider_column(pool, "use_proxy", "INTEGER DEFAULT 0").await?;
    ensure_provider_column(pool, "default_protocol", "TEXT NOT NULL DEFAULT ''").await?;
    ensure_provider_column(pool, "protocol_endpoints", "TEXT NOT NULL DEFAULT '{}'").await?;
    backfill_provider_protocol_endpoints(pool).await?;
    ensure_route_column(pool, "virtual_model", "TEXT").await?;
    ensure_route_column(pool, "strategy", "TEXT DEFAULT 'weighted'").await?;
    ensure_route_column(pool, "access_control", "INTEGER DEFAULT 0").await?;
    ensure_request_log_column(pool, "api_key_id", "TEXT").await?;
    ensure_api_key_tables(pool).await?;
    ensure_api_key_column(pool, "rpd", "INTEGER").await?;
    ensure_route_targets_table(pool).await?;
    ensure_cache_entries_table(pool).await?;
    backfill_provider_vendor(pool).await?;
    backfill_route_fields(pool).await?;
    backfill_route_targets(pool).await?;
    Ok(())
}

async fn backfill_provider_vendor(pool: &SqlitePool) -> anyhow::Result<()> {
    if column_exists(pool, "providers", "vendor").await? && column_exists(pool, "providers", "preset_key").await? {
        sqlx::query(
            "UPDATE providers \
             SET vendor = lower(trim(preset_key)) \
             WHERE (vendor IS NULL OR trim(vendor) = '') \
               AND preset_key IS NOT NULL \
               AND trim(preset_key) != '' \
               AND lower(trim(preset_key)) != 'custom'",
        )
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn backfill_provider_protocol_endpoints(pool: &SqlitePool) -> anyhow::Result<()> {
    if column_exists(pool, "providers", "default_protocol").await?
        && column_exists(pool, "providers", "protocol_endpoints").await?
        && column_exists(pool, "providers", "protocol").await?
    {
        sqlx::query(
            "UPDATE providers \
             SET default_protocol = protocol \
             WHERE (default_protocol IS NULL OR trim(default_protocol) = '') \
               AND protocol IS NOT NULL AND trim(protocol) != ''",
        )
        .execute(pool)
        .await?;

        sqlx::query(
            "UPDATE providers \
             SET protocol_endpoints = json_object(trim(protocol), json_object('base_url', trim(base_url))) \
             WHERE (protocol_endpoints IS NULL OR trim(protocol_endpoints) = '' OR trim(protocol_endpoints) = '{}') \
               AND protocol IS NOT NULL AND trim(protocol) != '' \
               AND base_url IS NOT NULL AND trim(base_url) != ''",
        )
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

async fn ensure_api_key_column(
    pool: &SqlitePool,
    column_name: &str,
    definition: &str,
) -> anyhow::Result<()> {
    if !column_exists(pool, "api_keys", column_name).await? {
        let sql = format!("ALTER TABLE api_keys ADD COLUMN {column_name} {definition}");
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
            rpd         INTEGER,
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

async fn ensure_route_targets_table(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS route_targets (
            id          TEXT PRIMARY KEY,
            route_id    TEXT NOT NULL REFERENCES routes(id) ON DELETE CASCADE,
            provider_id TEXT NOT NULL REFERENCES providers(id),
            model       TEXT NOT NULL,
            weight      INTEGER DEFAULT 100,
            priority    INTEGER DEFAULT 1,
            created_at  TEXT DEFAULT (datetime('now'))
        )"#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_route_targets_route_id ON route_targets(route_id)")
        .execute(pool)
        .await?;

    Ok(())
}

async fn ensure_cache_entries_table(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS cache_entries (
            key         TEXT PRIMARY KEY,
            data        BLOB NOT NULL,
            expires_at  TEXT NOT NULL,
            created_at  TEXT DEFAULT (datetime('now'))
        )"#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_cache_entries_expires_at ON cache_entries(expires_at)")
        .execute(pool)
        .await?;

    Ok(())
}

async fn backfill_route_fields(pool: &SqlitePool) -> anyhow::Result<()> {
    if column_exists(pool, "routes", "strategy").await? {
        sqlx::query(
            "UPDATE routes SET strategy = 'weighted' WHERE strategy IS NULL OR trim(strategy) = ''",
        )
        .execute(pool)
        .await?;
    }

    Ok(())
}

async fn backfill_route_targets(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO route_targets (id, route_id, provider_id, model, weight, priority)
        SELECT lower(hex(randomblob(16))), r.id, r.target_provider, r.target_model, 100, 1
        FROM routes r
        WHERE r.target_provider IS NOT NULL
          AND trim(r.target_provider) != ''
          AND NOT EXISTS (
              SELECT 1 FROM route_targets rt WHERE rt.route_id = r.id
          )
        "#,
    )
    .execute(pool)
    .await?;

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
    vendor      TEXT,
    protocol    TEXT NOT NULL,
    base_url    TEXT NOT NULL,
    preset_key  TEXT,
    channel     TEXT,
    models_source TEXT,
    capabilities_source TEXT,
    static_models TEXT,
    api_key     TEXT NOT NULL,
    use_proxy   INTEGER DEFAULT 0,
    last_test_success INTEGER,
    last_test_at TEXT,
    is_active   INTEGER DEFAULT 1,
    priority    INTEGER DEFAULT 0,
    created_at  TEXT DEFAULT (datetime('now')),
    updated_at  TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS routes (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    virtual_model     TEXT,
    strategy          TEXT DEFAULT 'weighted',
    target_provider   TEXT NOT NULL REFERENCES providers(id),
    target_model      TEXT NOT NULL,
    access_control    INTEGER DEFAULT 0,
    is_active         INTEGER DEFAULT 1,
    priority          INTEGER DEFAULT 0,
    created_at        TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS route_targets (
    id          TEXT PRIMARY KEY,
    route_id    TEXT NOT NULL REFERENCES routes(id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL REFERENCES providers(id),
    model       TEXT NOT NULL,
    weight      INTEGER DEFAULT 100,
    priority    INTEGER DEFAULT 1,
    created_at  TEXT DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_route_targets_route_id ON route_targets(route_id);

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
    response_preview  TEXT
);

CREATE INDEX IF NOT EXISTS idx_logs_created_at ON request_logs(created_at);
CREATE INDEX IF NOT EXISTS idx_logs_provider ON request_logs(provider_name);
CREATE INDEX IF NOT EXISTS idx_logs_status ON request_logs(status_code);
CREATE INDEX IF NOT EXISTS idx_logs_model ON request_logs(actual_model);

CREATE TABLE IF NOT EXISTS settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS cache_entries (
    key         TEXT PRIMARY KEY,
    data        BLOB NOT NULL,
    expires_at  TEXT NOT NULL,
    created_at  TEXT DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_cache_entries_expires_at ON cache_entries(expires_at);
"#;
