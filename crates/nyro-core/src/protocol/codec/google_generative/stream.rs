use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;

use crate::protocol::ir::request::ToolCall;
use crate::protocol::ir::usage::Usage;
use crate::protocol::ir::{AiResponse, AiStreamDelta};
use crate::protocol::*;

// ── Non-streaming response parser ──

pub struct GoogleResponseParser;

impl ResponseDecoder for GoogleResponseParser {
    fn parse_response(&self, resp: Value) -> Result<AiResponse> {
        let candidate = resp
            .get("candidates")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first());

        let content_obj = candidate.and_then(|c| c.get("content"));

        let mut text = String::new();
        let mut tool_calls = Vec::new();

        if let Some(parts) = content_obj
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())
        {
            for part in parts {
                if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                    text.push_str(t);
                }
                if let Some(fc) = part.get("functionCall") {
                    let name = fc
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    let args = fc
                        .get("args")
                        .cloned()
                        .unwrap_or(Value::Object(Default::default()));
                    tool_calls.push(ToolCall {
                        id: format!("call_{}", uuid::Uuid::new_v4().simple()),
                        name,
                        arguments: args.to_string(),
                    });
                }
            }
        }

        let stop_reason = candidate
            .and_then(|c| c.get("finishReason"))
            .and_then(|v| v.as_str())
            .map(|r| match r {
                "STOP" => "stop".to_string(),
                "MAX_TOKENS" => "length".to_string(),
                other => other.to_lowercase(),
            });

        let usage = extract_gemini_usage(&resp);

        let model = resp
            .get("modelVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mut ai_resp = AiResponse::new(format!("gen-{}", uuid::Uuid::new_v4().simple()), model);
        ai_resp.content = text;
        ai_resp.tool_calls = tool_calls;
        ai_resp.stop_reason = stop_reason;
        ai_resp.usage = usage;
        Ok(ai_resp)
    }
}

// ── Non-streaming response formatter ──

pub struct GoogleResponseFormatter;

impl ResponseEncoder for GoogleResponseFormatter {
    fn format_response(&self, resp: &AiResponse) -> Value {
        let mut parts = Vec::new();

        if !resp.content.is_empty() {
            parts.push(serde_json::json!({"text": resp.content}));
        }

        for tc in &resp.tool_calls {
            let args: Value =
                serde_json::from_str(&tc.arguments).unwrap_or(Value::Object(Default::default()));
            parts.push(serde_json::json!({
                "functionCall": {"name": tc.name, "args": args}
            }));
        }

        let finish_reason = resp.stop_reason.as_deref().map(|r| match r {
            "stop" => "STOP",
            "length" => "MAX_TOKENS",
            "tool_calls" => "STOP",
            other => other,
        });

        serde_json::json!({
            "candidates": [{
                "content": {"role": "model", "parts": parts},
                "finishReason": finish_reason,
            }],
            "usageMetadata": {
                "promptTokenCount": resp.usage.prompt_tokens,
                "candidatesTokenCount": resp.usage.completion_tokens,
                "totalTokenCount": resp.usage.prompt_tokens + resp.usage.completion_tokens,
            }
        })
    }
}

// ── Stream parser (upstream Gemini SSE → deltas) ──

pub struct GoogleStreamParser {
    buffer: String,
    first: bool,
}

impl Default for GoogleStreamParser {
    fn default() -> Self {
        Self::new()
    }
}

impl GoogleStreamParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            first: true,
        }
    }
}

impl StreamResponseDecoder for GoogleStreamParser {
    fn parse_chunk(&mut self, raw: &str) -> Result<Vec<AiStreamDelta>> {
        self.buffer.push_str(raw);
        let mut deltas = Vec::new();

        while let Some(pos) = self.buffer.find("\n\n") {
            let block = self.buffer[..pos].to_string();
            self.buffer = self.buffer[pos + 2..].to_string();

            for line in block.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    let data = data.trim();
                    if let Ok(chunk) = serde_json::from_str::<Value>(data) {
                        parse_gemini_chunk(&chunk, &mut deltas, &mut self.first);
                    }
                }
            }
        }

        Ok(deltas)
    }

    fn finish(&mut self) -> Result<Vec<AiStreamDelta>> {
        if self.buffer.trim().is_empty() {
            return Ok(vec![]);
        }
        let remaining = std::mem::take(&mut self.buffer);
        self.parse_chunk(&format!("{remaining}\n\n"))
    }
}

