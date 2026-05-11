use chrono::{TimeZone, Utc};
use upstream_gateway::config::{
    DailyResetConfig, GatewayKeyConfig, GatewayProviderRateLimitConfig, ModelRateLimitRule, TpmMode,
};
use upstream_gateway::runtime::{KeySelectionInput, SettlementUsage};
use upstream_gateway::selector::DefaultKeySelector;

fn sample_config(models: Vec<ModelRateLimitRule>) -> GatewayProviderRateLimitConfig {
    GatewayProviderRateLimitConfig {
        key_pool: vec![
            GatewayKeyConfig {
                id: "key-a".to_string(),
                api_key: "sk-a".to_string(),
                enabled: true,
                weight: 1,
            },
            GatewayKeyConfig {
                id: "key-b".to_string(),
                api_key: "sk-b".to_string(),
                enabled: true,
                weight: 1,
            },
        ],
        daily_reset: DailyResetConfig {
            timezone: "+00:00".to_string(),
            hour: 0,
            minute: 0,
        },
        models,
        enabled_key_slots_cache: Vec::new(),
        total_weight_slots_cache: 0,
    }
    .normalized()
    .unwrap()
}

fn single_key_config(models: Vec<ModelRateLimitRule>) -> GatewayProviderRateLimitConfig {
    GatewayProviderRateLimitConfig {
        key_pool: vec![GatewayKeyConfig {
            id: "key-a".to_string(),
            api_key: "sk-a".to_string(),
            enabled: true,
            weight: 1,
        }],
        daily_reset: DailyResetConfig {
            timezone: "+00:00".to_string(),
            hour: 0,
            minute: 0,
        },
        models,
        enabled_key_slots_cache: Vec::new(),
        total_weight_slots_cache: 0,
    }
    .normalized()
    .unwrap()
}

fn input<'a>(
    model: &'a str,
    request_input_tokens: u32,
    request_output_reservation: u32,
) -> KeySelectionInput<'a> {
    KeySelectionInput {
        provider_id: "provider-1",
        provider_name: "Provider 1",
        actual_model: model,
        request_input_tokens,
        request_output_reservation,
    }
}

#[test]
fn wildcard_rule_is_used_when_exact_rule_missing() {
    let config = sample_config(vec![
        ModelRateLimitRule {
            model: "*".to_string(),
            rpm: Some(2),
            rpd: None,
            tpm: None,
            tpm_mode: None,
            tokenizer_encoding: None,
            tokenizer_model: None,
        },
        ModelRateLimitRule {
            model: "gpt-4o".to_string(),
            rpm: Some(1),
            rpd: None,
            tpm: None,
            tpm_mode: None,
            tokenizer_encoding: None,
            tokenizer_model: None,
        },
    ]);
    let selector = DefaultKeySelector::new();
    let now = Utc
        .with_ymd_and_hms(2026, 5, 5, 12, 0, 0)
        .unwrap()
        .timestamp_millis();

    let exact = selector
        .select_key_at(&config, &input("gpt-4o", 1, 0), now)
        .unwrap();
    let wildcard = selector
        .select_key_at(&config, &input("gpt-4.1", 1, 0), now)
        .unwrap();

    assert_eq!(exact.key_id, "key-a");
    assert_eq!(wildcard.key_id, "key-a");
}

#[test]
fn rpm_is_checked_before_rpd_and_tpm() {
    let config = single_key_config(vec![ModelRateLimitRule {
        model: "*".to_string(),
        rpm: Some(1),
        rpd: Some(1),
        tpm: Some(1),
        tpm_mode: Some(TpmMode::InputOnly),
        tokenizer_encoding: None,
        tokenizer_model: None,
    }]);
    let selector = DefaultKeySelector::new();
    let now = Utc
        .with_ymd_and_hms(2026, 5, 5, 12, 0, 0)
        .unwrap()
        .timestamp_millis();

    selector
        .select_key_at(&config, &input("gpt-4o", 1, 0), now)
        .unwrap();
    let error = selector
        .select_key_at(&config, &input("gpt-4o", 2, 0), now)
        .unwrap_err();

    assert!(error.to_string().contains("rpm quota exceeded"));
    assert!(config.key_pool.iter().all(|key| key.enabled));
}

