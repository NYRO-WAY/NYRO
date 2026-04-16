use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheStorageKind {
    Memory,
    Database,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorStorageKind {
    Memory,
    // SqliteVec, // future
    // PgVector,  // future
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExactCacheConfig {
    pub enabled: bool,
    pub storage: CacheStorageKind,
    pub default_ttl: Duration,
    pub max_entries: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticCacheConfig {
    pub enabled: bool,
    pub storage: VectorStorageKind,
    pub embedding_route: String,
    pub similarity_threshold: f64,
    pub vector_dimensions: usize,
    pub default_ttl: Duration,
    pub max_entries: usize,
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
                storage: CacheStorageKind::Memory,
                default_ttl: Duration::from_secs(3600),
                max_entries: 1000,
            },
            semantic: SemanticCacheConfig {
                enabled: false,
                storage: VectorStorageKind::Memory,
                embedding_route: String::new(),
                similarity_threshold: 0.92,
                vector_dimensions: 1536,
                default_ttl: Duration::from_secs(600),
                max_entries: 500,
            },
        }
    }
}

impl CacheConfig {
    pub fn to_admin_json(&self) -> Value {
        json!({
            "exact": {
                "enabled": self.exact.enabled,
                "storage": match self.exact.storage {
                    CacheStorageKind::Memory => "memory",
                    CacheStorageKind::Database => "database",
                },
                "default_ttl": self.exact.default_ttl.as_secs(),
                "max_entries": self.exact.max_entries,
            },
            "semantic": {
                "enabled": self.semantic.enabled,
                "storage": match self.semantic.storage {
                    VectorStorageKind::Memory => "memory",
                },
                "embedding_route": self.semantic.embedding_route,
                "similarity_threshold": self.semantic.similarity_threshold,
                "vector_dimensions": self.semantic.vector_dimensions,
                "default_ttl": self.semantic.default_ttl.as_secs(),
                "max_entries": self.semantic.max_entries,
            }
        })
    }

    pub fn from_admin_json(value: &Value) -> Option<Self> {
        let exact = value.get("exact")?;
        let semantic = value.get("semantic")?;

        let exact_enabled = exact.get("enabled")?.as_bool()?;
        let exact_storage = match exact
            .get("storage")?
            .as_str()?
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "database" => CacheStorageKind::Database,
            _ => CacheStorageKind::Memory,
        };
        let exact_default_ttl = exact.get("default_ttl")?.as_u64()?.max(1);
        let exact_max_entries = exact.get("max_entries")?.as_u64()?.max(1) as usize;

        let semantic_enabled = semantic.get("enabled")?.as_bool()?;
        let semantic_storage = match semantic
            .get("storage")?
            .as_str()?
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            _ => VectorStorageKind::Memory,
        };
        let embedding_route = semantic
            .get("embedding_route")?
            .as_str()?
            .trim()
            .to_string();
        let similarity_threshold = semantic.get("similarity_threshold")?.as_f64()?;
        let vector_dimensions = semantic.get("vector_dimensions")?.as_u64()?.max(1) as usize;
        let semantic_default_ttl = semantic.get("default_ttl")?.as_u64()?.max(1);
        let semantic_max_entries = semantic.get("max_entries")?.as_u64()?.max(1) as usize;

        Some(Self {
            exact: ExactCacheConfig {
                enabled: exact_enabled,
                storage: exact_storage,
                default_ttl: Duration::from_secs(exact_default_ttl),
                max_entries: exact_max_entries,
            },
            semantic: SemanticCacheConfig {
                enabled: semantic_enabled,
                storage: semantic_storage,
                embedding_route,
                similarity_threshold,
                vector_dimensions,
                default_ttl: Duration::from_secs(semantic_default_ttl),
                max_entries: semantic_max_entries,
            },
        })
    }
}
