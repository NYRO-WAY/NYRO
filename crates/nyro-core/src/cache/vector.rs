use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct VectorStoreEntry {
    pub key: String,
    pub vector: Vec<f32>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct VectorHit {
    pub key: String,
    pub data: Vec<u8>,
    pub score: f64,
}

#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn upsert(
        &self,
        partition: &str,
        key: String,
        vector: Vec<f32>,
        data: Vec<u8>,
    ) -> anyhow::Result<()>;
    async fn search(
        &self,
        partition: &str,
        query: &[f32],
        threshold: f64,
    ) -> anyhow::Result<Option<VectorHit>>;
    async fn clear(&self) -> anyhow::Result<()>;
    async fn clear_partition(&self, partition: &str) -> anyhow::Result<()>;
}

#[derive(Clone)]
pub struct MemoryVectorStore {
    entries: Arc<RwLock<HashMap<String, Vec<VectorStoreEntry>>>>,
    max_entries: usize,
}

impl MemoryVectorStore {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            max_entries: max_entries.max(1),
        }
    }
}

#[async_trait]
impl VectorStore for MemoryVectorStore {
    async fn upsert(
        &self,
        partition: &str,
        key: String,
        vector: Vec<f32>,
        data: Vec<u8>,
    ) -> anyhow::Result<()> {
        let mut entries = self.entries.write().await;
        let bucket = entries.entry(partition.to_string()).or_default();
        if let Some(existing) = bucket.iter_mut().find(|entry| entry.key == key) {
            existing.vector = vector;
            existing.data = data;
            return Ok(());
        }
        if bucket.len() >= self.max_entries {
            bucket.remove(0);
        }
        bucket.push(VectorStoreEntry { key, vector, data });
        Ok(())
    }

    async fn search(
        &self,
        partition: &str,
        query: &[f32],
        threshold: f64,
    ) -> anyhow::Result<Option<VectorHit>> {
        let entries = self.entries.read().await;
        let Some(bucket) = entries.get(partition) else {
            return Ok(None);
        };
        let mut best: Option<VectorHit> = None;
        for entry in bucket {
            let similarity = cosine_similarity(query, &entry.vector);
            if similarity >= threshold {
                if best
                    .as_ref()
                    .map(|hit| similarity > hit.score)
                    .unwrap_or(true)
                {
                    best = Some(VectorHit {
                        key: entry.key.clone(),
                        data: entry.data.clone(),
                        score: similarity,
                    });
                }
            }
        }
        Ok(best)
    }

    async fn clear(&self) -> anyhow::Result<()> {
        self.entries.write().await.clear();
        Ok(())
    }

    async fn clear_partition(&self, partition: &str) -> anyhow::Result<()> {
        self.entries.write().await.remove(partition);
        Ok(())
    }
}

fn cosine_similarity(lhs: &[f32], rhs: &[f32]) -> f64 {
    if lhs.len() != rhs.len() || lhs.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut lhs_norm = 0.0f64;
    let mut rhs_norm = 0.0f64;
    for (a, b) in lhs.iter().zip(rhs.iter()) {
        let af = *a as f64;
        let bf = *b as f64;
        dot += af * bf;
        lhs_norm += af * af;
        rhs_norm += bf * bf;
    }
    if lhs_norm == 0.0 || rhs_norm == 0.0 {
        return 0.0;
    }
    dot / (lhs_norm.sqrt() * rhs_norm.sqrt())
}
