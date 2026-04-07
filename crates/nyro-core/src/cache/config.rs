use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheType {
    Response,
    Semantic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheBackendKind {
    InMemory,
    Database,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheMode {
    DefaultOn,
    DefaultOff,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticCacheConfig {
    pub embedding_model: String,
    pub similarity_threshold: f64,
    pub vector_dimensions: usize,
}

#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub enabled: bool,
    pub cache_type: CacheType,
    pub backend: CacheBackendKind,
    pub default_ttl: Duration,
    pub max_entries: usize,
    pub namespace: Option<String>,
    pub mode: CacheMode,
    pub cache_streaming: bool,
    pub semantic: Option<SemanticCacheConfig>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cache_type: CacheType::Response,
            backend: CacheBackendKind::InMemory,
            default_ttl: Duration::from_secs(3600),
            max_entries: 1000,
            namespace: None,
            mode: CacheMode::DefaultOn,
            cache_streaming: true,
            semantic: None,
        }
    }
}
