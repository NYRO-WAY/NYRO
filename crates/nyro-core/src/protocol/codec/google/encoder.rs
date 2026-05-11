use anyhow::Result;
use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::protocol::EgressEncoder;
use crate::protocol::types::*;

pub struct GoogleEncoder;

impl EgressEncoder for GoogleEncoder {
    fn encode_request(&self, req: &InternalRequest) -> Result<(Value, HeaderMap)> {
        // ── System instruction ────────────────────────────────────────────────
        let system_val: Option<Value> =
            if let Some(v) = req.extra.get("__google_raw_system_instruction") {
                Some(v.clone())
            } else {
                let mut system_parts: Vec<Value> = Vec::new();
                for msg in &req.messages {
                    if msg.role == Role::System {
                        system_parts.push(serde_json::json!({"text": msg.content.as_text()}));
                    }
                }
                if system_parts.is_empty() {
                    None
                } else {
                    Some(serde_json::json!({"parts": system_parts}))
                }
            };

        // ── Contents ─────────────────────────────────────────────────────────
        let mut contents: Vec<Value> = Vec::new();
        for msg in &req.messages {
            if msg.role == Role::System {
                continue;
            }
            contents.push(encode_content(msg)?);
        }

        let mut body = serde_json::json!({ "contents": contents });
        let obj = body.as_object_mut().unwrap();

        if let Some(sv) = system_val {
            obj.insert("systemInstruction".into(), sv);
        }

        // ── generationConfig ──────────────────────────────────────────────────
        // Start from extra (full preserved config) and layer InternalRequest
        // overrides on top so model-override and routing changes still apply.
        let mut gen_config: serde_json::Map<String, Value> =
            if let Some(Value::Object(m)) = req.extra.get("__google_generation_config") {
                m.clone()
            } else {
                serde_json::Map::new()
            };

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

        // ── Tools ─────────────────────────────────────────────────────────────
        // Prefer raw tools (preserves built-ins) if present.
        if let Some(raw) = req.extra.get("__google_raw_tools") {
            obj.insert("tools".into(), raw.clone());
        } else if let Some(ref tools) = req.tools {
            let mut fn_decls: Vec<Value> = Vec::new();
            let mut builtin_entries: Vec<Value> = Vec::new();

            for t in tools {
                match t.name.as_str() {
                    "__builtin__google_search" => {
                        builtin_entries.push(serde_json::json!({"googleSearch": {}}));
                    }
                    "__builtin__code_execution" => {
                        builtin_entries.push(serde_json::json!({"codeExecution": {}}));
                    }
                    "__builtin__google_search_retrieval" => {
                        builtin_entries.push(serde_json::json!({"googleSearchRetrieval": {}}));
                    }
                    _ => {
                        let mut decl = serde_json::json!({"name": t.name});
                        let d = decl.as_object_mut().unwrap();
                        if let Some(ref desc) = t.description {
                            d.insert("description".into(), Value::String(desc.clone()));
                        }
                        d.insert("parameters".into(), sanitize_gemini_schema(&t.parameters));
                        fn_decls.push(decl);
                    }
                }
            }

            let mut tool_array: Vec<Value> = Vec::new();
            if !fn_decls.is_empty() {
                tool_array.push(serde_json::json!({"functionDeclarations": fn_decls}));
            }
            tool_array.extend(builtin_entries);

            if !tool_array.is_empty() {
                obj.insert("tools".into(), Value::Array(tool_array));
            }
        }

        // ── PR-11 extra passthrough fields ────────────────────────────────────
        if let Some(v) = req.extra.get("__google_tool_config") {
            obj.insert("toolConfig".into(), v.clone());
        }
        if let Some(v) = req.extra.get("__google_safety_settings") {
            obj.insert("safetySettings".into(), v.clone());
        }
        if let Some(v) = req.extra.get("__google_cached_content") {
            obj.insert("cachedContent".into(), v.clone());
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

// ── Schema sanitisation ───────────────────────────────────────────────────────

fn sanitize_gemini_schema(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                if matches!(
                    k.as_str(),
                    "$schema" | "additionalProperties" | "$ref" | "ref" | "definitions" | "$defs"
                ) {
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

// ── Content encoding ──────────────────────────────────────────────────────────

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
                    parts
                        .push(serde_json::json!({"functionCall": {"name": tc.name, "args": args}}));
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
                ContentBlock::Reasoning { text, .. } => {
                    serde_json::json!({"text": text})
                }
            })
            .collect(),
    };

    Ok(serde_json::json!({"role": role, "parts": parts}))
}
