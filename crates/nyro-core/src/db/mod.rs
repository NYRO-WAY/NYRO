pub mod models;

use std::path::Path;
use std::sync::Once;

use sqlite_vec::sqlite3_vec_init;
use sqlx::Row;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

static SQLITE_VEC_INIT: Once = Once::new();
const VECTOR_DIMENSIONS_SETTING_KEY: &str = "vector_embedding_dimensions";

pub async fn init_pool(data_dir: &Path) -> anyhow::Result<SqlitePool> {
    SQLITE_VEC_INIT.call_once(|| unsafe {
        // Register sqlite-vec once per process. All later SQLite connections can use vec0 tables.
        libsqlite3_sys::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite3_vec_init as *const (),
        )));
    });

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

pub async fn migrate(pool: &SqlitePool, vector_dimensions: usize) -> anyhow::Result<()> {
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
    ensure_route_column(pool, "route_type", "TEXT DEFAULT 'chat'").await?;
    ensure_route_column(pool, "cache_exact_ttl", "INTEGER").await?;
    ensure_route_column(pool, "cache_semantic_ttl", "INTEGER").await?;
    ensure_route_column(pool, "cache_semantic_threshold", "REAL").await?;
    ensure_request_log_column(pool, "api_key_id", "TEXT").await?;
    ensure_api_key_tables(pool).await?;
    ensure_api_key_column(pool, "rpd", "INTEGER").await?;
    // Migrate: providers/routes is_active -> is_enabled
    ensure_provider_column(pool, "is_enabled", "INTEGER DEFAULT 1").await?;
    migrate_provider_is_active_to_is_enabled(pool).await?;
    ensure_route_column(pool, "is_enabled", "INTEGER DEFAULT 1").await?;
    migrate_route_is_active_to_is_enabled(pool).await?;
    // Migrate: api_keys status -> is_enabled
    ensure_api_key_column(pool, "is_enabled", "INTEGER DEFAULT 1").await?;
    migrate_api_key_status_to_is_enabled(pool).await?;
    ensure_route_targets_table(pool).await?;
    ensure_cache_entries_table(pool).await?;
    ensure_semantic_cache_vectors_table(pool, vector_dimensions).await?;
    backfill_provider_vendor(pool).await?;
    backfill_route_fields(pool).await?;
    backfill_route_targets(pool).await?;
    Ok(())
}

async fn backfill_provider_vendor(pool: &SqlitePool) -> anyhow::Result<()> {
    if column_exists(pool, "providers", "vendor").await?
        && column_exists(pool, "providers", "preset_key").await?
    {
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
            is_enabled  INTEGER DEFAULT 1,
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
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_api_key_routes_route_id ON api_key_routes(route_id)",
    )
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

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_cache_entries_expires_at ON cache_entries(expires_at)",
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn recreate_vec0_table(pool: &SqlitePool, vector_dimensions: usize) -> anyhow::Result<()> {
    let dimensions = vector_dimensions.max(1);
    sqlx::query("DROP TABLE IF EXISTS semantic_cache_vectors")
        .execute(pool)
        .await?;

    let ddl = format!(
        r#"CREATE VIRTUAL TABLE semantic_cache_vectors USING vec0(
            partition TEXT partition key,
            cache_key TEXT,
            dimensions INTEGER,
            embedding float[{dimensions}] distance_metric=cosine,
            +data BLOB,
            created_at INTEGER
        )"#
    );
    sqlx::query(&ddl).execute(pool).await?;
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(VECTOR_DIMENSIONS_SETTING_KEY)
    .bind(dimensions.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

async fn ensure_semantic_cache_vectors_table(
    pool: &SqlitePool,
    vector_dimensions: usize,
) -> anyhow::Result<()> {
    let dimensions = vector_dimensions.max(1);
    let stored_dimension = sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
        .bind(VECTOR_DIMENSIONS_SETTING_KEY)
        .fetch_optional(pool)
        .await?
        .and_then(|raw| raw.trim().parse::<usize>().ok());
    let table_exists = semantic_cache_vectors_table_exists(pool).await?;
    if !table_exists || stored_dimension != Some(dimensions) {
        recreate_vec0_table(pool, dimensions).await?;
    }
    Ok(())
}

async fn semantic_cache_vectors_table_exists(pool: &SqlitePool) -> anyhow::Result<bool> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'semantic_cache_vectors'",
    )
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
}

