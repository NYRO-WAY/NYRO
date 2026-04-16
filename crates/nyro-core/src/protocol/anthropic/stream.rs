use anyhow::Result;
use serde_json::Value;
use uuid::Uuid;

use crate::protocol::types::*;
use crate::protocol::*;

// ── Non-streaming response parser ──

pub struct AnthropicResponseParser;

impl ResponseParser for AnthropicResponseParser {
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

        let mut content_text = String::new();
        let mut tool_calls = Vec::new();

        if let Some(blocks) = resp.get("content").and_then(|c| c.as_array()) {
            for block in blocks {
                match block.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            content_text.push_str(text);
                        }
                    }
                    Some("tool_use") => {
                        if let (Some(tc_id), Some(name)) = (
                            block.get("id").and_then(|v| v.as_str()),
                            block.get("name").and_then(|v| v.as_str()),
                        ) {
                            let input = block
                                .get("input")
                                .cloned()
                                .unwrap_or(Value::Object(Default::default()));
                            tool_calls.push(ToolCall {
                                id: tc_id.to_string(),
                                name: name.to_string(),
                                arguments: input.to_string(),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        let stop_reason = resp
            .get("stop_reason")
            .and_then(|v| v.as_str())
            .map(|r| match r {
                "end_turn" => "stop".to_string(),
                "tool_use" => "tool_calls".to_string(),
                other => other.to_string(),
            });

        let usage = extract_anthropic_usage(&resp);

        Ok(InternalResponse {
            id,
            model,
            content: content_text,
            reasoning_content: None,
            tool_calls,
            response_items: None,
            stop_reason,
            usage,
        })
    }
}

// ── Non-streaming response formatter ──

pub struct AnthropicResponseFormatter;

impl ResponseFormatter for AnthropicResponseFormatter {
    fn format_response(&self, resp: &InternalResponse) -> Value {
        let mut content = Vec::new();

        if let Some(reasoning) = resp
            .reasoning_content
            .as_ref()
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
        {
            content.push(serde_json::json!({
                "type": "thinking",
                "thinking": reasoning,
            }));
        }

        if !resp.content.is_empty() {
            content.push(serde_json::json!({"type": "text", "text": resp.content}));
        }

        for tc in &resp.tool_calls {
            let input: Value =
                serde_json::from_str(&tc.arguments).unwrap_or(Value::Object(Default::default()));
            content.push(serde_json::json!({
                "type": "tool_use",
                "id": tc.id,
                "name": tc.name,
                "input": input,
            }));
        }

        let stop_reason = resp.stop_reason.as_deref().map(|r| match r {
            "stop" => "end_turn",
            "tool_calls" => "tool_use",
            other => other,
        });

        serde_json::json!({
            "id": resp.id,
            "type": "message",
            "role": "assistant",
            "content": content,
            "model": resp.model,
            "stop_reason": stop_reason,
            "usage": {
                "input_tokens": resp.usage.input_tokens,
                "output_tokens": resp.usage.output_tokens,
            }
        })
    }
}

// ── Stream parser (upstream Anthropic SSE → deltas) ──

pub struct AnthropicStreamParser {
    buffer: String,
}

impl AnthropicStreamParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }
}

impl StreamParser for AnthropicStreamParser {
    fn parse_chunk(&mut self, raw: &str) -> Result<Vec<StreamDelta>> {
        self.buffer.push_str(raw);
        let mut deltas = Vec::new();

        while let Some(pos) = self.buffer.find("\n\n") {
            let block = self.buffer[..pos].to_string();
            self.buffer = self.buffer[pos + 2..].to_string();

            let mut event_type = None;
            let mut data_str = None;

            for line in block.lines() {
                if let Some(ev) = line.strip_prefix("event: ") {
                    event_type = Some(ev.trim().to_string());
                } else if let Some(d) = line.strip_prefix("data: ") {
                    data_str = Some(d.trim().to_string());
                }
            }

            if let Some(data) = data_str {
                if let Ok(json) = serde_json::from_str::<Value>(&data) {
                    parse_anthropic_event(event_type.as_deref(), &json, &mut deltas);
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

fn parse_anthropic_event(event_type: Option<&str>, data: &Value, deltas: &mut Vec<StreamDelta>) {
    match event_type {
        Some("message_start") => {
            if let Some(msg) = data.get("message") {
                let id = msg
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let model = msg
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                deltas.push(StreamDelta::MessageStart { id, model });

                let u = extract_anthropic_usage(msg);
                if u.input_tokens > 0 {
                    deltas.push(StreamDelta::Usage(u));
                }
            }
        }
        Some("content_block_start") => {
            let idx = data.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            if let Some(block) = data.get("content_block") {
                match block.get("type").and_then(|t| t.as_str()) {
                    Some("tool_use") => {
                        let id = block
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = block
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        deltas.push(StreamDelta::ToolCallStart {
                            index: idx,
                            id,
                            name,
                        });
                    }
                    _ => {}
                }
            }
        }
        Some("content_block_delta") => {
            if let Some(delta) = data.get("delta") {
                match delta.get("type").and_then(|t| t.as_str()) {
                    Some("text_delta") => {
                        if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                            deltas.push(StreamDelta::TextDelta(text.to_string()));
                        }
                    }
                    Some("input_json_delta") => {
                        if let Some(json) = delta.get("partial_json").and_then(|t| t.as_str()) {
                            let idx =
                                data.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                            deltas.push(StreamDelta::ToolCallDelta {
                                index: idx,
                                arguments: json.to_string(),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        Some("message_delta") => {
            if let Some(delta) = data.get("delta") {
                if let Some(reason) = delta.get("stop_reason").and_then(|v| v.as_str()) {
                    let normalized = match reason {
                        "end_turn" => "stop",
                        "tool_use" => "tool_calls",
                        other => other,
                    };
                    deltas.push(StreamDelta::Done {
                        stop_reason: normalized.to_string(),
                    });
                }
            }
            if let Some(u) = data.get("usage") {
                let output = u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                if output > 0 {
                    deltas.push(StreamDelta::Usage(TokenUsage {
                        input_tokens: 0,
                        output_tokens: output,
                    }));
                }
            }
        }
        Some("ping") | Some("content_block_stop") | Some("message_stop") => {}
        _ => {}
    }
}

// ── Stream formatter (deltas → Anthropic SSE) ──

pub struct AnthropicStreamFormatter {
    usage: TokenUsage,
    id: String,
    model: String,
    block_index: usize,
    in_thinking_block: bool,
    in_text_block: bool,
    in_tool_block: bool,
    message_started: bool,
}

impl AnthropicStreamFormatter {
    pub fn new() -> Self {
        Self {
            usage: TokenUsage::default(),
            id: format!("msg_{}", Uuid::new_v4().simple()),
            model: String::new(),
            block_index: 0,
            in_thinking_block: false,
            in_text_block: false,
            in_tool_block: false,
            message_started: false,
        }
    }

    fn ensure_message_start(&mut self, events: &mut Vec<SseEvent>) {
        if self.message_started {
            return;
        }
        self.message_started = true;
        let msg_start = serde_json::json!({
            "type": "message_start",
            "message": {
                "id": self.id,
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": self.model,
                "stop_reason": null,
                "usage": {"input_tokens": self.usage.input_tokens, "output_tokens": 0}
            }
        });
        events.push(SseEvent::new(Some("message_start"), msg_start.to_string()));
        events.push(SseEvent::new(Some("ping"), r#"{"type":"ping"}"#));
    }
}

impl StreamFormatter for AnthropicStreamFormatter {
    fn format_deltas(&mut self, deltas: &[StreamDelta]) -> Vec<SseEvent> {
        let mut events = Vec::new();

        for delta in deltas {
            match delta {
                StreamDelta::MessageStart { id, model } => {
                    self.id = id.clone();
                    self.model = model.clone();
                    self.ensure_message_start(&mut events);
                }
                StreamDelta::ReasoningDelta(text) => {
                    self.ensure_message_start(&mut events);
                    self.close_text_block_if_open(&mut events);
                    if !self.in_thinking_block {
                        self.in_thinking_block = true;
                        let block_start = serde_json::json!({
                            "type": "content_block_start",
                            "index": self.block_index,
                            "content_block": {"type": "thinking", "thinking": ""}
                        });
                        events.push(SseEvent::new(
                            Some("content_block_start"),
                            block_start.to_string(),
                        ));
                    }
                    let delta_ev = serde_json::json!({
                        "type": "content_block_delta",
                        "index": self.block_index,
                        "delta": {"type": "thinking_delta", "thinking": text}
                    });
                    events.push(SseEvent::new(
                        Some("content_block_delta"),
                        delta_ev.to_string(),
                    ));
                }
                StreamDelta::TextDelta(text) => {
                    if !self.in_text_block && text.trim().is_empty() {
                        continue;
                    }
                    self.ensure_message_start(&mut events);
                    self.close_thinking_block_if_open(&mut events);
                    self.close_tool_block_if_open(&mut events);
                    if !self.in_text_block {
                        self.in_text_block = true;
                        let block_start = serde_json::json!({
                            "type": "content_block_start",
                            "index": self.block_index,
                            "content_block": {"type": "text", "text": ""}
                        });
                        events.push(SseEvent::new(
                            Some("content_block_start"),
                            block_start.to_string(),
                        ));
                    }
                    let delta_ev = serde_json::json!({
                        "type": "content_block_delta",
                        "index": self.block_index,
                        "delta": {"type": "text_delta", "text": text}
                    });
                    events.push(SseEvent::new(
                        Some("content_block_delta"),
                        delta_ev.to_string(),
                    ));
                }
                StreamDelta::ToolCallStart { index: _, id, name } => {
                    self.ensure_message_start(&mut events);
                    self.close_thinking_block_if_open(&mut events);
                    self.close_text_block_if_open(&mut events);
                    self.close_tool_block_if_open(&mut events);
                    let tool_use_id = if id.trim().is_empty() {
                        format!("toolu_{}", Uuid::new_v4().simple())
                    } else {
                        id.clone()
                    };
                    let block_start = serde_json::json!({
                        "type": "content_block_start",
                        "index": self.block_index,
                        "content_block": {"type": "tool_use", "id": tool_use_id, "name": name, "input": {}}
                    });
                    events.push(SseEvent::new(
                        Some("content_block_start"),
                        block_start.to_string(),
                    ));
                    self.in_tool_block = true;
                }
                StreamDelta::ToolCallDelta {
                    index: _,
                    arguments,
                } => {
                    let delta_ev = serde_json::json!({
                        "type": "content_block_delta",
                        "index": self.block_index,
                        "delta": {"type": "input_json_delta", "partial_json": arguments}
                    });
                    events.push(SseEvent::new(
                        Some("content_block_delta"),
                        delta_ev.to_string(),
                    ));
                }
                StreamDelta::Usage(u) => {
                    if u.input_tokens > 0 {
                        self.usage.input_tokens = u.input_tokens;
                    }
                    if u.output_tokens > 0 {
                        self.usage.output_tokens = u.output_tokens;
                    }
                }
                StreamDelta::Done { stop_reason } => {
                    self.ensure_message_start(&mut events);
                    self.close_thinking_block_if_open(&mut events);
                    self.close_text_block_if_open(&mut events);
                    self.close_tool_block_if_open(&mut events);
                    let anthropic_reason = match stop_reason.as_str() {
                        "stop" => "end_turn",
                        "tool_calls" => "tool_use",
                        other => other,
                    };
                    let msg_delta = serde_json::json!({
                        "type": "message_delta",
                        "delta": {"stop_reason": anthropic_reason},
                        "usage": {"output_tokens": self.usage.output_tokens}
                    });
                    events.push(SseEvent::new(Some("message_delta"), msg_delta.to_string()));
                    events.push(SseEvent::new(
                        Some("message_stop"),
                        r#"{"type":"message_stop"}"#,
                    ));
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

impl AnthropicStreamFormatter {
    fn close_text_block_if_open(&mut self, events: &mut Vec<SseEvent>) {
        if !self.in_text_block {
            return;
        }
        let stop = serde_json::json!({
            "type": "content_block_stop",
            "index": self.block_index,
        });
        events.push(SseEvent::new(Some("content_block_stop"), stop.to_string()));
        self.block_index += 1;
        self.in_text_block = false;
    }

    fn close_thinking_block_if_open(&mut self, events: &mut Vec<SseEvent>) {
        if !self.in_thinking_block {
            return;
        }
        let stop = serde_json::json!({
            "type": "content_block_stop",
            "index": self.block_index,
        });
        events.push(SseEvent::new(Some("content_block_stop"), stop.to_string()));
        self.block_index += 1;
        self.in_thinking_block = false;
    }

    fn close_tool_block_if_open(&mut self, events: &mut Vec<SseEvent>) {
        if !self.in_tool_block {
            return;
        }
        let stop = serde_json::json!({
            "type": "content_block_stop",
            "index": self.block_index,
        });
        events.push(SseEvent::new(Some("content_block_stop"), stop.to_string()));
        self.block_index += 1;
        self.in_tool_block = false;
    }
}

fn extract_anthropic_usage(v: &Value) -> TokenUsage {
    if let Some(u) = v.get("usage") {
        TokenUsage {
            input_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            output_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        }
    } else {
        TokenUsage::default()
    }
}
