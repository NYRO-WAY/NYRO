use std::{borrow::Cow, collections::BTreeMap};

use bytes::Bytes;
use serde::Deserialize;

use crate::domain::{DownstreamRouteKind, ProtocolFamily};
use crate::group::CandidateState;

use super::{EventAnalysis, RaceProtocolAdapter};

#[derive(Debug, Clone, Copy)]
pub struct OpenAiAdapter {
    route_kind: DownstreamRouteKind,
}

impl OpenAiAdapter {
    pub fn new(route_kind: DownstreamRouteKind) -> Self {
        Self { route_kind }
    }
}

impl RaceProtocolAdapter for OpenAiAdapter {
    fn protocol_family(&self) -> ProtocolFamily {
        ProtocolFamily::OpenAi
    }

    fn route_kind(&self) -> DownstreamRouteKind {
        self.route_kind
    }

    fn protocol_label(&self) -> &'static str {
        match self.route_kind {
            DownstreamRouteKind::OpenAiResponses => "openai_responses",
            _ => "openai",
        }
    }

    fn inspect_event(&self, state: &mut CandidateState, event: &[u8]) -> EventAnalysis {
        let Some(event) = std::str::from_utf8(event).ok() else {
            return EventAnalysis::default();
        };
        let mut analysis = EventAnalysis::default();
        for payload in iter_sse_data(event) {
            if payload == "[DONE]" {
                state.protocol_flags.emitted_done = true;
                continue;
            }
            match self.route_kind {
                DownstreamRouteKind::OpenAiResponses => {
                    let Ok(value) = serde_json::from_str::<OpenAiResponseEvent<'_>>(payload) else {
                        continue;
                    };
                    if matches!(
                        value.kind.as_deref(),
                        Some(
                            "response.output_text.delta"
                                | "response.reasoning.delta"
                                | "response.function_call_arguments.delta"
                                | "response.output_item.added"
                        )
                    ) {
                        analysis.has_content = true;
                    }
                    if matches!(
                        value.kind.as_deref(),
                        Some(
                            "response.completed"
                                | "response.failed"
                                | "response.output_item.done"
                                | "response.content_part.done"
                                | "response.output_text.done"
                        )
                    ) {
                        state.protocol_flags.emitted_finish = true;
                    }
                }
                _ => {
                    let Ok(value) = serde_json::from_str::<OpenAiChatChunk<'_>>(payload) else {
                        continue;
                    };
                    if value
                        .choices
                        .iter()
                        .any(|choice| choice.delta.as_ref().is_some_and(has_openai_delta_content))
                    {
                        analysis.has_content = true;
                    }
                    if value
                        .choices
                        .iter()
                        .any(|choice| choice.finish_reason.is_some())
                    {
                        state.protocol_flags.emitted_finish = true;
                    }
                }
            }
        }
        analysis
    }

    fn all_failed_events(&self, group_name: &str, errors: &BTreeMap<String, String>) -> Vec<Bytes> {
        let message = format!(
            "All candidates failed in model group '{group_name}': {}",
            errors
                .iter()
                .map(|(name, reason)| format!("{name}: {reason}"))
                .collect::<Vec<_>>()
                .join("; ")
        );

        match self.route_kind {
            DownstreamRouteKind::OpenAiResponses => vec![
                sse_data(
                    &serde_json::json!({
                        "type": "error",
                        "error": {
                            "type": "model_group_race_error",
                            "message": message,
                            "code": "all_candidates_failed"
                        }
                    })
                    .to_string(),
                ),
                sse_data("[DONE]"),
            ],
            _ => vec![
                sse_data(
                    &serde_json::json!({
                        "error": {
                            "message": message,
                            "type": "model_group_race_error",
                            "param": "model",
                            "code": "all_candidates_failed"
                        }
                    })
                    .to_string(),
                ),
                sse_data("[DONE]"),
            ],
        }
    }

    fn fallback_close_events(&self, _group_name: &str, state: &CandidateState) -> Vec<Bytes> {
        let mut events = Vec::new();
        if !state.protocol_flags.emitted_finish {
            match self.route_kind {
                DownstreamRouteKind::OpenAiResponses => {
                    events.push(sse_data(
                        &serde_json::json!({
                            "type": "response.completed"
                        })
                        .to_string(),
                    ));
                }
                _ => {
                    events.push(sse_data(
                        &serde_json::json!({
                            "choices": [{
                                "index": 0,
                                "delta": {},
                                "finish_reason": "stop"
                            }]
                        })
                        .to_string(),
                    ));
                }
            }
        }
        if !state.protocol_flags.emitted_done {
            events.push(sse_data("[DONE]"));
        }
        events
    }
}

