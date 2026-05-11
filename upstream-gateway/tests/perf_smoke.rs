use std::collections::HashSet;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use upstream_gateway::config::{
    DailyResetConfig, GatewayKeyConfig, GatewayProviderRateLimitConfig, ModelRateLimitRule, TpmMode,
};
use upstream_gateway::provider::{
    GatewayAuthStrategy, GatewayKey, GatewayModelRule, GatewayProvider, GatewayProviderBundle,
    ProviderVendor,
};
use upstream_gateway::selector::DefaultKeySelector;
use upstream_gateway::storage::{GatewayConfigStore, SqliteGatewayConfigStore};

fn temp_db_url(prefix: &str) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{nonce}.sqlite"));
    let normalized = path.to_string_lossy().replace('\\', "/");
    format!("sqlite://{normalized}")
}

fn large_bundle(provider_index: usize, key_count: usize) -> GatewayProviderBundle {
    let provider_id = format!("provider-{provider_index:03}");
    GatewayProviderBundle {
        provider: GatewayProvider {
            id: provider_id.clone(),
            name: format!("Provider {provider_index:03}"),
            vendor: ProviderVendor::OpenAI,
            base_url: "https://api.openai.com".to_string(),
            auth_strategy: GatewayAuthStrategy::Bearer,
            enabled: true,
        },
        keys: (0..key_count)
            .map(|key_index| GatewayKey {
                id: format!("key-{key_index:03}"),
                provider_id: provider_id.clone(),
                display_name: Some(format!("Key {key_index:03}")),
                api_key: format!("sk-test-{provider_index:03}-{key_index:03}"),
                enabled: true,
                weight: Some(((key_index % 4) + 1) as u32),
            })
            .collect(),
        model_rules: vec![GatewayModelRule {
            provider_id: provider_id.clone(),
            rule: ModelRateLimitRule {
                model: "*".to_string(),
                rpm: Some(10_000),
                rpd: Some(1_000_000),
                tpm: Some(10_000_000),
                tpm_mode: Some(TpmMode::InputAndOutput),
                tokenizer_encoding: None,
                tokenizer_model: Some("gpt-4o".to_string()),
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

#[tokio::test]
#[ignore = "performance smoke test; run manually with --ignored --nocapture"]
async fn sqlite_store_perf_smoke_many_providers_and_keys() {
    let provider_count = 48usize;
    let key_count = 128usize;
    let bundles = (0..provider_count)
        .map(|provider_index| large_bundle(provider_index, key_count))
        .collect::<Vec<_>>();

    let store = SqliteGatewayConfigStore::connect(&temp_db_url("perf-sqlite"))
        .await
        .unwrap();
    store.replace_all_provider_bundles(&bundles).await.unwrap();

    let warm_start = Instant::now();
    let warm = store.list_provider_bundles_shared().await.unwrap();
    let warm_elapsed = warm_start.elapsed();
    assert_eq!(warm.len(), provider_count);
    assert_eq!(warm[0].keys.len(), key_count);

    let read_start = Instant::now();
    let mut total_keys_seen = 0usize;
    for round in 0..4_000usize {
        let provider_id = format!("provider-{:03}", round % provider_count);
        let bundle = store
            .get_provider_bundle_shared(&provider_id)
            .await
            .unwrap()
            .unwrap();
        total_keys_seen += bundle.keys.len();
    }
    let read_elapsed = read_start.elapsed();
    assert_eq!(total_keys_seen, 4_000 * key_count);

    let list_start = Instant::now();
    let mut total_bundles_seen = 0usize;
    for _ in 0..100usize {
        total_bundles_seen += store.list_provider_bundles_shared().await.unwrap().len();
    }
    let list_elapsed = list_start.elapsed();
    assert_eq!(total_bundles_seen, provider_count * 100);

    eprintln!(
        "perf_smoke sqlite: warm={:?} repeated_shared_reads={:?} repeated_bulk_lists={:?}",
        warm_elapsed, read_elapsed, list_elapsed
    );
}

#[test]
#[ignore = "performance smoke test; run manually with --ignored --nocapture"]
fn selector_perf_smoke_many_keys_preserves_rotation_logic() {
    let key_count = 128usize;
    let config = GatewayProviderRateLimitConfig {
        key_pool: (0..key_count)
            .map(|key_index| GatewayKeyConfig {
                id: format!("key-{key_index:03}"),
                api_key: format!("sk-{key_index:03}"),
                enabled: true,
                weight: ((key_index % 5) + 1) as u32,
            })
            .collect(),
        daily_reset: DailyResetConfig {
            timezone: "+00:00".to_string(),
            hour: 0,
            minute: 0,
        },
        models: vec![ModelRateLimitRule {
            model: "*".to_string(),
            rpm: Some(100_000),
            rpd: Some(1_000_000),
            tpm: Some(100_000_000),
            tpm_mode: Some(TpmMode::InputAndOutput),
            tokenizer_encoding: None,
            tokenizer_model: Some("gpt-4o".to_string()),
        }],
        enabled_key_slots_cache: Vec::new(),
        total_weight_slots_cache: 0,
    }
    .normalized()
    .unwrap();

    let selector = DefaultKeySelector::new();
    let mut seen_keys = HashSet::new();
    let now = 1_746_000_000_000i64;

    let started_at = Instant::now();
    for request_index in 0..20_000usize {
        let selected = selector
            .select_key_at(
                &config,
                &upstream_gateway::runtime::KeySelectionInput {
                    provider_id: "provider-perf",
                    provider_name: "Provider Perf",
                    actual_model: "gpt-4o",
                    request_input_tokens: 128,
                    request_output_reservation: 256,
                },
                now + request_index as i64,
            )
            .unwrap();
        seen_keys.insert(selected.key_id.clone());
        selector.rollback_at(&selected.lease, now + request_index as i64).unwrap();
    }
    let elapsed = started_at.elapsed();

    assert!(seen_keys.len() > key_count / 2);
    eprintln!(
        "perf_smoke selector: requests=20000 distinct_keys_seen={} elapsed={:?}",
        seen_keys.len(),
        elapsed
    );
}
