use async_trait::async_trait;
use sqlx::SqlitePool;
use zerocopy::AsBytes;

use super::vector::{VectorHit, VectorStore};

#[derive(Clone)]
pub struct SqliteVecVectorStore {
    pool: SqlitePool,
    max_entries: usize,
}

impl SqliteVecVectorStore {
    pub fn new(pool: SqlitePool, max_entries: usize) -> Self {
        Self {
            pool,
            max_entries: max_entries.max(1),
        }
    }

    async fn evict_partition_if_needed(&self, partition: &str) -> anyhow::Result<()> {
        let total: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM semantic_cache_vectors WHERE partition = ?")
                .bind(partition)
                .fetch_one(&self.pool)
                .await?;

        let overflow = total.saturating_sub(self.max_entries as i64);
        if overflow <= 0 {
            return Ok(());
        }

        sqlx::query(
            "DELETE FROM semantic_cache_vectors WHERE rowid IN (
                SELECT rowid
                FROM semantic_cache_vectors
                WHERE partition = ?
                ORDER BY created_at ASC, rowid ASC
                LIMIT ?
            )",
        )
        .bind(partition)
        .bind(overflow)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[async_trait]
impl VectorStore for SqliteVecVectorStore {
    async fn upsert(
        &self,
        partition: &str,
        key: String,
        vector: Vec<f32>,
        data: Vec<u8>,
    ) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM semantic_cache_vectors WHERE partition = ? AND cache_key = ?")
            .bind(partition)
            .bind(&key)
            .execute(&self.pool)
            .await?;

        sqlx::query(
            "INSERT INTO semantic_cache_vectors(partition, cache_key, dimensions, embedding, data, created_at)
             VALUES(?, ?, ?, ?, ?, unixepoch())",
        )
        .bind(partition)
        .bind(&key)
        .bind(vector.len() as i64)
        .bind(vector.as_bytes())
        .bind(data)
        .execute(&self.pool)
        .await?;

        self.evict_partition_if_needed(partition).await
    }

    async fn search(
        &self,
        partition: &str,
        query: &[f32],
        threshold: f64,
    ) -> anyhow::Result<Option<VectorHit>> {
        let row = sqlx::query_as::<_, (String, Vec<u8>, f64)>(
            "SELECT cache_key, data, distance
             FROM semantic_cache_vectors
             WHERE embedding MATCH ?
               AND k = 1
               AND partition = ?
               AND dimensions = ?",
        )
        .bind(query.as_bytes())
        .bind(partition)
        .bind(query.len() as i64)
        .fetch_optional(&self.pool)
        .await?;

        let Some((key, data, distance)) = row else {
            return Ok(None);
        };

        let similarity = 1.0 - distance;
        if similarity < threshold {
            return Ok(None);
        }

        Ok(Some(VectorHit {
            key,
            data,
            score: similarity,
        }))
    }

    async fn clear(&self) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM semantic_cache_vectors")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn clear_partition(&self, partition: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM semantic_cache_vectors WHERE partition = ?")
            .bind(partition)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
