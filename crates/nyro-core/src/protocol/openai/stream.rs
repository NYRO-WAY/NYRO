use anyhow::Result;
use serde_json::Value;
use uuid::Uuid;

use crate::protocol::types::*;
use crate::protocol::*;

// ── Non-streaming response parser ──

pub struct OpenAIResponseParser;

impl ResponseParser for OpenAIResponseParser {
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

        let choice = resp
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first());

        let message = choice.and_then(|c| c.get("message"));

        let content = message
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        let stop_reason = choice
            .and_then(|c| c.get("finish_reason"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let tool_calls = message
            .and_then(|m| m.get("tool_calls"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|tc| {
                        let func = tc.get("function")?;
                        Some(ToolCall {
                            id: tc.get("id")?.as_str()?.to_string(),
                            name: func.get("name")?.as_str()?.to_string(),
                            arguments: func
                                .get("arguments")
                                .and_then(|a| a.as_str())
                                .unwrap_or("")
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let usage = extract_usage(&resp);

        Ok(InternalResponse {
            id,
            model,
            content,
            tool_calls,
            stop_reason,
            usage,
        })
    }
}

// ── Non-streaming response formatter ──

pub struct OpenAIResponseFormatter;

impl ResponseFormatter for OpenAIResponseFormatter {
    fn format_response(&self, resp: &InternalResponse) -> Value {
        let mut message = serde_json::json!({
            "role": "assistant",
            "content": resp.content,
        });

        if !resp.tool_calls.is_empty() {
            let tcs: Vec<Value> = resp
                .tool_calls
                .iter()
                .map(|tc| {
                    serde_json::json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": tc.arguments,
                        }
                    })
                })
                .collect();
            message
                .as_object_mut()
                .unwrap()
                .insert("tool_calls".into(), Value::Array(tcs));
        }

        serde_json::json!({
            "id": resp.id,
            "object": "chat.completion",
            "model": resp.model,
            "choices": [{
                "index": 0,
                "message": message,
                "finish_reason": resp.stop_reason,
            }],
            "usage": {
                "prompt_tokens": resp.usage.input_tokens,
                "completion_tokens": resp.usage.output_tokens,
                "total_tokens": resp.usage.input_tokens + resp.usage.output_tokens,
            }
        })
    }
}

// ── Stream parser (upstream OpenAI SSE → deltas) ──

pub struct OpenAIStreamParser {
    buffer: String,
    started: bool,
}

impl OpenAIStreamParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            started: false,
        }
    }
}

impl StreamParser for OpenAIStreamParser {
    fn parse_chunk(&mut self, raw: &str) -> Result<Vec<StreamDelta>> {
        self.buffer.push_str(raw);
        let mut deltas = Vec::new();

        while let Some(pos) = self.buffer.find("\n\n") {
            let block = self.buffer[..pos].to_string();
            self.buffer = self.buffer[pos + 2..].to_string();

            for line in block.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    let data = data.trim();
                    if data == "[DONE]" {
                        deltas.push(StreamDelta::Done {
                            stop_reason: "stop".to_string(),
                        });
                        continue;
                    }
                    if let Ok(chunk) = serde_json::from_str::<Value>(data) {
                        self.parse_openai_chunk(&chunk, &mut deltas);
                    }
                }
            }
        }

        Ok(deltas)
    }

    fn finish(&mut self) -> Result<Vec<StreamDelta>> {
        if self.buffer.trim().is_empty() {
            return Ok(vec![]);
        }
        let remaining = std::mem::take(&mut self.buffer);
        self.parse_chunk(&format!("{remaining}\n\n"))
    }
}

impl OpenAIStreamParser {
    fn parse_openai_chunk(&mut self, chunk: &Value, deltas: &mut Vec<StreamDelta>) {
        if !self.started {
            if let (Some(id), Some(model)) = (
                chunk.get("id").and_then(|v| v.as_str()),
                chunk.get("model").and_then(|v| v.as_str()),
            ) {
                self.started = true;
                deltas.push(StreamDelta::MessageStart {
                    id: id.to_string(),
                    model: model.to_string(),
                });
            }
        }

        let Some(choice) = chunk
            .get("choices")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
        else {
            let u = extract_usage(chunk);
            if u.input_tokens > 0 || u.output_tokens > 0 {
                deltas.push(StreamDelta::Usage(u));
            }
            return;
        };

        if let Some(delta) = choice.get("delta") {
            if let Some(text) = delta.get("content").and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    deltas.push(StreamDelta::TextDelta(text.to_string()));
                }
            }

