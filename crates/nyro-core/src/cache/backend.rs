use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::storage::DynStorage;

#[async_trait]
pub trait CacheBackend: Send + Sync {
    async fn get(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>>;
    async fn set(&self, key: &str, data: &[u8], ttl: Option<Duration>) -> anyhow::Result<()>;
    async fn delete(&self, key: &str) -> anyhow::Result<()>;
    async fn flush(&self) -> anyhow::Result<()>;
    async fn ping(&self) -> anyhow::Result<bool>;
    fn backend_name(&self) -> &str;
}

#[derive(Clone)]
pub struct InMemoryCacheBackend {
    entries: Arc<RwLock<HashMap<String, InMemoryEntry>>>,
    max_entries: usize,
    name: String,
}

#[derive(Clone)]
struct InMemoryEntry {
    data: Vec<u8>,
    expires_at: Instant,
}

impl InMemoryCacheBackend {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            max_entries: max_entries.max(1),
            name: "in_memory".to_string(),
        }
    }

    async fn evict_if_needed(&self, entries: &mut HashMap<String, InMemoryEntry>) {
        if entries.len() < self.max_entries {
            return;
        }
        let mut oldest_key: Option<String> = None;
        let mut oldest_instant = Instant::now();
        for (key, value) in entries.iter() {
            if value.expires_at < oldest_instant {
                oldest_instant = value.expires_at;
                oldest_key = Some(key.clone());
            }
        }
        if let Some(key) = oldest_key {
            entries.remove(&key);
        }
    }
}

#[async_trait]
impl CacheBackend for InMemoryCacheBackend {
    async fn get(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let mut entries = self.entries.write().await;
        if let Some(entry) = entries.get(key) {
            if entry.expires_at > Instant::now() {
                return Ok(Some(entry.data.clone()));
            }
        }
        entries.remove(key);
        Ok(None)
    }

    async fn set(&self, key: &str, data: &[u8], ttl: Option<Duration>) -> anyhow::Result<()> {
        let ttl = ttl.unwrap_or_else(|| Duration::from_secs(3600));
        let mut entries = self.entries.write().await;
        self.evict_if_needed(&mut entries).await;
        entries.insert(
            key.to_string(),
            InMemoryEntry {
                data: data.to_vec(),
                expires_at: Instant::now() + ttl,
            },
        );
        Ok(())
    }

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        self.entries.write().await.remove(key);
        Ok(())
    }

    async fn flush(&self) -> anyhow::Result<()> {
        self.entries.write().await.clear();
        Ok(())
    }

    async fn ping(&self) -> anyhow::Result<bool> {
        Ok(true)
    }

    fn backend_name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone)]
pub struct DatabaseCacheBackend {
    storage: DynStorage,
    name: String,
}

impl DatabaseCacheBackend {
    pub fn new(storage: DynStorage) -> Self {
        Self {
            storage,
            name: "database".to_string(),
        }
    }
}

#[async_trait]
impl CacheBackend for DatabaseCacheBackend {
    async fn get(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
        self.storage
            .cache()
            .context("database cache backend is not supported by current storage")?
            .get(key)
            .await
    }

    async fn set(&self, key: &str, data: &[u8], ttl: Option<Duration>) -> anyhow::Result<()> {
        self.storage
            .cache()
            .context("database cache backend is not supported by current storage")?
            .set(key, data, ttl)
            .await
    }

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        self.storage
            .cache()
            .context("database cache backend is not supported by current storage")?
            .delete(key)
            .await
    }

    async fn flush(&self) -> anyhow::Result<()> {
        self.storage
            .cache()
            .context("database cache backend is not supported by current storage")?
            .flush()
            .await
    }

    async fn ping(&self) -> anyhow::Result<bool> {
        let supported = self.storage.cache().is_some();
        Ok(supported)
    }

    fn backend_name(&self) -> &str {
        &self.name
    }
}