async fn migrate_provider_is_active_to_is_enabled(pool: &SqlitePool) -> anyhow::Result<()> {
    if column_exists(pool, "providers", "is_active").await? {
        sqlx::query("UPDATE providers SET is_enabled = is_active WHERE is_enabled IS NULL OR is_enabled != is_active AND is_active IS NOT NULL")
            .execute(pool)
            .await?;
    }
    Ok(())
}

async fn migrate_route_is_active_to_is_enabled(pool: &SqlitePool) -> anyhow::Result<()> {
    if column_exists(pool, "routes", "is_active").await? {
        sqlx::query("UPDATE routes SET is_enabled = is_active WHERE is_enabled IS NULL OR is_enabled != is_active AND is_active IS NOT NULL")
            .execute(pool)
            .await?;
    }
    Ok(())
}

async fn migrate_api_key_status_to_is_enabled(pool: &SqlitePool) -> anyhow::Result<()> {
    if column_exists(pool, "api_keys", "status").await? {
        sqlx::query(
            "UPDATE api_keys SET is_enabled = CASE WHEN status = 'active' THEN 1 ELSE 0 END \
             WHERE is_enabled IS NULL OR (status = 'active' AND is_enabled = 0) OR (status != 'active' AND is_enabled = 1)",
        )
        .execute(pool)
        .await?;
    }
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
    if column_exists(pool, "routes", "route_type").await? {
        sqlx::query(
            "UPDATE routes SET route_type = 'chat' WHERE route_type IS NULL OR trim(route_type) = ''",
        )
        .execute(pool)
        .await?;
    }
    if column_exists(pool, "routes", "cache_ttl").await? {
        sqlx::query(
            "UPDATE routes SET cache_exact_ttl = cache_ttl WHERE cache_exact_ttl IS NULL AND cache_ttl IS NOT NULL",
        )
        .execute(pool)
        .await?;
    }
    if column_exists(pool, "routes", "cache_enabled").await? {
        sqlx::query(
            "UPDATE routes SET cache_exact_ttl = 3600 WHERE cache_enabled = 1 AND cache_exact_ttl IS NULL",
        )
        .execute(pool)
        .await?;
        sqlx::query("UPDATE routes SET cache_exact_ttl = NULL WHERE cache_enabled = 0")
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

async fn column_exists(
    pool: &SqlitePool,
    table_name: &str,
    column_name: &str,
) -> anyhow::Result<bool> {
    let pragma = format!("PRAGMA table_info({table_name})");
    let rows = sqlx::query(&pragma).fetch_all(pool).await?;
    Ok(rows.iter().any(|row| {
        row.try_get::<String, _>("name")
            .map(|name| name == column_name)
            .unwrap_or(false)
    }))
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
    is_enabled  INTEGER DEFAULT 1,
    priority    INTEGER DEFAULT 0,
    created_at  TEXT DEFAULT (datetime('now')),
    updated_at  TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS routes (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    virtual_model     TEXT,
    strategy          TEXT DEFAULT 'weighted',
    route_type        TEXT DEFAULT 'chat',
    target_provider   TEXT NOT NULL REFERENCES providers(id),
    target_model      TEXT NOT NULL,
    cache_exact_ttl   INTEGER,
    cache_semantic_ttl INTEGER,
    cache_semantic_threshold REAL,
    access_control    INTEGER DEFAULT 0,
    is_enabled        INTEGER DEFAULT 1,
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
