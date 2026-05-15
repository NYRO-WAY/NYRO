//! Stream response accumulator: buffers streaming deltas into a complete
//! `AiResponse` for caching and formatted response aggregation.

use crate::protocol::ir::request::ToolCall;
use crate::protocol::ir::{AiResponse, AiStreamDelta};
use crate::protocol::types::TokenUsage;

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
    pub(super) fn apply_all(&mut self, deltas: &[AiStreamDelta]) {
        for delta in deltas {
            self.apply(delta);
        }
    }

    pub(super) fn apply(&mut self, delta: &AiStreamDelta) {
        match delta {
            AiStreamDelta::MessageStart { id, model } => {
                if self.id.is_empty() {
                    self.id = id.clone();
                }
                if self.model.is_empty() {
                    self.model = model.clone();
                }
            }
            AiStreamDelta::ThinkingDelta(text) => self.reasoning_content.push_str(text),
            AiStreamDelta::ThinkingSignature(sig) => self.reasoning_signature.push_str(sig),
            AiStreamDelta::TextDelta(text) => self.content.push_str(text),
            AiStreamDelta::ToolCallStart { index, id, name } => {
                ensure_tool_index(&mut self.tool_calls, *index);
                self.tool_calls[*index] = Some(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: String::new(),
                });
            }
            AiStreamDelta::ToolCallDelta { index, arguments } => {
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
            AiStreamDelta::ToolCallComplete { index, tool_call } => {
                ensure_tool_index(&mut self.tool_calls, *index);
                self.tool_calls[*index] = Some(tool_call.clone());
            }
            AiStreamDelta::Usage(usage) => self.usage = usage.clone(),
            AiStreamDelta::Done { stop_reason } => self.stop_reason = Some(stop_reason.clone()),
            AiStreamDelta::StreamError { error } => {
                self.stop_reason = Some("error".to_string());
                tracing::warn!(error = ?error, "stream error delta received");
            }
            AiStreamDelta::UnexpectedEof => {
                if self.stop_reason.is_none() {
                    self.stop_reason = Some("error".to_string());
                }
            }
            AiStreamDelta::Unknown { .. } => {}
        }
    }

    pub(super) fn into_ai_response(self) -> AiResponse {
        let tool_calls = self
            .tool_calls
            .into_iter()
            .flatten()
            .filter(|tc| !tc.name.is_empty())
            .collect::<Vec<_>>();
        let mut resp = AiResponse::new(self.id, self.model);
        resp.content = self.content;
        resp.reasoning_content = if self.reasoning_content.is_empty() {
            None
        } else {
            Some(self.reasoning_content)
        };
        resp.reasoning_signature = if self.reasoning_signature.is_empty() {
            None
        } else {
            Some(self.reasoning_signature)
        };
        resp.tool_calls = tool_calls;
        resp.stop_reason = self.stop_reason;
        resp.usage = self.usage;
        resp
    }
}

pub(super) fn ensure_tool_index(tool_calls: &mut Vec<Option<ToolCall>>, index: usize) {
    if tool_calls.len() <= index {
        tool_calls.resize(index + 1, None);
    }
}
