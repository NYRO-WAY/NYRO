use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExactCacheConfig {
    pub enabled: bool,
    pub default_ttl: Duration,
    pub max_entries: usize,
    /// Tokens per second for cached stream replay. 0 = no throttle (instant).
    pub stream_replay_tps: u32,
    /// Whether to expose X-NYRO-CACHE-* response headers on cache hit/miss.
    pub expose_headers: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticCacheConfig {
    pub enabled: bool,
    pub embedding_route: String,
    pub similarity_threshold: f64,
    pub vector_dimensions: usize,
    pub default_ttl: Duration,
    pub max_entries: usize,
    /// Tokens per second for cached stream replay. 0 = no throttle (instant).
    pub stream_replay_tps: u32,
    /// Whether to expose X-NYRO-CACHE-* response headers on cache hit/miss.
    pub expose_headers: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    pub exact: ExactCacheConfig,
    pub semantic: SemanticCacheConfig,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            exact: ExactCacheConfig {
                enabled: false,
                default_ttl: Duration::from_secs(3600),
                max_entries: 1000,
                stream_replay_tps: 100,
                expose_headers: true,
            },
            semantic: SemanticCacheConfig {
                enabled: false,
                embedding_route: String::new(),
                similarity_threshold: 0.92,
                vector_dimensions: 1536,
                default_ttl: Duration::from_secs(600),
                max_entries: 500,
                stream_replay_tps: 100,
                expose_headers: true,
            },
        }
    }
}

impl CacheConfig {
    pub fn to_admin_json(&self) -> Value {
        json!({
            "exact": {
                "enabled": self.exact.enabled,
                "default_ttl": self.exact.default_ttl.as_secs(),
                "max_entries": self.exact.max_entries,
                "stream_replay_tps": self.exact.stream_replay_tps,
                "expose_headers": self.exact.expose_headers,
            },
            "semantic": {
                "enabled": self.semantic.enabled,
                "embedding_route": self.semantic.embedding_route,
                "similarity_threshold": self.semantic.similarity_threshold,
                "vector_dimensions": self.semantic.vector_dimensions,
                "default_ttl": self.semantic.default_ttl.as_secs(),
                "max_entries": self.semantic.max_entries,
                "stream_replay_tps": self.semantic.stream_replay_tps,
                "expose_headers": self.semantic.expose_headers,
            }
        })
    }

    pub fn from_admin_json(value: &Value) -> Option<Self> {
        let exact = value.get("exact")?;
        let semantic = value.get("semantic")?;

        let exact_enabled = exact.get("enabled")?.as_bool()?;
        let exact_default_ttl = exact.get("default_ttl")?.as_u64()?.max(1);
        let exact_max_entries = exact.get("max_entries")?.as_u64()?.max(1) as usize;

        let semantic_enabled = semantic.get("enabled")?.as_bool()?;
        let embedding_route = semantic
            .get("embedding_route")?
            .as_str()?
            .trim()
            .to_string();
        let similarity_threshold = semantic.get("similarity_threshold")?.as_f64()?;
        let vector_dimensions = semantic.get("vector_dimensions")?.as_u64()?.max(1) as usize;
        let semantic_default_ttl = semantic.get("default_ttl")?.as_u64()?.max(1);
        let semantic_max_entries = semantic.get("max_entries")?.as_u64()?.max(1) as usize;

        let exact_stream_replay_tps = exact
            .get("stream_replay_tps")
            .and_then(|v| v.as_u64())
            .unwrap_or(100) as u32;
        let exact_expose_headers = exact
            .get("expose_headers")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let semantic_stream_replay_tps = semantic
            .get("stream_replay_tps")
            .and_then(|v| v.as_u64())
            .unwrap_or(100) as u32;
        let semantic_expose_headers = semantic
            .get("expose_headers")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        Some(Self {
            exact: ExactCacheConfig {
                enabled: exact_enabled,
                default_ttl: Duration::from_secs(exact_default_ttl),
                max_entries: exact_max_entries,
                stream_replay_tps: exact_stream_replay_tps,
                expose_headers: exact_expose_headers,
            },
            semantic: SemanticCacheConfig {
                enabled: semantic_enabled,
                embedding_route,
                similarity_threshold,
                vector_dimensions,
                default_ttl: Duration::from_secs(semantic_default_ttl),
                max_entries: semantic_max_entries,
                stream_replay_tps: semantic_stream_replay_tps,
                expose_headers: semantic_expose_headers,
            },
        })
    }
}
