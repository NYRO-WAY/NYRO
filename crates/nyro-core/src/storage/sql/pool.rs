use anyhow::Context;
use sqlx::postgres::PgPoolOptions;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Pool, Postgres, Sqlite};

use super::config::{SqlBackendConfig, SqlBackendKind};
use super::dialect::SqlDialect;

#[derive(Clone)]
pub enum RelationalPool {
    Sqlite(Pool<Sqlite>),
    Postgres(Pool<Postgres>),
}

impl RelationalPool {
    pub async fn connect(kind: SqlBackendKind, cfg: &SqlBackendConfig) -> anyhow::Result<Self> {
        match kind {
            SqlBackendKind::Sqlite => {
                let pool = SqlitePoolOptions::new()
                    .max_connections(cfg.max_connections)
                    .min_connections(cfg.min_connections)
                    .acquire_timeout(cfg.acquire_timeout)
                    .idle_timeout(cfg.idle_timeout)
                    .max_lifetime(cfg.max_lifetime)
                    .connect(&cfg.url)
                    .await
                    .with_context(|| format!("failed to connect sqlite: {}", cfg.url))?;
                Ok(Self::Sqlite(pool))
            }
            SqlBackendKind::Postgres => {
                let pool = PgPoolOptions::new()
                    .max_connections(cfg.max_connections)
                    .min_connections(cfg.min_connections)
                    .acquire_timeout(cfg.acquire_timeout)
                    .idle_timeout(cfg.idle_timeout)
                    .max_lifetime(cfg.max_lifetime)
                    .connect(&cfg.url)
                    .await
                    .with_context(|| format!("failed to connect postgres: {}", cfg.url))?;
                Ok(Self::Postgres(pool))
            }
        }
    }

    pub fn dialect(&self) -> SqlDialect {
        match self {
            RelationalPool::Sqlite(_) => SqlDialect::Sqlite,
            RelationalPool::Postgres(_) => SqlDialect::Postgres,
        }
    }

    pub async fn ping(&self) -> anyhow::Result<()> {
        match self {
            RelationalPool::Sqlite(pool) => {
                sqlx::query("SELECT 1").execute(pool).await?;
            }
            RelationalPool::Postgres(pool) => {
                sqlx::query("SELECT 1").execute(pool).await?;
            }
        }
        Ok(())
    }

    pub async fn close(self) {
        match self {
            RelationalPool::Sqlite(pool) => pool.close().await,
            RelationalPool::Postgres(pool) => pool.close().await,
        }
    }

    pub fn as_postgres(&self) -> Option<&Pool<Postgres>> {
        match self {
            RelationalPool::Postgres(pool) => Some(pool),
            RelationalPool::Sqlite(_) => None,
        }
    }
}