#[test]
fn selector_rotates_from_last_hit_position() {
    let config = sample_config(vec![]);
    let selector = DefaultKeySelector::new();
    let now = Utc
        .with_ymd_and_hms(2026, 5, 5, 12, 0, 0)
        .unwrap()
        .timestamp_millis();

    let first = selector
        .select_key_at(&config, &input("gpt-4o", 1, 0), now)
        .unwrap();
    let second = selector
        .select_key_at(&config, &input("gpt-4o", 1, 0), now)
        .unwrap();
    let third = selector
        .select_key_at(&config, &input("gpt-4o", 1, 0), now)
        .unwrap();

    assert_eq!(first.key_id, "key-a");
    assert_eq!(second.key_id, "key-b");
    assert_eq!(third.key_id, "key-a");
}

#[test]
fn selector_respects_configured_key_weights() {
    let config = GatewayProviderRateLimitConfig {
        key_pool: vec![
            GatewayKeyConfig {
                id: "key-a".to_string(),
                api_key: "sk-a".to_string(),
                enabled: true,
                weight: 1,
            },
            GatewayKeyConfig {
                id: "key-b".to_string(),
                api_key: "sk-b".to_string(),
                enabled: true,
                weight: 2,
            },
        ],
        daily_reset: DailyResetConfig {
            timezone: "+00:00".to_string(),
            hour: 0,
            minute: 0,
        },
        models: vec![],
        enabled_key_slots_cache: Vec::new(),
        total_weight_slots_cache: 0,
    }
    .normalized()
    .unwrap();
    let selector = DefaultKeySelector::new();
    let now = Utc
        .with_ymd_and_hms(2026, 5, 5, 12, 0, 0)
        .unwrap()
        .timestamp_millis();

    let first = selector
        .select_key_at(&config, &input("gpt-4o", 1, 0), now)
        .unwrap();
    let second = selector
        .select_key_at(&config, &input("gpt-4o", 1, 0), now)
        .unwrap();
    let third = selector
        .select_key_at(&config, &input("gpt-4o", 1, 0), now)
        .unwrap();
    let fourth = selector
        .select_key_at(&config, &input("gpt-4o", 1, 0), now)
        .unwrap();

    assert_eq!(first.key_id, "key-a");
    assert_eq!(second.key_id, "key-b");
    assert_eq!(third.key_id, "key-b");
    assert_eq!(fourth.key_id, "key-a");
}

#[test]
fn saturated_heavy_weight_key_does_not_block_next_available_key() {
    let config = GatewayProviderRateLimitConfig {
        key_pool: vec![
            GatewayKeyConfig {
                id: "key-a".to_string(),
                api_key: "sk-a".to_string(),
                enabled: true,
                weight: 10_000,
            },
            GatewayKeyConfig {
                id: "key-b".to_string(),
                api_key: "sk-b".to_string(),
                enabled: true,
                weight: 1,
            },
        ],
        daily_reset: DailyResetConfig {
            timezone: "+00:00".to_string(),
            hour: 0,
            minute: 0,
        },
        models: vec![ModelRateLimitRule {
            model: "*".to_string(),
            rpm: Some(1),
            rpd: None,
            tpm: None,
            tpm_mode: None,
            tokenizer_encoding: None,
            tokenizer_model: None,
        }],
        enabled_key_slots_cache: Vec::new(),
        total_weight_slots_cache: 0,
    }
    .normalized()
    .unwrap();
    let selector = DefaultKeySelector::new();
    let now = Utc
        .with_ymd_and_hms(2026, 5, 5, 12, 0, 0)
        .unwrap()
        .timestamp_millis();

    let first = selector
        .select_key_at(&config, &input("gpt-4o", 1, 0), now)
        .unwrap();
    let second = selector
        .select_key_at(&config, &input("gpt-4o", 1, 0), now)
        .unwrap();

    assert_eq!(first.key_id, "key-a");
    assert_eq!(second.key_id, "key-b");
}

