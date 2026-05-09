//! Stream response accumulator: buffers streaming deltas into a complete
//! `InternalResponse` for caching and formatted response aggregation.

use crate::protocol::types::{InternalResponse, StreamDelta, TokenUsage, ToolCall};

#[derive(Default)]
pub(super) struct StreamResponseAccumulator {
    pub(super) id: String,
    pub(super) model: String,
    pub(super) content: String,
    pub(super) reasoning_content: String,
    pub(super) reasoning_signature: String,
    pub(super) tool_calls: Vec<Option<ToolCall>>,
    pub(super) stop_reason: Option<String>,
    pub(super) usage: TokenUsage,
}

impl StreamResponseAccumulator {
    pub(super) fn apply_all(&mut self, deltas: &[StreamDelta]) {
        for delta in deltas { self.apply(delta); }
    }

    pub(super) fn apply(&mut self, delta: &StreamDelta) {
        match delta {
            StreamDelta::MessageStart { id, model } => {
                if self.id.is_empty() { self.id = id.clone(); }
                if self.model.is_empty() { self.model = model.clone(); }
            }
            StreamDelta::ReasoningDelta(text) => self.reasoning_content.push_str(text),
            StreamDelta::ReasoningSignature(sig) => self.reasoning_signature.push_str(sig),
            StreamDelta::TextDelta(text) => self.content.push_str(text),
            StreamDelta::ToolCallStart { index, id, name } => {
                ensure_tool_index(&mut self.tool_calls, *index);
                self.tool_calls[*index] = Some(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: String::new(),
                });
            }
            StreamDelta::ToolCallDelta { index, arguments } => {
                ensure_tool_index(&mut self.tool_calls, *index);
                if let Some(tc) = self.tool_calls[*index].as_mut() {
                    tc.arguments.push_str(arguments);
                } else {
                    self.tool_calls[*index] = Some(ToolCall {
                        id: format!("tool-{index}"),
                        name: String::new(),
                        arguments: arguments.clone(),
                    });
                }
            }
            StreamDelta::Usage(usage) => self.usage = usage.clone(),
            StreamDelta::Done { stop_reason } => self.stop_reason = Some(stop_reason.clone()),
            StreamDelta::RawEvent { .. } => {}
        }
    }

    pub(super) fn into_internal_response(self) -> InternalResponse {
        let tool_calls = self.tool_calls.into_iter().flatten()
            .filter(|tc| !tc.name.is_empty())
            .collect::<Vec<_>>();
        InternalResponse {
            id: self.id,
            model: self.model,
            content: self.content,
            reasoning_content: if self.reasoning_content.is_empty() {
                None
            } else {
                Some(self.reasoning_content)
            },
            reasoning_signature: if self.reasoning_signature.is_empty() {
                None
            } else {
                Some(self.reasoning_signature)
            },
            tool_calls,
            stop_reason: self.stop_reason,
            usage: self.usage,
            response_items: None,
        }
    }
}

pub(super) fn ensure_tool_index(tool_calls: &mut Vec<Option<ToolCall>>, index: usize) {
    if tool_calls.len() <= index {
        tool_calls.resize(index + 1, None);
    }
}
