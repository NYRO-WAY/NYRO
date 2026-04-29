use anyhow::Result;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;

use crate::protocol::types::*;
use crate::protocol::EgressEncoder;

pub struct AnthropicEncoder;

impl EgressEncoder for AnthropicEncoder {
    fn encode_request(&self, req: &InternalRequest) -> Result<(Value, HeaderMap)> {
        let mut system_text = String::new();
        let mut raw_messages = Vec::new();

        for msg in &req.messages {
            if msg.role == Role::System {
                if !system_text.is_empty() {
                    system_text.push('\n');
                }
                system_text.push_str(&msg.content.as_text());
                continue;
            }

            raw_messages.push(encode_message(msg)?);
        }
        let messages = normalize_anthropic_messages(raw_messages);

        let max_tokens = req.max_tokens.unwrap_or(4096);

        let mut body = serde_json::json!({
            "model": req.model,
            "messages": messages,
            "max_tokens": max_tokens,
            "stream": req.stream,
        });

        let obj = body.as_object_mut().unwrap();

        if !system_text.is_empty() {
            obj.insert("system".into(), Value::String(system_text));
        }
        if let Some(t) = req.temperature {
            obj.insert("temperature".into(), t.into());
        }
        if let Some(p) = req.top_p {
            obj.insert("top_p".into(), p.into());
        }

        if let Some(ref tools) = req.tools {
            let tools_val: Vec<Value> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.parameters,
                    })
                })
                .collect();
            obj.insert("tools".into(), Value::Array(tools_val));
        }

        if let Some(mapped_tool_choice) = req
            .tool_choice
            .as_ref()
            .and_then(map_tool_choice_for_anthropic)
        {
            obj.insert("tool_choice".into(), mapped_tool_choice);
        }

        validate_anthropic_payload(&body)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static("2023-06-01"),
        );

        Ok((body, headers))
    }

    fn egress_path(&self, _model: &str, _stream: bool) -> String {
        "/v1/messages".to_string()
    }
}

fn map_tool_choice_for_anthropic(raw: &Value) -> Option<Value> {
    if let Some(s) = raw.as_str() {
        return match s {
            "auto" => Some(serde_json::json!({ "type": "auto" })),
            "required" => Some(serde_json::json!({ "type": "any" })),
            "none" => None,
            _ => None,
        };
    }

    let obj = raw.as_object()?;
    let kind = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "auto" => Some(serde_json::json!({ "type": "auto" })),
        "required" | "any" => Some(serde_json::json!({ "type": "any" })),
        "none" => None,
        "tool" => {
            let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name.is_empty() {
                None
            } else {
                Some(serde_json::json!({ "type": "tool", "name": name }))
            }
        }
        "function" => {
            let name = obj
                .get("name")
                .and_then(|v| v.as_str())
                .or_else(|| {
                    obj.get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|v| v.as_str())
                })
                .unwrap_or("");
            if name.is_empty() {
                None
            } else {
                Some(serde_json::json!({ "type": "tool", "name": name }))
            }
        }
        _ => None,
    }
}

fn validate_anthropic_payload(body: &Value) -> Result<()> {
    let obj = body
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("anthropic payload must be object"))?;
    let _model = obj
        .get("model")
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("anthropic payload missing model"))?;
    let _max_tokens = obj
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("anthropic payload missing max_tokens"))?;
    let messages = obj
        .get("messages")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("anthropic payload missing messages"))?;
    if messages.is_empty() {
        anyhow::bail!("anthropic payload has empty messages");
    }
    for (idx, msg) in messages.iter().enumerate() {
        let role = msg
            .get("role")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("anthropic payload message[{idx}] missing role"))?;
        if role != "user" && role != "assistant" {
            anyhow::bail!("anthropic payload message[{idx}] invalid role: {role}");
        }

        if let Some(content) = msg.get("content") {
            match content {
                Value::String(_) => {}
                Value::Array(blocks) => {
                    for (bidx, block) in blocks.iter().enumerate() {
                        let btype = block
                            .get("type")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "anthropic payload message[{idx}] block[{bidx}] missing type"
                                )
                            })?;
                        match btype {
                            "text" => {}
                            "tool_use" => {
                                let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                                let name =
                                    block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                if id.is_empty() || name.is_empty() {
                                    anyhow::bail!(
                                        "anthropic payload message[{idx}] tool_use block[{bidx}] missing id/name"
                                    );
                                }
                            }
                            "tool_result" => {
                                let tool_use_id = block
                                    .get("tool_use_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                if tool_use_id.is_empty() {
                                    anyhow::bail!(
                                        "anthropic payload message[{idx}] tool_result block[{bidx}] missing tool_use_id"
                                    );
                                }
                            }
                            "image" | "thinking" => {}
                            other => {
                                anyhow::bail!(
                                    "anthropic payload message[{idx}] unsupported block type: {other}"
                                );
                            }
                        }
                    }
                }
                _ => {
                    anyhow::bail!("anthropic payload message[{idx}] content must be string or array");
                }
            }
        } else {
            anyhow::bail!("anthropic payload message[{idx}] missing content");
        }
    }

    if let Some(tool_choice) = obj.get("tool_choice") {
        let tc = tool_choice
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("anthropic tool_choice must be object"))?;
        let t = tc.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if t != "auto" && t != "any" && t != "tool" {
            anyhow::bail!("anthropic tool_choice invalid type: {t}");
        }
        if t == "tool" && tc.get("name").and_then(|v| v.as_str()).unwrap_or("").is_empty() {
            anyhow::bail!("anthropic tool_choice=tool missing name");
        }
    }

    Ok(())
}

