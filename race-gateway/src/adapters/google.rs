use std::{borrow::Cow, collections::BTreeMap};

use bytes::Bytes;
use serde::Deserialize;
use serde::de::IgnoredAny;

use crate::domain::{DownstreamRouteKind, ProtocolFamily};
use crate::group::CandidateState;

use super::{EventAnalysis, RaceProtocolAdapter};

#[derive(Debug, Clone, Copy)]
pub struct GoogleAdapter {
    route_kind: DownstreamRouteKind,
}

impl GoogleAdapter {
    pub fn new(route_kind: DownstreamRouteKind) -> Self {
        Self { route_kind }
    }
}

impl RaceProtocolAdapter for GoogleAdapter {
    fn protocol_family(&self) -> ProtocolFamily {
        ProtocolFamily::Google
    }

    fn route_kind(&self) -> DownstreamRouteKind {
        self.route_kind
    }

    fn protocol_label(&self) -> &'static str {
        "google"
    }

    fn inspect_event(&self, state: &mut CandidateState, event: &[u8]) -> EventAnalysis {
        let Some(event) = std::str::from_utf8(event).ok() else {
            return EventAnalysis::default();
        };

        let mut analysis = EventAnalysis::default();
        for payload in iter_sse_data(event) {
            let Ok(value) = serde_json::from_str::<GoogleChunk<'_>>(payload) else {
                continue;
            };
            if value
                .candidates
                .iter()
                .flat_map(|candidate| candidate.content.parts.iter())
                .any(|part| {
                    part.text.as_deref().is_some_and(|text| !text.is_empty())
                        || part.function_call.is_some()
                        || part.executable_code.is_some()
                        || part.code_execution_result.is_some()
                })
            {
                analysis.has_content = true;
            }
            if value.candidates.iter().any(|item| {
                item.finish_reason
                    .as_deref()
                    .is_some_and(|reason| !reason.is_empty())
            }) {
                state.protocol_flags.emitted_finish = true;
            }
        }

        analysis
    }

    fn all_failed_events(&self, group_name: &str, errors: &BTreeMap<String, String>) -> Vec<Bytes> {
        let message = format!(
            "All candidates failed in group '{group_name}': {}",
            errors
                .iter()
                .map(|(name, reason)| format!("{name}: {reason}"))
                .collect::<Vec<_>>()
                .join("; ")
        );
        vec![Bytes::from(format!(
            "data: {}\n\n",
            serde_json::json!({
                "candidates": [{
                    "content": {
                        "role": "model",
                        "parts": [{"text": message}]
                    },
                    "finishReason": "STOP"
                }]
            })
        ))]
    }

    fn fallback_close_events(&self, _group_name: &str, state: &CandidateState) -> Vec<Bytes> {
        if state.protocol_flags.emitted_finish {
            Vec::new()
        } else {
            vec![Bytes::from(format!(
                "data: {}\n\n",
                serde_json::json!({
                    "candidates": [{
                        "content": {"role": "model", "parts": []},
                        "finishReason": "STOP"
                    }]
                })
            ))]
        }
    }
}

fn iter_sse_data(event: &str) -> impl Iterator<Item = &str> {
    event
        .lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim))
}

#[derive(Debug, Deserialize)]
struct GoogleChunk<'a> {
    #[serde(default, borrow)]
    candidates: Vec<GoogleCandidate<'a>>,
}

#[derive(Debug, Deserialize)]
struct GoogleCandidate<'a> {
    #[serde(default, borrow)]
    content: GoogleContent<'a>,
    #[serde(rename = "finishReason")]
    #[serde(borrow)]
    finish_reason: Option<Cow<'a, str>>,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleContent<'a> {
    #[serde(default, borrow)]
    parts: Vec<GooglePart<'a>>,
}

#[derive(Debug, Deserialize)]
struct GooglePart<'a> {
    #[serde(borrow)]
    text: Option<Cow<'a, str>>,
    #[serde(rename = "functionCall")]
    function_call: Option<IgnoredAny>,
    #[serde(rename = "executableCode")]
    executable_code: Option<IgnoredAny>,
    #[serde(rename = "codeExecutionResult")]
    code_execution_result: Option<IgnoredAny>,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::GoogleAdapter;
    use crate::adapters::RaceProtocolAdapter;
    use crate::domain::{DownstreamRouteKind, RaceCandidate};
    use crate::group::{CandidateState, CandidateWeightSnapshot};

    #[test]
    fn metadata_or_finish_only_chunk_is_not_treated_as_content() {
        let adapter = GoogleAdapter::new(DownstreamRouteKind::GoogleV1BetaModels);
        let event = "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":12}}\n\n";
        let mut state = empty_state();
        assert!(
            !adapter
                .inspect_event(&mut state, event.as_bytes())
                .has_content
        );
    }

    #[test]
    fn text_part_is_treated_as_content() {
        let adapter = GoogleAdapter::new(DownstreamRouteKind::GoogleV1BetaModels);
        let event = "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"hello\"}]}}]}\n\n";
        let mut state = empty_state();
        assert!(
            adapter
                .inspect_event(&mut state, event.as_bytes())
                .has_content
        );
    }

    fn empty_state() -> CandidateState {
        CandidateState {
            candidate: RaceCandidate {
                id: "cand-a".to_string(),
                group_id: "group-a".to_string(),
                name: "A".to_string(),
                model_id: None,
                upstream_model: "vendor/model-a".to_string(),
                inline_endpoint_overrides: vec![],
                initial_weight: 100.0,
                response_protection_timeout_ms: 1_000,
                enabled: true,
                metadata: json!({}),
            },
            launched_at: None,
            first_content_at: None,
            winner_selected: false,
            failed: false,
            ended: false,
            error: None,
            buffered_count: 0,
            buffered_bytes: 0,
            api_key_masked: "secret***".to_string(),
            relative_delay: Duration::ZERO,
            weight_snapshot: CandidateWeightSnapshot {
                initial_weight: 100.0,
                effective_weight: 100.0,
                weight_deviation: 0.0,
                status: "normal".to_string(),
            },
            protocol_flags: crate::group::ProtocolFlags::default(),
            protocol_open_blocks: Vec::new(),
        }
    }
}
