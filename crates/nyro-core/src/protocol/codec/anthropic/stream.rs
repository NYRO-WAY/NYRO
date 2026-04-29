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

        let mut thinking_parts: Vec<String> = Vec::new();
        if let Some(blocks) = resp.get("content").and_then(|c| c.as_array()) {
            for block in blocks {
                match block.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            content_text.push_str(text);
                        }
                    }
                    Some("thinking") => {
                        // Present in responses from Ollama and native Anthropic thinking models.
                        if let Some(text) = block.get("thinking").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                thinking_parts.push(text.to_string());
                            }
                        }
                    }
                    Some("tool_use") => {
                        if let (Some(tc_id), Some(name)) = (
                            block.get("id").and_then(|v| v.as_str()),
                            block.get("name").and_then(|v| v.as_str()),
                        ) {
                            let input = block.get("input").cloned().unwrap_or(Value::Object(Default::default()));
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
        let reasoning_content = if thinking_parts.is_empty() {
            None
        } else {
            Some(thinking_parts.join("\n"))
        };

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
            reasoning_content,
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

        if let Some(reasoning) = resp.reasoning_content.as_ref().map(|v| v.trim()).filter(|v| !v.is_empty()) {
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
            let idx = data
                .get("index")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
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
                    Some("thinking_delta") => {
                        // Produced by Ollama and native Anthropic thinking models.
                        // signature_delta events (native Anthropic only) are silently
                        // ignored below — the parser must not depend on them.
                        if let Some(text) = delta.get("thinking").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                deltas.push(StreamDelta::ReasoningDelta(text.to_string()));
                            }
                        }
                    }
                    Some("input_json_delta") => {
                        if let Some(json) = delta.get("partial_json").and_then(|t| t.as_str()) {
                            let idx = data
                                .get("index")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0)
                                as usize;
                            deltas.push(StreamDelta::ToolCallDelta {
                                index: idx,
                                arguments: json.to_string(),
                            });
                        }
                    }
                    // signature_delta (native Anthropic) and other unknown types are
                    // intentionally ignored.
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
                let output = u
                    .get("output_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
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
                StreamDelta::ToolCallDelta { index: _, arguments } => {
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
            input_tokens: u
                .get("input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            output_tokens: u
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
        }
    } else {
        TokenUsage::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ResponseParser, StreamParser};

    fn make_sse_block(event: &str, data: &str) -> String {
        format!("event: {event}\ndata: {data}\n\n")
    }

    // ── AnthropicResponseParser ──

    #[test]
    fn test_parse_response_text_only() {
        let resp = serde_json::json!({
            "id": "msg_1",
            "model": "claude-3-5-sonnet",
            "content": [{"type": "text", "text": "hello"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 5, "output_tokens": 3}
        });
        let r = AnthropicResponseParser.parse_response(resp).unwrap();
        assert_eq!(r.content, "hello");
        assert!(r.reasoning_content.is_none());
        assert_eq!(r.stop_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn test_parse_response_thinking_and_text() {
        // Ollama returns thinking + text blocks in non-stream response.
        let resp = serde_json::json!({
            "id": "msg_2",
            "model": "qwen3",
            "content": [
                {"type": "thinking", "thinking": "let me think..."},
                {"type": "text", "text": "hi there"}
            ],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 20}
        });
        let r = AnthropicResponseParser.parse_response(resp).unwrap();
        assert_eq!(r.content, "hi there");
        assert_eq!(r.reasoning_content.as_deref(), Some("let me think..."));
    }

    #[test]
    fn test_parse_response_tool_use() {
        let resp = serde_json::json!({
            "id": "msg_3",
            "model": "claude-3-5-sonnet",
            "content": [{
                "type": "tool_use",
                "id": "toolu_01",
                "name": "get_weather",
                "input": {"city": "Paris"}
            }],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 15, "output_tokens": 8}
        });
        let r = AnthropicResponseParser.parse_response(resp).unwrap();
        assert_eq!(r.tool_calls.len(), 1);
        assert_eq!(r.tool_calls[0].name, "get_weather");
        assert_eq!(r.stop_reason.as_deref(), Some("tool_calls"));
    }

    // ── AnthropicStreamParser ──

    #[test]
    fn test_stream_basic_text() {
        let sse = [
            make_sse_block(
                "message_start",
                r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[],"model":"claude-3-5-sonnet","stop_reason":null,"usage":{"input_tokens":9,"output_tokens":0}}}"#,
            ),
            make_sse_block(
                "content_block_start",
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            ),
            make_sse_block(
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello"}}"#,
            ),
            make_sse_block("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
            make_sse_block(
                "message_delta",
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}"#,
            ),
            make_sse_block("message_stop", r#"{"type":"message_stop"}"#),
        ]
        .concat();

        let mut parser = AnthropicStreamParser::new();
        let deltas = parser.parse_chunk(&sse).unwrap();

        let has_text = deltas.iter().any(|d| matches!(d, StreamDelta::TextDelta(t) if t == "hello"));
        let has_done = deltas.iter().any(|d| matches!(d, StreamDelta::Done { stop_reason } if stop_reason == "stop"));
        assert!(has_text, "expected TextDelta('hello'), got: {deltas:?}");
        assert!(has_done, "expected Done(stop), got: {deltas:?}");
    }

    #[test]
    fn test_stream_thinking_delta_no_signature_delta() {
        // Ollama sends thinking_delta events but omits signature_delta entirely.
        // Parser must not fail and must emit ReasoningDelta.
        let sse = [
            make_sse_block(
                "message_start",
                r#"{"type":"message_start","message":{"id":"msg_2","type":"message","role":"assistant","content":[],"model":"qwen3","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":0}}}"#,
            ),
            make_sse_block(
                "content_block_start",
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#,
            ),
            make_sse_block(
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"step one"}}"#,
            ),
            make_sse_block(
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":" step two"}}"#,
            ),
            // No signature_delta here (Ollama omits it)
            make_sse_block("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
            make_sse_block(
                "content_block_start",
                r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
            ),
            make_sse_block(
                "content_block_delta",
                r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"answer"}}"#,
            ),
            make_sse_block("content_block_stop", r#"{"type":"content_block_stop","index":1}"#),
            make_sse_block(
                "message_delta",
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":15}}"#,
            ),
            make_sse_block("message_stop", r#"{"type":"message_stop"}"#),
        ]
        .concat();

        let mut parser = AnthropicStreamParser::new();
        let deltas = parser.parse_chunk(&sse).unwrap();

        let reasoning: Vec<_> = deltas
            .iter()
            .filter_map(|d| if let StreamDelta::ReasoningDelta(t) = d { Some(t.as_str()) } else { None })
            .collect();
        assert!(!reasoning.is_empty(), "expected ReasoningDelta events, got: {deltas:?}");
        assert!(reasoning.contains(&"step one"), "expected 'step one', got: {reasoning:?}");

        let has_text = deltas.iter().any(|d| matches!(d, StreamDelta::TextDelta(t) if t == "answer"));
        assert!(has_text, "expected TextDelta('answer'), got: {deltas:?}");
    }

    #[test]
    fn test_stream_signature_delta_is_silently_ignored() {
        // Native Anthropic sends signature_delta after thinking block.
        // Parser must not fail, just ignore it.
        let sse = [
            make_sse_block(
                "message_start",
                r#"{"type":"message_start","message":{"id":"msg_3","model":"claude-3-7-sonnet","content":[],"stop_reason":null,"usage":{"input_tokens":8,"output_tokens":0}}}"#,
            ),
            make_sse_block(
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"think"}}"#,
            ),
            // signature_delta must be silently ignored
            make_sse_block(
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"abc123"}}"#,
            ),
            make_sse_block(
                "message_delta",
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}"#,
            ),
        ]
        .concat();

        let mut parser = AnthropicStreamParser::new();
        let result = parser.parse_chunk(&sse);
        assert!(result.is_ok(), "parser must not fail on signature_delta");

        let deltas = result.unwrap();
        let no_unknown = deltas
            .iter()
            .all(|d| !matches!(d, StreamDelta::TextDelta(t) if t.contains("abc123")));
        assert!(no_unknown, "signature_delta content must not appear in output");
    }

    #[test]
    fn test_stream_tool_use() {
        let sse = [
            make_sse_block(
                "message_start",
                r#"{"type":"message_start","message":{"id":"msg_4","model":"claude-3-5-sonnet","content":[],"stop_reason":null,"usage":{"input_tokens":20,"output_tokens":0}}}"#,
            ),
            make_sse_block(
                "content_block_start",
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_01","name":"get_weather","input":{}}}"#,
            ),
            make_sse_block(
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"city\":"}}"#,
            ),
            make_sse_block(
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"Paris\"}"}}"#,
            ),
            make_sse_block("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
            make_sse_block(
                "message_delta",
                r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":12}}"#,
            ),
        ]
        .concat();

        let mut parser = AnthropicStreamParser::new();
        let deltas = parser.parse_chunk(&sse).unwrap();

        let has_tool_start = deltas
            .iter()
            .any(|d| matches!(d, StreamDelta::ToolCallStart { name, .. } if name == "get_weather"));
        let has_tool_delta = deltas
            .iter()
            .any(|d| matches!(d, StreamDelta::ToolCallDelta { .. }));
        let has_done_tool = deltas
            .iter()
            .any(|d| matches!(d, StreamDelta::Done { stop_reason } if stop_reason == "tool_calls"));
        assert!(has_tool_start, "expected ToolCallStart(get_weather), got: {deltas:?}");
        assert!(has_tool_delta, "expected ToolCallDelta, got: {deltas:?}");
        assert!(has_done_tool, "expected Done(tool_calls), got: {deltas:?}");
    }
}