#[test]
fn sliding_windows_are_cleaned_lazily() {
    let config = sample_config(vec![ModelRateLimitRule {
        model: "*".to_string(),
        rpm: Some(1),
        rpd: None,
        tpm: Some(10),
        tpm_mode: Some(TpmMode::InputOnly),
        tokenizer_encoding: None,
        tokenizer_model: None,
    }]);
    let selector = DefaultKeySelector::new();
    let first_at = Utc
        .with_ymd_and_hms(2026, 5, 5, 12, 0, 0)
        .unwrap()
        .timestamp_millis();
    let second_at = first_at + 61_000;

    selector
        .select_key_at(&config, &input("gpt-4o", 5, 0), first_at)
        .unwrap();
    let next = selector
        .select_key_at(&config, &input("gpt-4o", 5, 0), second_at)
        .unwrap();

    assert_eq!(next.key_id, "key-b");
}

#[test]
fn rpd_fixed_window_refreshes_at_configured_boundary() {
    let config = single_key_config(vec![ModelRateLimitRule {
        model: "*".to_string(),
        rpm: None,
        rpd: Some(1),
        tpm: None,
        tpm_mode: None,
        tokenizer_encoding: None,
        tokenizer_model: None,
    }]);
    let selector = DefaultKeySelector::new();
    let day_one = Utc
        .with_ymd_and_hms(2026, 5, 5, 10, 0, 0)
        .unwrap()
        .timestamp_millis();
    let day_two = Utc
        .with_ymd_and_hms(2026, 5, 6, 0, 1, 0)
        .unwrap()
        .timestamp_millis();

    selector
        .select_key_at(&config, &input("gpt-4o", 1, 0), day_one)
        .unwrap();
    let exhausted = selector
        .select_key_at(&config, &input("gpt-4o", 1, 0), day_one)
        .unwrap_err();
    let reset = selector
        .select_key_at(&config, &input("gpt-4o", 1, 0), day_two)
        .unwrap();

    assert!(exhausted.to_string().contains("rpd quota exceeded"));
    assert_eq!(reset.key_id, "key-a");
}

#[test]
fn settle_reconciles_tpm_reservation_and_rollback_releases_it() {
    let config = single_key_config(vec![ModelRateLimitRule {
        model: "*".to_string(),
        rpm: None,
        rpd: None,
        tpm: Some(30),
        tpm_mode: Some(TpmMode::InputAndOutput),
        tokenizer_encoding: None,
        tokenizer_model: None,
    }]);
    let selector = DefaultKeySelector::new();
    let now = Utc
        .with_ymd_and_hms(2026, 5, 5, 12, 0, 0)
        .unwrap()
        .timestamp_millis();

    let first = selector
        .select_key_at(&config, &input("gpt-4o", 10, 20), now)
        .unwrap();
    let exhausted = selector
        .select_key_at(&config, &input("gpt-4o", 1, 0), now)
        .unwrap_err();
    assert!(exhausted.to_string().contains("tpm quota exceeded"));

    selector
        .settle_at(
            &first.lease,
            SettlementUsage {
                input_tokens: 10,
                output_tokens: 5,
            },
            now,
        )
        .unwrap();

    let after_settle = selector
        .select_key_at(&config, &input("gpt-4o", 10, 5), now)
        .unwrap();
    selector.rollback_at(&after_settle.lease, now).unwrap();

    let after_rollback = selector
        .select_key_at(&config, &input("gpt-4o", 10, 5), now)
        .unwrap();
    assert_eq!(after_rollback.key_id, "key-a");
}
