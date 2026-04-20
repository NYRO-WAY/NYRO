use anyhow::Result;
use serde_json::Value;

use crate::protocol::types::*;
use crate::protocol::{ResponseParser, StreamParser};

pub struct ResponsesResponseParser;

impl ResponseParser for ResponsesResponseParser {
    fn parse_response(&self, resp: Value) -> Result<InternalResponse> {
        let id = resp
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let model = resp
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let stop_reason = resp
            .get("status")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let mut content = String::new();
        let mut tool_calls = Vec::new();

        if let Some(items) = resp.get("output").and_then(|v| v.as_array()) {
            for item in items {
                match item.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                    "message" => {
                        if let Some(blocks) = item.get("content").and_then(|v| v.as_array()) {
                            for block in blocks {
                                if matches!(
                                    block.get("type").and_then(|v| v.as_str()),
                                    Some("output_text" | "text")
                                ) {
                                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                                        content.push_str(text);
                                    }
                                }
                            }
                        }
                    }
                    "function_call" => {
                        let call_id = item
                            .get("call_id")
                            .or_else(|| item.get("id"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = item
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let arguments = item
                            .get("arguments")
                            .and_then(|v| v.as_str())
                            .unwrap_or("{}")
                            .to_string();
                        if !call_id.is_empty() && !name.is_empty() {
                            tool_calls.push(ToolCall {
                                id: call_id,
                                name,
                                arguments,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        let usage = TokenUsage {
            input_tokens: resp
                .get("usage")
                .and_then(|v| v.get("input_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            output_tokens: resp
                .get("usage")
                .and_then(|v| v.get("output_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
        };

        Ok(InternalResponse {
            id,
            model,
            content,
            reasoning_content: None,
            tool_calls,
            response_items: None,
            stop_reason,
            usage,
        })
    }
}

pub struct ResponsesStreamParser {
    buffer: String,
    started: bool,
}

impl ResponsesStreamParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            started: false,
        }
    }
}

impl StreamParser for ResponsesStreamParser {
    fn parse_chunk(&mut self, raw: &str) -> Result<Vec<StreamDelta>> {
        self.buffer.push_str(raw);
        let mut deltas = Vec::new();

        while let Some(pos) = self.buffer.find("\n\n") {
            let block = self.buffer[..pos].to_string();
            self.buffer = self.buffer[pos + 2..].to_string();

            let mut event_name: Option<String> = None;
            for line in block.lines() {
                if let Some(event) = line.strip_prefix("event: ") {
                    event_name = Some(event.trim().to_string());
                    continue;
                }
                let Some(data) = line.strip_prefix("data: ") else {
                    continue;
                };
                let data = data.trim();
                if data == "[DONE]" {
                    deltas.push(StreamDelta::Done {
                        stop_reason: "stop".to_string(),
                    });
                    continue;
                }
                let Ok(payload) = serde_json::from_str::<Value>(data) else {
                    continue;
                };
                self.parse_event(event_name.as_deref(), &payload, &mut deltas);
            }
        }

        Ok(deltas)
    }

    fn finish(&mut self) -> Result<Vec<StreamDelta>> {
        if self.buffer.trim().is_empty() {
            return Ok(Vec::new());
        }
        let remaining = std::mem::take(&mut self.buffer);
        self.parse_chunk(&format!("{remaining}\n\n"))
    }
}

impl ResponsesStreamParser {
    fn parse_event(&mut self, event: Option<&str>, payload: &Value, deltas: &mut Vec<StreamDelta>) {
        match event.unwrap_or("") {
            "response.created" | "response.in_progress" => {
                if self.started {
                    return;
                }
                let response = payload.get("response").unwrap_or(payload);
                let id = response
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let model = response
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if !id.is_empty() || !model.is_empty() {
                    self.started = true;
                    deltas.push(StreamDelta::MessageStart { id, model });
                }
            }
            "response.output_text.delta" => {
                if let Some(text) = payload.get("delta").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        deltas.push(StreamDelta::TextDelta(text.to_string()));
                    }
                }
            }
            "response.function_call_arguments.delta" => {
                let index = payload
                    .get("output_index")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;
                if let Some(arguments) = payload.get("delta").and_then(|v| v.as_str()) {
                    if !arguments.is_empty() {
                        deltas.push(StreamDelta::ToolCallDelta {
                            index,
                            arguments: arguments.to_string(),
                        });
                    }
                }
            }
            "response.output_item.added" | "response.output_item.done" => {
                let index = payload
                    .get("output_index")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;
                let item = payload.get("item").unwrap_or(payload);
                if item.get("type").and_then(|v| v.as_str()) == Some("function_call") {
                    let id = item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = item
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if !id.is_empty() && !name.is_empty() {
                        deltas.push(StreamDelta::ToolCallStart { index, id, name });
                    }
                }
            }
            "response.completed" => {
                let response = payload.get("response").unwrap_or(payload);
                let usage = TokenUsage {
                    input_tokens: response
                        .get("usage")
                        .and_then(|v| v.get("input_tokens"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    output_tokens: response
                        .get("usage")
                        .and_then(|v| v.get("output_tokens"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                };
                if usage.input_tokens > 0 || usage.output_tokens > 0 {
                    deltas.push(StreamDelta::Usage(usage));
                }
                deltas.push(StreamDelta::Done {
                    stop_reason: response
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("completed")
                        .to_string(),
                });
            }
            _ => {}
        }
    }
}
