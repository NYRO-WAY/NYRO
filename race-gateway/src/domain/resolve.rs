use std::collections::BTreeMap;

use anyhow::{Context, bail};

use super::{
    ProtocolFamily, RaceCandidate, RaceGroup, RaceModelDescriptor, RaceTargetEndpoint,
    ResolvedCandidateTarget, ResolvedEndpointSource,
};

pub fn resolve_candidates_for_protocol(
    group: &RaceGroup,
    models: &BTreeMap<String, RaceModelDescriptor>,
    protocol_family: ProtocolFamily,
) -> anyhow::Result<Vec<ResolvedCandidateTarget>> {
    let mut resolved = Vec::new();

    for candidate in group
        .candidates
        .iter()
        .filter(|candidate| candidate.enabled)
    {
        if let Some(target) = resolve_single_candidate(candidate, models, protocol_family)? {
            resolved.push(target);
        }
    }

    if resolved.is_empty() {
        bail!(
            "group '{}' does not have any enabled candidate for protocol '{}'",
            group.id,
            protocol_label(protocol_family)
        );
    }

    Ok(resolved)
}

pub fn resolve_single_candidate(
    candidate: &RaceCandidate,
    models: &BTreeMap<String, RaceModelDescriptor>,
    protocol_family: ProtocolFamily,
) -> anyhow::Result<Option<ResolvedCandidateTarget>> {
    if !candidate.enabled {
        return Ok(None);
    }

    let resolved_model = resolve_model_for_candidate(candidate, models)?;
    let effective_upstream_model =
        candidate_effective_upstream_model(candidate, resolved_model.as_ref());
    let mut resolved_candidate = candidate.clone();
    resolved_candidate.upstream_model = effective_upstream_model;

    if resolved_candidate.upstream_model.is_empty() {
        return Ok(None);
    }

    if let Some(endpoint) =
        find_matching_endpoint(&candidate.inline_endpoint_overrides, protocol_family)
    {
        return Ok(Some(ResolvedCandidateTarget {
            candidate: resolved_candidate,
            key_pool_id: endpoint.key_pool_id.clone(),
            endpoint,
            endpoint_source: ResolvedEndpointSource::CandidateInlineOverride,
        }));
    }

    let Some(model) = resolved_model else {
        return Ok(None);
    };
    if !model.enabled {
        return Ok(None);
    }

    Ok(
        find_matching_endpoint(&model.endpoints, protocol_family).map(|endpoint| {
            ResolvedCandidateTarget {
                candidate: resolved_candidate,
                key_pool_id: endpoint.key_pool_id.clone(),
                endpoint,
                endpoint_source: ResolvedEndpointSource::ModelDescriptor,
            }
        }),
    )
}

pub fn resolve_model_for_candidate(
    candidate: &RaceCandidate,
    models: &BTreeMap<String, RaceModelDescriptor>,
) -> anyhow::Result<Option<RaceModelDescriptor>> {
    if let Some(model_id) = candidate.model_id.as_deref() {
        return models
            .get(model_id)
            .cloned()
            .with_context(|| {
                format!(
                    "candidate '{}' references missing model '{model_id}'",
                    candidate.name
                )
            })
            .map(Some);
    }

    let upstream_model = candidate.upstream_model.trim();
    if upstream_model.is_empty() {
        return Ok(None);
    }

    let matches = models
        .values()
        .filter(|model| model.upstream_model == upstream_model)
        .cloned()
        .collect::<Vec<_>>();

    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.into_iter().next()),
        _ => bail!(
            "candidate '{}' upstream model '{}' matches multiple model descriptors; please bind model_id explicitly",
            candidate.name,
            upstream_model
        ),
    }
}

pub fn candidate_effective_upstream_model(
    candidate: &RaceCandidate,
    resolved_model: Option<&RaceModelDescriptor>,
) -> String {
    let upstream_model = candidate.upstream_model.trim();
    if !upstream_model.is_empty() {
        return upstream_model.to_string();
    }

    resolved_model
        .map(|model| model.upstream_model.trim().to_string())
        .unwrap_or_default()
}

fn find_matching_endpoint(
    endpoints: &[RaceTargetEndpoint],
    protocol_family: ProtocolFamily,
) -> Option<RaceTargetEndpoint> {
    endpoints
        .iter()
        .find(|endpoint| endpoint.enabled && endpoint.protocol_family == protocol_family)
        .cloned()
}

