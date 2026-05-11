use std::collections::BTreeMap;

use bytes::Bytes;

use crate::domain::{DownstreamRouteKind, ProtocolFamily};
use crate::group::CandidateState;

use super::{EventAnalysis, RaceProtocolAdapter};

#[derive(Debug, Clone, Copy)]
pub struct AnthropicAdapter;

impl RaceProtocolAdapter for AnthropicAdapter {
    fn protocol_family(&self) -> ProtocolFamily {
        ProtocolFamily::Anthropic
    }

    fn route_kind(&self) -> DownstreamRouteKind {
        DownstreamRouteKind::AnthropicMessages
    }

    fn protocol_label(&self) -> &'static str {
        "anthropic"
    }

    fn inspect_event(&self, state: &mut CandidateState, event: &[u8]) -> EventAnalysis {
        let Some(event) = std::str::from_utf8(event).ok() else {
            return EventAnalysis::default();
        };
        track_content_blocks(state, event);
        if event.contains("event: message_delta") {
            state.protocol_flags.emitted_message_delta = true;
        }
        if event.contains("event: message_stop") {
            state.protocol_flags.emitted_message_stop = true;
        }

        EventAnalysis {
            has_content: event.contains("event: content_block_delta")
                && (event.contains("\"text_delta\"")
                    || event.contains("\"thinking_delta\"")
                    || event.contains("\"input_json_delta\"")),
        }
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

        vec![
            sse_event(
                "message_start",
                r#"{"type":"message_start","message":{"id":"msg_race_failed","type":"message","role":"assistant","content":[],"model":"race-gateway","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":0,"output_tokens":0}}}"#,
            ),
            sse_event(
                "content_block_start",
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            ),
            sse_event(
                "content_block_delta",
                &format!(
                    r#"{{"type":"content_block_delta","index":0,"delta":{{"type":"text_delta","text":{}}}}}"#,
                    serde_json::to_string(&message)
                        .unwrap_or_else(|_| "\"all failed\"".to_string())
                ),
            ),
            sse_event(
                "content_block_stop",
                r#"{"type":"content_block_stop","index":0}"#,
            ),
            sse_event(
                "message_delta",
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":1}}"#,
            ),
            sse_event("message_stop", r#"{"type":"message_stop"}"#),
        ]
    }

    fn fallback_close_events(&self, _group_name: &str, state: &CandidateState) -> Vec<Bytes> {
        let mut events = Vec::new();
        for index in state.protocol_open_blocks.iter().rev() {
            events.push(sse_event(
                "content_block_stop",
                &format!(r#"{{"type":"content_block_stop","index":{index}}}"#),
            ));
        }
        if !state.protocol_flags.emitted_message_delta {
            events.push(sse_event(
                "message_delta",
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":1}}"#,
            ));
        }
        if !state.protocol_flags.emitted_message_stop {
            events.push(sse_event("message_stop", r#"{"type":"message_stop"}"#));
        }
        events
    }
}

fn sse_event(event: &str, data: &str) -> Bytes {
    Bytes::from(format!("event: {event}\ndata: {data}\n\n"))
}

fn track_content_blocks(state: &mut CandidateState, event: &str) {
    let event_name = event
        .lines()
        .find_map(|line| line.strip_prefix("event:").map(str::trim));
    let payload = event
        .lines()
        .find_map(|line| line.strip_prefix("data:").map(str::trim));
    let (Some(event_name), Some(payload)) = (event_name, payload) else {
        return;
    };
    let index = extract_json_usize_field(payload, "index");

    match event_name {
        "content_block_start" => {
            if let Some(index) = index {
                state.protocol_open_blocks.push(index);
            }
        }
        "content_block_stop" => {
            if let Some(index) = index {
                if state.protocol_open_blocks.last().copied() == Some(index) {
                    state.protocol_open_blocks.pop();
                } else if let Some(position) = state
                    .protocol_open_blocks
                    .iter()
                    .position(|open_index| *open_index == index)
                {
                    state.protocol_open_blocks.remove(position);
                }
            }
        }
        _ => {}
    }
}

fn extract_json_usize_field(payload: &str, field: &str) -> Option<usize> {
    let needle = ["\"", field, "\""].concat();
    let start = payload.find(&needle)?;
    let after = &payload[start + needle.len()..];
    let colon = after.find(':')?;
    let digits = after[colon + 1..].trim_start().as_bytes();
    let len = digits.iter().take_while(|ch| ch.is_ascii_digit()).count();
    if len == 0 {
        return None;
    }
    std::str::from_utf8(&digits[..len]).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;
    use crate::domain::RaceCandidate;
    use crate::group::CandidateWeightSnapshot;

    fn candidate_state() -> CandidateState {
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
            winner_selected: true,
            failed: true,
            ended: true,
            error: Some("connection reset".to_string()),
            buffered_count: 1,
            buffered_bytes: 32,
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

    #[test]
    fn fallback_close_closes_unclosed_blocks() {
        let adapter = AnthropicAdapter;
        let mut state = candidate_state();
        let analysis = adapter.inspect_event(
            &mut state,
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n"
                .as_bytes(),
        );
        assert!(!analysis.has_content);

        let events = adapter.fallback_close_events("group-a", &state);
        assert!(events.iter().any(|event| {
            std::str::from_utf8(event).is_ok_and(|text| text.contains("event: content_block_stop"))
        }));
        assert!(events.iter().any(|event| {
            std::str::from_utf8(event).is_ok_and(|text| text.contains("event: message_stop"))
        }));
    }
}
