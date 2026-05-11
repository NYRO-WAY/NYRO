use std::collections::BTreeSet;

use super::{
    ProtocolFamily, RaceGroup, RaceKeyPool, RaceModelDescriptor, ValidationErrorResponse,
    ValidationIssue,
};

const MIN_RESPONSE_PROTECTION_TIMEOUT_MS: u64 = 1_000;
const MAX_RESPONSE_PROTECTION_TIMEOUT_MS: u64 = 120_000;
const MAX_CANDIDATES_PER_GROUP: usize = 8;

#[derive(Debug, Default)]
pub struct ValidationBuilder {
    issues: Vec<ValidationIssue>,
}

impl ValidationBuilder {
    pub fn push(
        &mut self,
        field: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.issues.push(ValidationIssue {
            field: field.into(),
            code: code.into(),
            message: message.into(),
        });
    }

    pub fn finish(self) -> ValidationErrorResponse {
        ValidationErrorResponse {
            valid: self.issues.is_empty(),
            issues: self.issues,
        }
    }
}

pub fn validate_model_descriptor(model: &RaceModelDescriptor) -> ValidationErrorResponse {
    let mut builder = ValidationBuilder::default();

    if model.id.trim().is_empty() {
        builder.push("id", "required", "model id cannot be empty");
    }
    if model.display_name.trim().is_empty() {
        builder.push("display_name", "required", "display_name cannot be empty");
    }
    if model.upstream_model.trim().is_empty() {
        builder.push(
            "upstream_model",
            "required",
            "upstream_model cannot be empty",
        );
    }

    let enabled_endpoints = model
        .endpoints
        .iter()
        .filter(|endpoint| endpoint.enabled)
        .count();
    if enabled_endpoints == 0 {
        builder.push(
            "endpoints",
            "missing_enabled_endpoint",
            "at least one enabled endpoint is required",
        );
    }

    validate_endpoint_uniqueness(
        &mut builder,
        "endpoints",
        model.endpoints.iter().enumerate().map(|(index, endpoint)| {
            (
                format!("endpoints[{index}].protocol_family"),
                endpoint.protocol_family,
                endpoint.enabled,
            )
        }),
    );

    builder.finish()
}

pub fn validate_group(group: &RaceGroup) -> ValidationErrorResponse {
    let mut builder = ValidationBuilder::default();

    if group.id.trim().is_empty() {
        builder.push("id", "required", "group id cannot be empty");
    }
    if group.display_name.trim().is_empty() {
        builder.push("display_name", "required", "display_name cannot be empty");
    }
    if group.candidates.is_empty() {
        builder.push(
            "candidates",
            "missing_candidates",
            "at least one candidate is required",
        );
    }
    if group.candidates.len() > MAX_CANDIDATES_PER_GROUP {
        builder.push(
            "candidates",
            "too_many_candidates",
            format!("a group can contain at most {MAX_CANDIDATES_PER_GROUP} candidates"),
        );
    }

    let enabled_candidates = group
        .candidates
        .iter()
        .filter(|candidate| candidate.enabled)
        .count();
    if enabled_candidates == 0 {
        builder.push(
            "candidates",
            "missing_enabled_candidate",
            "at least one enabled candidate is required",
        );
    }

    let mut names = BTreeSet::new();
    for (index, candidate) in group.candidates.iter().enumerate() {
        let name_field = format!("candidates[{index}].name");
        if candidate.id.trim().is_empty() {
            builder.push(
                format!("candidates[{index}].id"),
                "required",
                "candidate id cannot be empty",
            );
        }
        if candidate.name.trim().is_empty() {
            builder.push(
                name_field.clone(),
                "required",
                "candidate name cannot be empty",
            );
        } else if !names.insert(candidate.name.trim().to_string()) {
            builder.push(
                name_field,
                "duplicate_candidate_name",
                "candidate name must be unique inside the group",
            );
        }
        if candidate
            .model_id
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
            && candidate.upstream_model.trim().is_empty()
        {
            builder.push(
                format!("candidates[{index}].upstream_model"),
                "required",
                "candidate must provide upstream_model or model_id",
            );
        }
        if candidate.response_protection_timeout_ms < MIN_RESPONSE_PROTECTION_TIMEOUT_MS
            || candidate.response_protection_timeout_ms > MAX_RESPONSE_PROTECTION_TIMEOUT_MS
        {
            builder.push(
                format!("candidates[{index}].response_protection_timeout_ms"),
                "out_of_range",
                format!(
                    "response_protection_timeout_ms must be between {MIN_RESPONSE_PROTECTION_TIMEOUT_MS} and {MAX_RESPONSE_PROTECTION_TIMEOUT_MS}"
                ),
            );
        }
        validate_endpoint_uniqueness(
            &mut builder,
            &format!("candidates[{index}].inline_endpoint_overrides"),
            candidate
                .inline_endpoint_overrides
                .iter()
                .enumerate()
                .map(|(endpoint_index, endpoint)| {
                    (
                        format!(
                            "candidates[{index}].inline_endpoint_overrides[{endpoint_index}].protocol_family"
                        ),
                        endpoint.protocol_family,
                        endpoint.enabled,
                    )
                }),
        );
    }

    builder.finish()
}

