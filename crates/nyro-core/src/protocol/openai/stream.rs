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
        let reasoning_content = message.and_then(extract_reasoning_from_message);

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
            reasoning_content,
            tool_calls,
            response_items: None,
            stop_reason,
            usage,
        })
    }
}

// ── Non-streaming response formatter ──

pub struct OpenAIResponseFormatter;

impl ResponseFormatter for OpenAIResponseFormatter {
    fn format_response(&self, resp: &InternalResponse) -> Value {
        let finish_reason = if !resp.tool_calls.is_empty() {
            Some("tool_calls")
        } else {
            resp.stop_reason.as_deref()
        };
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
                "finish_reason": finish_reason,
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
    think_buffer: String,
    in_think_block: bool,
}

impl OpenAIStreamParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            started: false,
            think_buffer: String::new(),
            in_think_block: false,
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
        let mut deltas = Vec::new();
        if !self.buffer.trim().is_empty() {
            let remaining = std::mem::take(&mut self.buffer);
            deltas.extend(self.parse_chunk(&format!("{remaining}\n\n"))?);
        }
        deltas.extend(self.flush_pending_text());
        Ok(deltas)
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
            if let Some(reasoning) = extract_reasoning_from_message(delta) {
                if !reasoning.is_empty() {
                    deltas.push(StreamDelta::ReasoningDelta(reasoning));
                }
            }
            if let Some(text) = delta.get("content").and_then(|v| v.as_str()) {
                self.parse_text_with_think_tags(text, deltas);
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

    fn parse_text_with_think_tags(&mut self, text: &str, deltas: &mut Vec<StreamDelta>) {
        if text.is_empty() {
            return;
        }
        self.think_buffer.push_str(text);

        loop {
            if self.in_think_block {
                if let Some(end_idx) = self.think_buffer.find("</think>") {
                    let thought = self.think_buffer[..end_idx].trim().to_string();
                    if !thought.is_empty() {
                        deltas.push(StreamDelta::ReasoningDelta(thought));
                    }
                    self.think_buffer = self.think_buffer[end_idx + "</think>".len()..].to_string();
                    self.in_think_block = false;
                    continue;
                }
                break;
            }

            if let Some(start_idx) = self.think_buffer.find("<think>") {
                let before = self.think_buffer[..start_idx].to_string();
                if !before.is_empty() {
                    deltas.push(StreamDelta::TextDelta(before));
                }
                self.think_buffer = self.think_buffer[start_idx + "<think>".len()..].to_string();
                self.in_think_block = true;
                continue;
            }

            let keep = longest_suffix_that_can_start_tag(&self.think_buffer, "<think>");
            if self.think_buffer.len() > keep {
                let emit = self.think_buffer[..self.think_buffer.len() - keep].to_string();
                if !emit.is_empty() {
                    deltas.push(StreamDelta::TextDelta(emit));
                }
                self.think_buffer = self.think_buffer[self.think_buffer.len() - keep..].to_string();
            }
            break;
        }
    }

    fn flush_pending_text(&mut self) -> Vec<StreamDelta> {
        if self.think_buffer.is_empty() {
            return vec![];
        }
        if self.in_think_block {
            let mut fallback = String::from("<think>");
            fallback.push_str(&self.think_buffer);
            self.think_buffer.clear();
            self.in_think_block = false;
            vec![StreamDelta::TextDelta(fallback)]
        } else {
            let remaining = std::mem::take(&mut self.think_buffer);
            vec![StreamDelta::TextDelta(remaining)]
        }
    }
}

fn longest_suffix_that_can_start_tag(text: &str, tag: &str) -> usize {
    let max = std::cmp::min(text.len(), tag.len().saturating_sub(1));
    for len in (1..=max).rev() {
        if text.ends_with(&tag[..len]) {
            return len;
        }
    }
    0
}

// ── Stream formatter (deltas → OpenAI SSE) ──

pub struct OpenAIStreamFormatter {
    usage: TokenUsage,
    id: String,
    model: String,
    saw_tool_call: bool,
}

impl OpenAIStreamFormatter {
    pub fn new() -> Self {
        Self {
            usage: TokenUsage::default(),
            id: format!("chatcmpl-{}", Uuid::new_v4()),
            model: String::new(),
            saw_tool_call: false,
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
                StreamDelta::ReasoningDelta(text) => {
                    let chunk = serde_json::json!({
                        "id": self.id,
                        "object": "chat.completion.chunk",
                        "model": self.model,
                        "choices": [{"index": 0, "delta": {"reasoning_content": text}, "finish_reason": null}]
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
                    self.saw_tool_call = true;
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
                    self.saw_tool_call = true;
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
                    let final_reason = if self.saw_tool_call {
                        "tool_calls".to_string()
                    } else {
                        stop_reason.clone()
                    };
                    let chunk = serde_json::json!({
                        "id": self.id,
                        "object": "chat.completion.chunk",
                        "model": self.model,
                        "choices": [{"index": 0, "delta": {}, "finish_reason": final_reason}],
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
    let usage = v.get("usage").or_else(|| v.get("usageMetadata"));
    let Some(u) = usage else {
        return TokenUsage::default();
    };

    let input = first_u64(
        u,
        &[
            "prompt_tokens",
            "promptTokenCount",
            "input_tokens",
            "inputTokenCount",
        ],
    )
    .unwrap_or(0);
    let output = first_u64(
        u,
        &[
            "completion_tokens",
            "candidatesTokenCount",
            "output_tokens",
            "outputTokenCount",
        ],
    )
    .unwrap_or(0);

    TokenUsage {
        input_tokens: input as u32,
        output_tokens: output as u32,
    }
}

fn first_u64(obj: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|k| obj.get(*k).and_then(|v| v.as_u64()))
}

fn extract_reasoning_from_message(message: &Value) -> Option<String> {
    if let Some(reasoning) = message.get("reasoning_content").and_then(|v| v.as_str()) {
        return Some(reasoning.to_string());
    }

    let details = message
        .get("reasoning_details")
        .and_then(|v| v.as_array())?;
    let mut parts: Vec<String> = Vec::new();
    for detail in details {
        if let Some(text) = detail
            .get("text")
            .or_else(|| detail.get("content"))
            .and_then(|v| v.as_str())
        {
            if !text.is_empty() {
                parts.push(text.to_string());
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}
