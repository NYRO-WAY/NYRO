use std::sync::Arc;

use crate::auth::drivers::OpenAIOAuthDriver;
use crate::auth::types::{AuthDriver, AuthDriverMetadata};

pub fn normalize_driver_key(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "openai-oauth" | "openai_oauth" | "openai" | "codex-cli" | "codex" => "codex".to_string(),
        other => other.to_string(),
    }
}

pub fn build_driver(key: &str) -> Option<Arc<dyn AuthDriver>> {
    match normalize_driver_key(key).as_str() {
        "codex" => Some(Arc::new(OpenAIOAuthDriver)),
        _ => None,
    }
}

pub fn list_driver_metadata() -> Vec<AuthDriverMetadata> {
    [build_driver("codex")]
        .into_iter()
        .flatten()
        .map(|driver| driver.metadata())
        .collect()
}
