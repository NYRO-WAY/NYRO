use anyhow::Result;
use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::protocol::EgressEncoder;
use crate::protocol::types::*;

pub struct GeminiEncoder;

impl EgressEncoder for GeminiEncoder {
    fn encode_request(&self, req: &InternalRequest) -> Result<(Value, HeaderMap)> {
        let mut contents = Vec::new();
        let mut system_parts = Vec::new();

        for msg in &req.messages {
            if msg.role == Role::System {
                system_parts.push(serde_json::json!({"text": msg.content.as_text()}));
                continue;
            }
            contents.push(encode_content(msg)?);
        }

        let mut body = serde_json::json!({
            "contents": contents,
        });

        let obj = body.as_object_mut().unwrap();

        if !system_parts.is_empty() {
            obj.insert(
                "systemInstruction".into(),
                serde_json::json!({"parts": system_parts}),
            );
        }

        let mut gen_config = serde_json::Map::new();
        if let Some(t) = req.temperature {
            gen_config.insert("temperature".into(), t.into());
        }
        if let Some(m) = req.max_tokens {
            gen_config.insert("maxOutputTokens".into(), m.into());
        }
        if let Some(p) = req.top_p {
            gen_config.insert("topP".into(), p.into());
        }
        if !gen_config.is_empty() {
            obj.insert("generationConfig".into(), Value::Object(gen_config));
        }

        if let Some(ref tools) = req.tools {
            let decls: Vec<Value> = tools
                .iter()
                .map(|t| {
                    let mut decl = serde_json::json!({
                        "name": t.name,
                    });
                    let d = decl.as_object_mut().unwrap();
                    if let Some(ref desc) = t.description {
                        d.insert("description".into(), Value::String(desc.clone()));
                    }
                    d.insert("parameters".into(), sanitize_gemini_schema(&t.parameters));
                    decl
                })
                .collect();
            obj.insert(
                "tools".into(),
                serde_json::json!([{"functionDeclarations": decls}]),
            );
        }

        Ok((body, HeaderMap::new()))
    }

    fn egress_path(&self, model: &str, stream: bool) -> String {
        if stream {
            format!("/v1beta/models/{}:streamGenerateContent?alt=sse", model)
        } else {
            format!("/v1beta/models/{}:generateContent", model)
        }
    }
}

fn sanitize_gemini_schema(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                // Gemini functionDeclarations.parameters rejects many JSON Schema keys.
                if k == "$schema"
                    || k == "additionalProperties"
                    || k == "$ref"
                    || k == "ref"
                    || k == "definitions"
                    || k == "$defs"
                {
                    continue;
                }
                out.insert(k.clone(), sanitize_gemini_schema(v));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(sanitize_gemini_schema).collect()),
        _ => value.clone(),
    }
}

fn encode_content(msg: &InternalMessage) -> Result<Value> {
    let role = match msg.role {
        Role::User | Role::Tool => "user",
        Role::Assistant => "model",
        Role::System => unreachable!("system handled separately"),
    };

    let parts = match &msg.content {
        MessageContent::Text(t) => {
            if msg.tool_call_id.is_some() {
                vec![serde_json::json!({
                    "functionResponse": {
                        "name": msg.tool_call_id,
                        "response": {"result": t}
                    }
                })]
            } else if let Some(ref tcs) = msg.tool_calls {
                let mut parts = Vec::new();
                if !t.is_empty() {
                    parts.push(serde_json::json!({"text": t}));
                }
                for tc in tcs {
                    let args: Value = serde_json::from_str(&tc.arguments)
                        .unwrap_or(Value::Object(Default::default()));
                    parts.push(serde_json::json!({
                        "functionCall": {"name": tc.name, "args": args}
                    }));
                }
                parts
            } else {
                vec![serde_json::json!({"text": t})]
            }
        }
        MessageContent::Blocks(blocks) => blocks
            .iter()
            .map(|b| match b {
                ContentBlock::Text { text } => serde_json::json!({"text": text}),
                ContentBlock::Image { source } => {
                    serde_json::json!({
                        "inlineData": {
                            "mimeType": source.media_type,
                            "data": source.data,
                        }
                    })
                }
                ContentBlock::ToolUse { id: _, name, input } => {
                    serde_json::json!({"functionCall": {"name": name, "args": input}})
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                } => {
                    serde_json::json!({
                        "functionResponse": {"name": tool_use_id, "response": content}
                    })
                }
            })
            .collect(),
    };

    Ok(serde_json::json!({"role": role, "parts": parts}))
}
