use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct VectorStoreEntry {
    pub key: String,
    pub vector: Vec<f32>,
}

#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn upsert(&self, key: String, vector: Vec<f32>) -> anyhow::Result<()>;
    async fn search(&self, query: &[f32], threshold: f64) -> anyhow::Result<Option<String>>;
    async fn clear(&self) -> anyhow::Result<()>;
}

#[derive(Clone, Default)]
pub struct InMemoryVectorStore {
    entries: Arc<RwLock<Vec<VectorStoreEntry>>>,
}

impl InMemoryVectorStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl VectorStore for InMemoryVectorStore {
    async fn upsert(&self, key: String, vector: Vec<f32>) -> anyhow::Result<()> {
        let mut entries = self.entries.write().await;
        if let Some(existing) = entries.iter_mut().find(|entry| entry.key == key) {
            existing.vector = vector;
            return Ok(());
        }
        entries.push(VectorStoreEntry { key, vector });
        Ok(())
    }

    async fn search(&self, query: &[f32], threshold: f64) -> anyhow::Result<Option<String>> {
        let entries = self.entries.read().await;
        let mut best: Option<(f64, String)> = None;
        for entry in entries.iter() {
            let similarity = cosine_similarity(query, &entry.vector);
            if similarity >= threshold {
                if best
                    .as_ref()
                    .map(|(score, _)| similarity > *score)
                    .unwrap_or(true)
                {
                    best = Some((similarity, entry.key.clone()));
                }
            }
        }
        Ok(best.map(|(_, key)| key))
    }

    async fn clear(&self) -> anyhow::Result<()> {
        self.entries.write().await.clear();
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