pub fn validate_key_pool(pool: &RaceKeyPool) -> ValidationErrorResponse {
    let mut builder = ValidationBuilder::default();

    if pool.id.trim().is_empty() {
        builder.push("id", "required", "key pool id cannot be empty");
    }
    if pool.display_name.trim().is_empty() {
        builder.push("display_name", "required", "display_name cannot be empty");
    }
    if pool.keys.iter().filter(|key| key.enabled).count() == 0 {
        builder.push(
            "keys",
            "missing_enabled_key",
            "at least one enabled key is required",
        );
    }

    let mut key_ids = BTreeSet::new();
    for (index, key) in pool.keys.iter().enumerate() {
        if key.id.trim().is_empty() {
            builder.push(
                format!("keys[{index}].id"),
                "required",
                "key id cannot be empty",
            );
        } else if !key_ids.insert(key.id.trim().to_string()) {
            builder.push(
                format!("keys[{index}].id"),
                "duplicate_key_id",
                "key id must be unique inside the pool",
            );
        }
        if key.secret.trim().is_empty() {
            builder.push(
                format!("keys[{index}].secret"),
                "required",
                "key secret cannot be empty",
            );
        }
    }

    builder.finish()
}

fn validate_endpoint_uniqueness<I>(builder: &mut ValidationBuilder, root_field: &str, endpoints: I)
where
    I: Iterator<Item = (String, ProtocolFamily, bool)>,
{
    let mut enabled_protocols = BTreeSet::new();
    for (field, protocol_family, enabled) in endpoints {
        if !enabled {
            continue;
        }
        if !enabled_protocols.insert(protocol_family) {
            builder.push(
                field,
                "duplicate_enabled_protocol",
                format!("{root_field} cannot contain duplicate enabled protocol_family"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::{AuthStrategy, RaceCandidate, RaceKey, RaceTargetEndpoint};

    #[test]
    fn group_validation_rejects_duplicate_candidate_names() {
        let group = RaceGroup {
            id: "g".to_string(),
            display_name: "G".to_string(),
            fallback_ratio: 0.5,
            decay_factor: 0.8,
            penalty_rate: 1.0,
            recovery_rate: 1.0,
            race_max_wait_time_ms: None,
            enabled: true,
            candidates: vec![
                RaceCandidate {
                    id: "a".to_string(),
                    group_id: "g".to_string(),
                    name: "dup".to_string(),
                    model_id: None,
                    upstream_model: "m".to_string(),
                    inline_endpoint_overrides: vec![],
                    initial_weight: 100.0,
                    response_protection_timeout_ms: 2_000,
                    enabled: true,
                    metadata: json!({}),
                },
                RaceCandidate {
                    id: "b".to_string(),
                    group_id: "g".to_string(),
                    name: "dup".to_string(),
                    model_id: None,
                    upstream_model: "m".to_string(),
                    inline_endpoint_overrides: vec![],
                    initial_weight: 90.0,
                    response_protection_timeout_ms: 2_000,
                    enabled: true,
                    metadata: json!({}),
                },
            ],
        };

        let result = validate_group(&group);
        assert!(!result.valid);
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.code == "duplicate_candidate_name")
        );
    }

    #[test]
    fn key_pool_validation_requires_enabled_key() {
        let pool = RaceKeyPool {
            id: "pool".to_string(),
            display_name: "Pool".to_string(),
            auth_strategy: AuthStrategy::Bearer,
            selection_strategy: Default::default(),
            enabled: true,
            keys: vec![RaceKey {
                id: "k1".to_string(),
                key_pool_id: "pool".to_string(),
                secret: "secret".to_string(),
                enabled: false,
                metadata: json!({}),
            }],
        };

        let result = validate_key_pool(&pool);
        assert!(!result.valid);
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.code == "missing_enabled_key")
        );
    }

    #[test]
    fn model_validation_rejects_duplicate_enabled_protocols() {
        let model = RaceModelDescriptor {
            id: "m".to_string(),
            display_name: "M".to_string(),
            upstream_model: "vendor/m".to_string(),
            description: String::new(),
            enabled: true,
            endpoints: vec![
                RaceTargetEndpoint {
                    protocol_family: ProtocolFamily::OpenAi,
                    base_url: "https://one.example".to_string(),
                    auth_strategy: AuthStrategy::Bearer,
                    key_pool_id: "pool".to_string(),
                    request_timeout_ms: None,
                    extra_headers: Default::default(),
                    extra_query: Default::default(),
                    enabled: true,
                },
                RaceTargetEndpoint {
                    protocol_family: ProtocolFamily::OpenAi,
                    base_url: "https://two.example".to_string(),
                    auth_strategy: AuthStrategy::Bearer,
                    key_pool_id: "pool".to_string(),
                    request_timeout_ms: None,
                    extra_headers: Default::default(),
                    extra_query: Default::default(),
                    enabled: true,
                },
            ],
            metadata: json!({}),
        };

        let result = validate_model_descriptor(&model);
        assert!(!result.valid);
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.code == "duplicate_enabled_protocol")
        );
    }

    #[test]
    fn group_validation_requires_candidate_model_source() {
        let group = RaceGroup {
            id: "g".to_string(),
            display_name: "G".to_string(),
            fallback_ratio: 0.5,
            decay_factor: 0.8,
            penalty_rate: 1.0,
            recovery_rate: 1.0,
            race_max_wait_time_ms: None,
            enabled: true,
            candidates: vec![RaceCandidate {
                id: "a".to_string(),
                group_id: "g".to_string(),
                name: "A".to_string(),
                model_id: None,
                upstream_model: String::new(),
                inline_endpoint_overrides: vec![],
                initial_weight: 100.0,
                response_protection_timeout_ms: 2_000,
                enabled: true,
                metadata: json!({}),
            }],
        };

        let result = validate_group(&group);
        assert!(!result.valid);
        assert!(result.issues.iter().any(|issue| issue.code == "required"));
    }
}
