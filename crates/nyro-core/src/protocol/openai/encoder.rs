use anyhow::Result;
use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::protocol::EgressEncoder;
use crate::protocol::types::*;

pub struct OpenAIEncoder;

impl EgressEncoder for OpenAIEncoder {
    fn encode_request(
        &self,
        req: &InternalRequest,
    ) -> Result<(Value, HeaderMap)> {
        let messages: Vec<Value> = req
            .messages
            .iter()
            .map(encode_message)
            .collect::<Result<Vec<_>>>()?;

        let mut body = serde_json::json!({
            "model": req.model,
            "messages": messages,
            "stream": req.stream,
        });

        let obj = body.as_object_mut().unwrap();

        if let Some(t) = req.temperature {
            obj.insert("temperature".into(), t.into());
        }
        if let Some(m) = req.max_tokens {
            obj.insert("max_tokens".into(), m.into());
        }
        if let Some(p) = req.top_p {
            obj.insert("top_p".into(), p.into());
        }
        if let Some(ref tools) = req.tools {
            obj.insert("tools".into(), Value::Array(tools.clone()));
        }
        if let Some(ref tc) = req.tool_choice {
            obj.insert("tool_choice".into(), tc.clone());
        }

        if req.stream {
            obj.insert(
                "stream_options".into(),
                serde_json::json!({"include_usage": true}),
            );
        }

        for (k, v) in &req.extra {
            obj.entry(k.clone()).or_insert_with(|| v.clone());
        }

        Ok((body, HeaderMap::new()))
    }
}

fn encode_message(msg: &InternalMessage) -> Result<Value> {
    let role = match msg.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };

    let mut obj = serde_json::json!({ "role": role });
    let map = obj.as_object_mut().unwrap();

    match &msg.content {
        MessageContent::Text(t) => {
            map.insert("content".into(), Value::String(t.clone()));
        }
        MessageContent::Blocks(blocks) => {
            let parts: Vec<Value> = blocks
                .iter()
                .map(|b| match b {
                    ContentBlock::Text { text } => {
                        serde_json::json!({"type": "text", "text": text})
                    }
                    ContentBlock::Image { source } => {
                        serde_json::json!({
                            "type": "image_url",
                            "image_url": {"url": source.data}
                        })
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        serde_json::json!({
                            "type": "function",
                            "id": id,
                            "function": {"name": name, "arguments": input.to_string()}
                        })
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                    } => {
                        serde_json::json!({
                            "type": "text",
                            "text": content.to_string(),
                            "tool_call_id": tool_use_id,
                        })
                    }
                })
                .collect();
            map.insert("content".into(), Value::Array(parts));
        }
    }

    if let Some(ref tc) = msg.tool_calls {
        map.insert("tool_calls".into(), Value::Array(tc.clone()));
    }
    if let Some(ref tid) = msg.tool_call_id {
        map.insert("tool_call_id".into(), Value::String(tid.clone()));
    }

    Ok(obj)
}

pub fn egress_path() -> &'static str {
    "/v1/chat/completions"
}
