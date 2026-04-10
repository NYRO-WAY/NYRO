use async_trait::async_trait;
use pgvector::Vector;
use sqlx::{Pool, Postgres};

use super::vector::{VectorHit, VectorStore};

#[derive(Clone)]
pub struct PgVectorStore {
    pool: Pool<Postgres>,
    max_entries: usize,
}

impl PgVectorStore {
    pub fn new(pool: Pool<Postgres>, max_entries: usize) -> Self {
        Self {
            pool,
            max_entries: max_entries.max(1),
        }
    }

    async fn evict_partition_if_needed(&self, partition: &str) -> anyhow::Result<()> {
        let total: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM semantic_cache_vectors WHERE partition = $1")
                .bind(partition)
                .fetch_one(&self.pool)
                .await?;
        let overflow = total.saturating_sub(self.max_entries as i64);
        if overflow <= 0 {
            return Ok(());
        }

        sqlx::query(
            "DELETE FROM semantic_cache_vectors
             WHERE id IN (
                SELECT id
                FROM semantic_cache_vectors
                WHERE partition = $1
                ORDER BY created_at ASC, id ASC
                LIMIT $2
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
impl VectorStore for PgVectorStore {
    async fn upsert(
        &self,
        partition: &str,
        key: String,
        vector: Vec<f32>,
        data: Vec<u8>,
    ) -> anyhow::Result<()> {
        let dimensions = vector.len() as i32;
        let vector = Vector::from(vector);
        sqlx::query(
            "INSERT INTO semantic_cache_vectors (partition, cache_key, dimensions, embedding, data, created_at)
             VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP)
             ON CONFLICT (partition, cache_key)
             DO UPDATE SET dimensions = EXCLUDED.dimensions,
                           embedding = EXCLUDED.embedding,
                           data = EXCLUDED.data,
                           created_at = CURRENT_TIMESTAMP",
        )
        .bind(partition)
        .bind(&key)
        .bind(dimensions)
        .bind(vector)
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
        let dimensions = query.len() as i32;
        let query_vector = Vector::from(query.to_vec());
        let row = sqlx::query_as::<_, (String, Vec<u8>, f64)>(
            "SELECT cache_key, data, (1 - (embedding <=> $1))::float8 AS similarity
             FROM semantic_cache_vectors
             WHERE partition = $2
               AND dimensions = $3
             ORDER BY embedding <=> $1
             LIMIT 1",
        )
        .bind(query_vector)
        .bind(partition)
        .bind(dimensions)
        .fetch_optional(&self.pool)
        .await?;

        let Some((key, data, similarity)) = row else {
            return Ok(None);
        };
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
        sqlx::query("DELETE FROM semantic_cache_vectors WHERE partition = $1")
            .bind(partition)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
