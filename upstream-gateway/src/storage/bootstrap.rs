use std::path::Path;

use anyhow::Context;
use serde::Deserialize;

use crate::provider::GatewayProviderBundle;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BootstrapFile {
    Bundles(Vec<GatewayProviderBundle>),
    Root {
        providers: Vec<GatewayProviderBundle>,
    },
}

pub async fn load_provider_bundles(path: &Path) -> anyhow::Result<Vec<GatewayProviderBundle>> {
    let contents = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("failed to read bootstrap json {}", path.display()))?;

    let parsed = serde_json::from_str::<BootstrapFile>(&contents)
        .with_context(|| format!("failed to parse bootstrap json {}", path.display()))?;

    Ok(match parsed {
        BootstrapFile::Bundles(items) => items,
        BootstrapFile::Root { providers } => providers,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::config::{DailyResetConfig, ModelRateLimitRule};
    use crate::provider::{
        GatewayAuthStrategy, GatewayKey, GatewayModelRule, GatewayProvider, GatewayProviderBundle,
        ProviderVendor,
    };

    use super::load_provider_bundles;

    #[tokio::test]
    async fn bootstrap_loader_accepts_root_object_shape() {
        let path = temp_json_path("bootstrap-root");
        let payload = serde_json::json!({
            "providers": [sample_bundle_json()]
        });
        tokio::fs::write(&path, serde_json::to_vec(&payload).unwrap())
            .await
            .unwrap();

        let bundles = load_provider_bundles(&path).await.unwrap();
        assert_eq!(bundles.len(), 1);
        assert_eq!(bundles[0].provider.id, "openai-prod");

        let _ = tokio::fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn bootstrap_loader_accepts_array_shape() {
        let path = temp_json_path("bootstrap-array");
        let payload = serde_json::json!([sample_bundle_json()]);
        tokio::fs::write(&path, serde_json::to_vec(&payload).unwrap())
            .await
            .unwrap();

        let bundles = load_provider_bundles(&path).await.unwrap();
        assert_eq!(bundles.len(), 1);
        assert_eq!(bundles[0].provider.name, "OpenAI Prod");

        let _ = tokio::fs::remove_file(&path).await;
    }

    fn temp_json_path(prefix: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nonce}.json"))
    }

    fn sample_bundle_json() -> GatewayProviderBundle {
        GatewayProviderBundle {
            provider: GatewayProvider {
                id: "openai-prod".to_string(),
                name: "OpenAI Prod".to_string(),
                vendor: ProviderVendor::OpenAI,
                base_url: "https://api.openai.com".to_string(),
                auth_strategy: GatewayAuthStrategy::Bearer,
                enabled: true,
            },
            keys: vec![GatewayKey {
                id: "key-a".to_string(),
                provider_id: "openai-prod".to_string(),
                display_name: Some("Key A".to_string()),
                api_key: "sk-test-a".to_string(),
                enabled: true,
                weight: None,
            }],
            model_rules: vec![GatewayModelRule {
                provider_id: "openai-prod".to_string(),
                rule: ModelRateLimitRule {
                    model: "*".to_string(),
                    rpm: Some(60),
                    rpd: None,
                    tpm: None,
                    tpm_mode: None,
                    tokenizer_encoding: None,
                    tokenizer_model: None,
                },
            }],
            daily_reset: DailyResetConfig {
                timezone: "+00:00".to_string(),
                hour: 0,
                minute: 0,
            },
            normalized_rate_limit_config_cache: None,
        }
    }
}
