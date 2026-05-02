use std::sync::Arc;

use crate::auth::drivers::{ClaudeOAuthDriver, OpenAIOAuthDriver};
use crate::auth::types::{AuthDriver, AuthDriverMetadata};
use crate::db::models::Provider;

pub fn normalize_driver_key(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

pub fn resolve_provider_driver_key(provider: &Provider) -> String {
    match provider
        .channel
        .as_deref()
        .map(normalize_driver_key)
        .as_deref()
    {
        Some("codex") => "codex".to_string(),
        Some("claude-code") => "claude-code".to_string(),
        _ => String::new(),
    }
}

pub fn build_driver(key: &str) -> Option<Arc<dyn AuthDriver>> {
    match normalize_driver_key(key).as_str() {
        "codex" => Some(Arc::new(OpenAIOAuthDriver)),
        "claude-code" => Some(Arc::new(ClaudeOAuthDriver)),
        _ => None,
    }
}

pub fn list_driver_metadata() -> Vec<AuthDriverMetadata> {
    [build_driver("codex"), build_driver("claude-code")]
        .into_iter()
        .flatten()
        .map(|driver| driver.metadata())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(vendor: Option<&str>, channel: Option<&str>) -> Provider {
        Provider {
            id: "test".into(),
            name: "test".into(),
            vendor: vendor.map(ToString::to_string),
            protocol: "anthropic".into(),
            base_url: String::new(),
            default_protocol: String::new(),
            protocol_endpoints: String::new(),
            preset_key: vendor.map(ToString::to_string),
            channel: channel.map(ToString::to_string),
            models_source: None,
            capabilities_source: None,
            static_models: None,
            api_key: String::new(),
            auth_mode: "apikey".into(),
            use_proxy: false,
            last_test_success: None,
            last_test_at: None,
            is_enabled: true,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn normalize_driver_key_does_not_remap_provider_names() {
        assert_eq!(normalize_driver_key("openai"), "openai");
        assert_eq!(normalize_driver_key("anthropic"), "anthropic");
        assert_eq!(normalize_driver_key("codex"), "codex");
        assert_eq!(normalize_driver_key("claude-code"), "claude-code");
    }

    #[test]
    fn resolve_provider_driver_key_only_uses_explicit_oauth_channels() {
        assert_eq!(
            resolve_provider_driver_key(&provider(Some("openai"), Some("codex"))),
            "codex"
        );
        assert_eq!(
            resolve_provider_driver_key(&provider(Some("anthropic"), Some("claude-code"))),
            "claude-code"
        );
        assert_eq!(
            resolve_provider_driver_key(&provider(Some("anthropic"), Some("default"))),
            ""
        );
        assert_eq!(
            resolve_provider_driver_key(&provider(Some("openai"), None)),
            ""
        );
    }
}