fn encode_message(msg: &InternalMessage) -> Result<Value> {
    let role = match msg.role {
        Role::User | Role::Tool => "user",
        Role::Assistant => "assistant",
        Role::System => unreachable!("system handled separately"),
    };

    if msg.role == Role::Tool {
        let (tool_content, hinted_tool_use_id) = anthropic_tool_result_payload(msg);
        let tool_use_id = msg
            .tool_call_id
            .clone()
            .filter(|v| !v.trim().is_empty())
            .or(hinted_tool_use_id)
            .map(|v| normalize_anthropic_tool_id(&v))
            .unwrap_or_else(|| normalize_anthropic_tool_id("tool_result"));
        return Ok(serde_json::json!({
            "role": role,
            "content": [{
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": tool_content,
            }],
        }));
    }

    let content = match &msg.content {
        MessageContent::Text(t) => {
            if let Some(ref tcs) = msg.tool_calls {
                let mut blocks: Vec<Value> = vec![];
                if !t.is_empty() {
                    blocks.push(serde_json::json!({"type": "text", "text": t}));
                }
                for tc in tcs {
                    let input: Value =
                        serde_json::from_str(&tc.arguments).unwrap_or(Value::Object(Default::default()));
                    blocks.push(serde_json::json!({
                        "type": "tool_use",
                        "id": normalize_anthropic_tool_id(&tc.id),
                        "name": tc.name,
                        "input": input,
                    }));
                }
                Value::Array(blocks)
            } else {
                Value::String(t.clone())
            }
        }
        MessageContent::Blocks(blocks) => {
            let arr: Vec<Value> = blocks
                .iter()
                .map(|b| match b {
                    ContentBlock::Text { text } => {
                        serde_json::json!({"type": "text", "text": text})
                    }
                    ContentBlock::Image { source } => {
                        serde_json::json!({
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": source.media_type,
                                "data": source.data,
                            }
                        })
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        serde_json::json!({
                            "type": "tool_use",
                            "id": normalize_anthropic_tool_id(id),
                            "name": name,
                            "input": input,
                        })
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                    } => {
                        serde_json::json!({
                            "type": "tool_result",
                            "tool_use_id": normalize_anthropic_tool_id(tool_use_id),
                            "content": content,
                        })
                    }
                })
                .collect();
            Value::Array(arr)
        }
    };

    Ok(serde_json::json!({
        "role": role,
        "content": content,
    }))
}

fn anthropic_tool_result_payload(msg: &InternalMessage) -> (Value, Option<String>) {
    match &msg.content {
        MessageContent::Text(t) => (Value::String(t.clone()), None),
        MessageContent::Blocks(blocks) => {
            for block in blocks {
                if let ContentBlock::ToolResult { tool_use_id, content } = block {
                    return (content.clone(), Some(tool_use_id.clone()));
                }
            }
            (Value::String(msg.content.as_text()), None)
        }
    }
}

fn normalize_anthropic_tool_id(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "toolu_nyro".to_string();
    }
    if trimmed.starts_with("toolu_")
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return trimmed.to_string();
    }
    let sanitized: String = trimmed
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    format!("toolu_{sanitized}")
}

fn normalize_anthropic_messages(messages: Vec<Value>) -> Vec<Value> {
    let mut normalized: Vec<Value> = Vec::new();
    for msg in messages {
        let Some(role) = msg.get("role").and_then(|v| v.as_str()) else {
            continue;
        };
        let blocks = content_to_blocks(msg.get("content").cloned().unwrap_or(Value::Null));
        if blocks.is_empty() {
            continue;
        }

        if let Some(last) = normalized.last_mut() {
            let same_role = last.get("role").and_then(|v| v.as_str()) == Some(role);
            if same_role {
                if let Some(last_obj) = last.as_object_mut() {
                    let mut merged = content_to_blocks(
                        last_obj.get("content").cloned().unwrap_or(Value::Null),
                    );
                    merged.extend(blocks);
                    last_obj.insert("content".into(), Value::Array(merged));
                }
                continue;
            }
        }

        normalized.push(serde_json::json!({
            "role": role,
            "content": Value::Array(blocks),
        }));
    }
    normalized
}

fn content_to_blocks(content: Value) -> Vec<Value> {
    match content {
        Value::String(s) => {
            if s.trim().is_empty() {
                Vec::new()
            } else {
                vec![serde_json::json!({"type":"text","text":s})]
            }
        }
        Value::Array(arr) => arr
            .into_iter()
            .filter(|v| {
                let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
                if t == "text" {
                    !v.get("text")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .trim()
                        .is_empty()
                } else {
                    true
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}