fn protocol_label(protocol_family: ProtocolFamily) -> &'static str {
    match protocol_family {
        ProtocolFamily::OpenAi => "openai",
        ProtocolFamily::Anthropic => "anthropic",
        ProtocolFamily::Google => "google",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::{AuthStrategy, RaceTargetEndpoint};

    fn endpoint(protocol_family: ProtocolFamily, base_url: &str) -> RaceTargetEndpoint {
        RaceTargetEndpoint {
            protocol_family,
            base_url: base_url.to_string(),
            auth_strategy: AuthStrategy::Bearer,
            key_pool_id: "pool-a".to_string(),
            request_timeout_ms: Some(30_000),
            extra_headers: Default::default(),
            extra_query: Default::default(),
            enabled: true,
        }
    }

    #[test]
    fn inline_override_wins_over_model_endpoint() {
        let candidate = RaceCandidate {
            id: "cand-a".to_string(),
            group_id: "group-a".to_string(),
            name: "A".to_string(),
            model_id: Some("model-a".to_string()),
            upstream_model: "vendor/model-a".to_string(),
            inline_endpoint_overrides: vec![endpoint(
                ProtocolFamily::OpenAi,
                "https://inline.example",
            )],
            initial_weight: 100.0,
            response_protection_timeout_ms: 5_000,
            enabled: true,
            metadata: json!({}),
        };
        let model = RaceModelDescriptor {
            id: "model-a".to_string(),
            display_name: "Model A".to_string(),
            upstream_model: "vendor/model-a".to_string(),
            description: String::new(),
            enabled: true,
            endpoints: vec![endpoint(ProtocolFamily::OpenAi, "https://model.example")],
            metadata: json!({}),
        };

        let resolved = resolve_single_candidate(
            &candidate,
            &BTreeMap::from([(model.id.clone(), model)]),
            ProtocolFamily::OpenAi,
        )
        .expect("resolve candidate")
        .expect("resolved target");

        assert_eq!(resolved.endpoint.base_url, "https://inline.example");
        assert_eq!(
            resolved.endpoint_source,
            ResolvedEndpointSource::CandidateInlineOverride
        );
    }

    #[test]
    fn group_resolution_skips_candidates_without_protocol_endpoint() {
        let group = RaceGroup {
            id: "group-a".to_string(),
            display_name: "Group A".to_string(),
            fallback_ratio: 0.5,
            decay_factor: 0.8,
            penalty_rate: 5.0,
            recovery_rate: 0.1,
            race_max_wait_time_ms: None,
            enabled: true,
            candidates: vec![
                RaceCandidate {
                    id: "cand-a".to_string(),
                    group_id: "group-a".to_string(),
                    name: "A".to_string(),
                    model_id: Some("model-a".to_string()),
                    upstream_model: "vendor/model-a".to_string(),
                    inline_endpoint_overrides: vec![],
                    initial_weight: 100.0,
                    response_protection_timeout_ms: 5_000,
                    enabled: true,
                    metadata: json!({}),
                },
                RaceCandidate {
                    id: "cand-b".to_string(),
                    group_id: "group-a".to_string(),
                    name: "B".to_string(),
                    model_id: Some("model-b".to_string()),
                    upstream_model: "vendor/model-b".to_string(),
                    inline_endpoint_overrides: vec![],
                    initial_weight: 90.0,
                    response_protection_timeout_ms: 5_000,
                    enabled: true,
                    metadata: json!({}),
                },
            ],
        };

        let models = BTreeMap::from([
            (
                "model-a".to_string(),
                RaceModelDescriptor {
                    id: "model-a".to_string(),
                    display_name: "Model A".to_string(),
                    upstream_model: "vendor/model-a".to_string(),
                    description: String::new(),
                    enabled: true,
                    endpoints: vec![endpoint(
                        ProtocolFamily::Anthropic,
                        "https://anthropic.example",
                    )],
                    metadata: json!({}),
                },
            ),
            (
                "model-b".to_string(),
                RaceModelDescriptor {
                    id: "model-b".to_string(),
                    display_name: "Model B".to_string(),
                    upstream_model: "vendor/model-b".to_string(),
                    description: String::new(),
                    enabled: true,
                    endpoints: vec![endpoint(ProtocolFamily::OpenAi, "https://openai.example")],
                    metadata: json!({}),
                },
            ),
        ]);

        let resolved = resolve_candidates_for_protocol(&group, &models, ProtocolFamily::OpenAi)
            .expect("resolve group");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].candidate.name, "B");
    }

    #[test]
    fn candidate_without_model_id_can_resolve_model_by_upstream_model() {
        let candidate = RaceCandidate {
            id: "cand-a".to_string(),
            group_id: "group-a".to_string(),
            name: "A".to_string(),
            model_id: None,
            upstream_model: "vendor/model-a".to_string(),
            inline_endpoint_overrides: vec![],
            initial_weight: 100.0,
            response_protection_timeout_ms: 5_000,
            enabled: true,
            metadata: json!({}),
        };
        let model = RaceModelDescriptor {
            id: "model-a".to_string(),
            display_name: "Model A".to_string(),
            upstream_model: "vendor/model-a".to_string(),
            description: String::new(),
            enabled: true,
            endpoints: vec![endpoint(ProtocolFamily::OpenAi, "https://model.example")],
            metadata: json!({}),
        };

        let resolved = resolve_single_candidate(
            &candidate,
            &BTreeMap::from([(model.id.clone(), model)]),
            ProtocolFamily::OpenAi,
        )
        .expect("resolve candidate")
        .expect("resolved target");

        assert_eq!(resolved.endpoint.base_url, "https://model.example");
        assert_eq!(resolved.candidate.upstream_model, "vendor/model-a");
    }

    #[test]
    fn candidate_with_blank_upstream_model_uses_bound_model_upstream() {
        let candidate = RaceCandidate {
            id: "cand-a".to_string(),
            group_id: "group-a".to_string(),
            name: "A".to_string(),
            model_id: Some("model-a".to_string()),
            upstream_model: String::new(),
            inline_endpoint_overrides: vec![],
            initial_weight: 100.0,
            response_protection_timeout_ms: 5_000,
            enabled: true,
            metadata: json!({}),
        };
        let model = RaceModelDescriptor {
            id: "model-a".to_string(),
            display_name: "Model A".to_string(),
            upstream_model: "vendor/model-a".to_string(),
            description: String::new(),
            enabled: true,
            endpoints: vec![endpoint(ProtocolFamily::OpenAi, "https://model.example")],
            metadata: json!({}),
        };

        let resolved = resolve_single_candidate(
            &candidate,
            &BTreeMap::from([(model.id.clone(), model)]),
            ProtocolFamily::OpenAi,
        )
        .expect("resolve candidate")
        .expect("resolved target");

        assert_eq!(resolved.candidate.upstream_model, "vendor/model-a");
    }

    #[test]
    fn candidate_upstream_model_lookup_rejects_ambiguous_match() {
        let candidate = RaceCandidate {
            id: "cand-a".to_string(),
            group_id: "group-a".to_string(),
            name: "A".to_string(),
            model_id: None,
            upstream_model: "vendor/shared".to_string(),
            inline_endpoint_overrides: vec![],
            initial_weight: 100.0,
            response_protection_timeout_ms: 5_000,
            enabled: true,
            metadata: json!({}),
        };
        let model_a = RaceModelDescriptor {
            id: "model-a".to_string(),
            display_name: "Model A".to_string(),
            upstream_model: "vendor/shared".to_string(),
            description: String::new(),
            enabled: true,
            endpoints: vec![endpoint(ProtocolFamily::OpenAi, "https://a.example")],
            metadata: json!({}),
        };
        let model_b = RaceModelDescriptor {
            id: "model-b".to_string(),
            display_name: "Model B".to_string(),
            upstream_model: "vendor/shared".to_string(),
            description: String::new(),
            enabled: true,
            endpoints: vec![endpoint(ProtocolFamily::OpenAi, "https://b.example")],
            metadata: json!({}),
        };

        let error = resolve_single_candidate(
            &candidate,
            &BTreeMap::from([(model_a.id.clone(), model_a), (model_b.id.clone(), model_b)]),
            ProtocolFamily::OpenAi,
        )
        .expect_err("ambiguous upstream model should fail");

        assert!(
            error
                .to_string()
                .contains("matches multiple model descriptors")
        );
    }
}