fn iter_sse_data(event: &str) -> impl Iterator<Item = &str> {
    event
        .lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim))
}

fn has_openai_delta_content(delta: &OpenAiDelta) -> bool {
    delta
        .content
        .as_deref()
        .is_some_and(|content| !content.is_empty())
        || delta
            .reasoning_content
            .as_deref()
            .is_some_and(|content| !content.is_empty())
        || delta.tool_calls.as_ref().is_some_and(|calls| {
            calls.iter().any(|call| {
                call.function.as_ref().is_some_and(|function| {
                    function
                        .name
                        .as_deref()
                        .is_some_and(|name| !name.is_empty())
                        || function
                            .arguments
                            .as_deref()
                            .is_some_and(|args| !args.is_empty())
                })
            })
        })
        || delta.function_call.as_ref().is_some_and(|function| {
            function
                .name
                .as_deref()
                .is_some_and(|name| !name.is_empty())
                || function
                    .arguments
                    .as_deref()
                    .is_some_and(|args| !args.is_empty())
        })
}

fn sse_data(data: &str) -> Bytes {
    Bytes::from(format!("data: {data}\n\n"))
}

#[derive(Debug, Deserialize)]
struct OpenAiResponseEvent<'a> {
    #[serde(rename = "type")]
    #[serde(borrow)]
    kind: Option<Cow<'a, str>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatChunk<'a> {
    #[serde(default, borrow)]
    choices: Vec<OpenAiChoice<'a>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice<'a> {
    #[serde(borrow)]
    delta: Option<OpenAiDelta<'a>>,
    #[serde(borrow)]
    finish_reason: Option<Cow<'a, str>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiDelta<'a> {
    #[serde(borrow)]
    content: Option<Cow<'a, str>>,
    #[serde(borrow)]
    reasoning_content: Option<Cow<'a, str>>,
    #[serde(borrow)]
    tool_calls: Option<Vec<OpenAiToolCall<'a>>>,
    #[serde(borrow)]
    function_call: Option<OpenAiFunctionFields<'a>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiToolCall<'a> {
    #[serde(borrow)]
    function: Option<OpenAiFunctionFields<'a>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiFunctionFields<'a> {
    #[serde(borrow)]
    name: Option<Cow<'a, str>>,
    #[serde(borrow)]
    arguments: Option<Cow<'a, str>>,
}

#[cfg(test)]
mod tests {
    use super::OpenAiAdapter;
    use crate::adapters::RaceProtocolAdapter;
    use crate::domain::DownstreamRouteKind;

    #[test]
    fn role_only_chunk_is_not_treated_as_content() {
        let adapter = OpenAiAdapter::new(DownstreamRouteKind::OpenAiChatCompletions);
        let event = "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n";
        let mut state = empty_state();
        assert!(
            !adapter
                .inspect_event(&mut state, event.as_bytes())
                .has_content
        );
    }

    #[test]
    fn reasoning_or_tool_delta_is_treated_as_content() {
        let adapter = OpenAiAdapter::new(DownstreamRouteKind::OpenAiChatCompletions);
        let event = "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"function\":{\"name\":\"lookup\",\"arguments\":\"{\\\"q\\\":\\\"x\\\"}\"}}]},\"finish_reason\":null}]}\n\n";
        let mut state = empty_state();
        assert!(
            adapter
                .inspect_event(&mut state, event.as_bytes())
                .has_content
        );
    }

    fn empty_state() -> crate::group::CandidateState {
        use std::time::Duration;

        use serde_json::json;

        crate::group::CandidateState {
            candidate: crate::domain::RaceCandidate {
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
            weight_snapshot: crate::group::CandidateWeightSnapshot {
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