fn parse_gemini_chunk(chunk: &Value, deltas: &mut Vec<AiStreamDelta>, first: &mut bool) {
    if *first {
        *first = false;
        let model = chunk
            .get("modelVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        deltas.push(AiStreamDelta::MessageStart {
            id: format!("gen-{}", uuid::Uuid::new_v4().simple()),
            model,
        });
    }

    let candidate = chunk
        .get("candidates")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first());

    if let Some(parts) = candidate
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
    {
        for part in parts {
            if let Some(text) = part.get("text").and_then(|t| t.as_str())
                && !text.is_empty()
            {
                deltas.push(AiStreamDelta::TextDelta(text.to_string()));
            }
            if let Some(fc) = part.get("functionCall") {
                let name = fc
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                let id = format!("call_{}", uuid::Uuid::new_v4().simple());
                deltas.push(AiStreamDelta::ToolCallStart {
                    index: 0,
                    id,
                    name: name.clone(),
                });
                let args = fc.get("args").map(|a| a.to_string()).unwrap_or_default();
                if !args.is_empty() && args != "{}" {
                    deltas.push(AiStreamDelta::ToolCallDelta {
                        index: 0,
                        arguments: args,
                    });
                }
            }
        }
    }

    if let Some(reason) = candidate
        .and_then(|c| c.get("finishReason"))
        .and_then(|v| v.as_str())
    {
        let normalized = match reason {
            "STOP" => "stop",
            "MAX_TOKENS" => "length",
            other => other,
        };
        deltas.push(AiStreamDelta::Done {
            stop_reason: normalized.to_string(),
        });
    }

    let u = extract_gemini_usage(chunk);
    if u.prompt_tokens > 0 || u.completion_tokens > 0 {
        deltas.push(AiStreamDelta::Usage(u));
    }
}

// ── Stream formatter (deltas → Gemini SSE) ──

pub struct GoogleStreamFormatter {
    usage: Usage,
    model: String,
    tool_names: HashMap<usize, String>,
    tool_arg_buffers: HashMap<usize, String>,
}

impl Default for GoogleStreamFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl GoogleStreamFormatter {
    pub fn new() -> Self {
        Self {
            usage: Usage::default(),
            model: String::new(),
            tool_names: HashMap::new(),
            tool_arg_buffers: HashMap::new(),
        }
    }
}

impl StreamResponseEncoder for GoogleStreamFormatter {
    fn format_deltas(&mut self, deltas: &[AiStreamDelta]) -> Vec<SseEvent> {
        let mut events = Vec::new();

        for delta in deltas {
            match delta {
                AiStreamDelta::MessageStart { model, .. } => {
                    self.model = model.clone();
                }
                AiStreamDelta::ThinkingDelta(text) => {
                    let chunk = serde_json::json!({
                        "candidates": [{
                            "content": {"role": "model", "parts": [{"text": text}]},
                        }],
                        "modelVersion": self.model,
                    });
                    events.push(SseEvent::new(None, chunk.to_string()));
                }
                AiStreamDelta::ThinkingSignature(_) => {}
                AiStreamDelta::TextDelta(text) => {
                    let chunk = serde_json::json!({
                        "candidates": [{
                            "content": {"role": "model", "parts": [{"text": text}]},
                        }],
                        "modelVersion": self.model,
                    });
                    events.push(SseEvent::new(None, chunk.to_string()));
                }
                AiStreamDelta::ToolCallStart { index, id: _, name } => {
                    self.tool_names.insert(*index, name.clone());
                    self.tool_arg_buffers.insert(*index, String::new());
                }
                AiStreamDelta::ToolCallDelta { index, arguments } => {
                    let Some(name) = self.tool_names.get(index).cloned() else {
                        continue;
                    };
                    let buf = self.tool_arg_buffers.entry(*index).or_default();
                    buf.push_str(arguments);
                    let Ok(args) = serde_json::from_str::<Value>(buf) else {
                        continue;
                    };
                    let normalized_args = normalize_tool_args(&name, args);
                    let chunk = serde_json::json!({
                        "candidates": [{
                            "content": {"role": "model", "parts": [{
                                "functionCall": {"name": name, "args": normalized_args}
                            }]},
                        }],
                    });
                    events.push(SseEvent::new(None, chunk.to_string()));
                }
                AiStreamDelta::Usage(u) => {
                    if u.prompt_tokens > 0 {
                        self.usage.prompt_tokens = u.prompt_tokens;
                    }
                    if u.completion_tokens > 0 {
                        self.usage.completion_tokens = u.completion_tokens;
                    }
                }
                AiStreamDelta::Done { stop_reason } => {
                    let gemini_reason = match stop_reason.as_str() {
                        "stop" => "STOP",
                        "length" => "MAX_TOKENS",
                        other => other,
                    };
                    let chunk = serde_json::json!({
                        "candidates": [{
                            "content": {"role": "model", "parts": []},
                            "finishReason": gemini_reason,
                        }],
                        "usageMetadata": {
                            "promptTokenCount": self.usage.prompt_tokens,
                            "candidatesTokenCount": self.usage.completion_tokens,
                            "totalTokenCount": self.usage.prompt_tokens + self.usage.completion_tokens,
                        }
                    });
                    events.push(SseEvent::new(None, chunk.to_string()));
                }
                _ => {}
            }
        }

        events
    }