            if let Some(tcs) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                for tc in tcs {
                    let idx = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    if let Some(func) = tc.get("function") {
                        if let Some(name) = func.get("name").and_then(|v| v.as_str()) {
                            let id = tc
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            deltas.push(StreamDelta::ToolCallStart {
                                index: idx,
                                id,
                                name: name.to_string(),
                            });
                        }
                        if let Some(args) = func.get("arguments").and_then(|v| v.as_str()) {
                            if !args.is_empty() {
                                deltas.push(StreamDelta::ToolCallDelta {
                                    index: idx,
                                    arguments: args.to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }

        if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
            deltas.push(StreamDelta::Done {
                stop_reason: reason.to_string(),
            });
        }

        let u = extract_usage(chunk);
        if u.input_tokens > 0 || u.output_tokens > 0 {
            deltas.push(StreamDelta::Usage(u));
        }
    }
}

// ── Stream formatter (deltas → OpenAI SSE) ──

pub struct OpenAIStreamFormatter {
    usage: TokenUsage,
    id: String,
    model: String,
}

impl OpenAIStreamFormatter {
    pub fn new() -> Self {
        Self {
            usage: TokenUsage::default(),
            id: format!("chatcmpl-{}", Uuid::new_v4()),
            model: String::new(),
        }
    }
}

impl StreamFormatter for OpenAIStreamFormatter {
    fn format_deltas(&mut self, deltas: &[StreamDelta]) -> Vec<SseEvent> {
        let mut events = Vec::new();
        for delta in deltas {
            match delta {
                StreamDelta::MessageStart { id, model } => {
                    self.id = id.clone();
                    self.model = model.clone();
                    let chunk = serde_json::json!({
                        "id": self.id,
                        "object": "chat.completion.chunk",
                        "model": self.model,
                        "choices": [{"index": 0, "delta": {"role": "assistant"}, "finish_reason": null}]
                    });
                    events.push(SseEvent::new(None, chunk.to_string()));
                }
                StreamDelta::TextDelta(text) => {
                    let chunk = serde_json::json!({
                        "id": self.id,
                        "object": "chat.completion.chunk",
                        "model": self.model,
                        "choices": [{"index": 0, "delta": {"content": text}, "finish_reason": null}]
                    });
                    events.push(SseEvent::new(None, chunk.to_string()));
                }
                StreamDelta::ToolCallStart { index, id, name } => {
                    let chunk = serde_json::json!({
                        "id": self.id,
                        "object": "chat.completion.chunk",
                        "model": self.model,
                        "choices": [{"index": 0, "delta": {
                            "tool_calls": [{"index": index, "id": id, "type": "function", "function": {"name": name, "arguments": ""}}]
                        }, "finish_reason": null}]
                    });
                    events.push(SseEvent::new(None, chunk.to_string()));
                }
                StreamDelta::ToolCallDelta { index, arguments } => {
                    let chunk = serde_json::json!({
                        "id": self.id,
                        "object": "chat.completion.chunk",
                        "model": self.model,
                        "choices": [{"index": 0, "delta": {
                            "tool_calls": [{"index": index, "function": {"arguments": arguments}}]
                        }, "finish_reason": null}]
                    });
                    events.push(SseEvent::new(None, chunk.to_string()));
                }
                StreamDelta::Usage(u) => {
                    self.usage = u.clone();
                }
                StreamDelta::Done { stop_reason } => {
                    let chunk = serde_json::json!({
                        "id": self.id,
                        "object": "chat.completion.chunk",
                        "model": self.model,
                        "choices": [{"index": 0, "delta": {}, "finish_reason": stop_reason}],
                        "usage": {
                            "prompt_tokens": self.usage.input_tokens,
                            "completion_tokens": self.usage.output_tokens,
                            "total_tokens": self.usage.input_tokens + self.usage.output_tokens,
                        }
                    });
                    events.push(SseEvent::new(None, chunk.to_string()));
                    events.push(SseEvent::new(None, "[DONE]"));
                }
            }
        }
        events
    }

    fn format_done(&mut self) -> Vec<SseEvent> {
        vec![]
    }

    fn usage(&self) -> TokenUsage {
        self.usage.clone()
    }
}

fn extract_usage(v: &Value) -> TokenUsage {
    if let Some(u) = v.get("usage") {
        TokenUsage {
            input_tokens: u
                .get("prompt_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            output_tokens: u
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
        }
    } else {
        TokenUsage::default()
    }
}