    fn format_done(&mut self) -> Vec<SseEvent> {
        vec![]
    }

    fn usage(&self) -> Usage {
        self.usage.clone()
    }
}

fn extract_gemini_usage(v: &Value) -> Usage {
    let usage = v
        .get("usageMetadata")
        .or_else(|| v.get("usage_metadata"))
        .or_else(|| v.get("usage"));
    let Some(u) = usage else {
        return Usage::default();
    };

    let input = first_u64(
        u,
        &[
            "promptTokenCount",
            "prompt_tokens",
            "inputTokenCount",
            "input_tokens",
        ],
    )
    .unwrap_or(0);
    let output = first_u64(
        u,
        &[
            "candidatesTokenCount",
            "completion_tokens",
            "outputTokenCount",
            "output_tokens",
        ],
    )
    .unwrap_or(0);

    Usage {
        prompt_tokens: input as u32,
        completion_tokens: output as u32,
        ..Usage::default()
    }
}

fn first_u64(obj: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|k| obj.get(*k).and_then(|v| v.as_u64()))
}

fn normalize_tool_args(tool_name: &str, mut args: Value) -> Value {
    let Some(obj) = args.as_object_mut() else {
        return args;
    };

    if let Some(v) = obj.get("exclude_patterns").cloned() {
        obj.insert(
            "exclude_patterns".to_string(),
            normalize_stringified_string_array(v),
        );
    }
    if let Some(v) = obj.remove("exclude_pattern") {
        let normalized = match v {
            Value::String(s) => Value::Array(vec![Value::String(s)]),
            other => normalize_stringified_string_array(other),
        };
        obj.entry("exclude_patterns".to_string())
            .or_insert(normalized);
    }

    match tool_name {
        "glob" => {
            if let Some(v) = obj.remove("include_pattern") {
                obj.entry("pattern".to_string()).or_insert(v);
            }
            if let Some(v) = obj.remove("path") {
                obj.entry("root_dir".to_string()).or_insert(v);
            }
            if let Some(v) = obj.remove("search_root") {
                obj.entry("root_dir".to_string()).or_insert(v);
            }
        }
        "list_directory" => {
            if let Some(v) = obj.remove("path") {
                obj.entry("dir_path".to_string()).or_insert(v);
            }
        }
        _ => {}
    }

    args
}

fn normalize_stringified_string_array(v: Value) -> Value {
    match v {
        Value::String(s) => {
            let parsed = serde_json::from_str::<Value>(&s).ok();
            if let Some(Value::Array(arr)) = parsed {
                let only_strings = arr.iter().all(|item| item.is_string());
                if only_strings {
                    return Value::Array(arr);
                }
            }
            Value::String(s)
        }
        Value::Array(arr) => Value::Array(arr),
        other => other,
    }
}
