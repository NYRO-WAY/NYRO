use anyhow::Result;
use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::protocol::types::*;
use crate::protocol::EgressEncoder;

/// Encoder for the OpenAI Responses API (`POST /v1/responses`).
///
/// Produces a Responses-format body (`instructions`, `input[]`) and targets
/// `/v1/responses`. Forces `stream: true` because the Codex/Responses backend
/// only supports SSE; non-streaming ingress is aggregated downstream in the
/// proxy handler.
pub struct ResponsesEncoder;

impl EgressEncoder for ResponsesEncoder {
    fn encode_request(&self, req: &InternalRequest) -> Result<(Value, HeaderMap)> {
        let mut instructions: Vec<String> = Vec::new();
        let mut input: Vec<Value> = Vec::new();

        for message in &req.messages {
            match message.role {
                Role::System => {
                    let text = message.content.as_text();
                    if !text.is_empty() {
                        instructions.push(text);
                    }
                }
                Role::User | Role::Assistant => {
                    let text = message.content.as_text();
                    if !text.is_empty() {
                        let role_str = match message.role {
                            Role::User => "user",
                            Role::Assistant => "assistant",
                            _ => unreachable!(),
                        };
                        let content_type = if message.role == Role::Assistant {
                            "output_text"
                        } else {
                            "input_text"
                        };
                        input.push(serde_json::json!({
                            "type": "message",
                            "role": role_str,
                            "content": [{
                                "type": content_type,
                                "text": text,
                            }]
                        }));
                    }
                    if let Some(tool_calls) = &message.tool_calls {
                        for tool_call in tool_calls {
                            input.push(serde_json::json!({
                                "type": "function_call",
                                "call_id": tool_call.id,
                                "name": tool_call.name,
                                "arguments": tool_call.arguments,
                            }));
                        }
                    }
                }
                Role::Tool => {
                    input.push(serde_json::json!({
                        "type": "function_call_output",
                        "call_id": message.tool_call_id.clone().unwrap_or_default(),
                        "output": message.content.as_text(),
                    }));
                }
            }
        }

        if input.is_empty() {
            anyhow::bail!("responses request requires at least one input item");
        }

        let instructions_value = if instructions.is_empty() {
            Value::String("You are a helpful assistant.".to_string())
        } else {
            Value::String(instructions.join("\n\n"))
        };

        let mut body = serde_json::json!({
            "model": req.model,
            "store": false,
            "stream": true,
            "instructions": instructions_value,
            "input": input,
        });
        let obj = body.as_object_mut().unwrap();

        if let Some(t) = req.temperature {
            obj.insert("temperature".into(), t.into());
        }
        // Note: `max_output_tokens` is part of the Responses API spec, but the
        // Codex backend (`chatgpt.com/backend-api/codex`) rejects it. Upstreams
        // that need a cap can pass it via `extra`.
        if let Some(p) = req.top_p {
            obj.insert("top_p".into(), p.into());
        }

        if let Some(ref tools) = req.tools {
            let tools_val: Vec<Value> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    })
                })
                .collect();
            obj.insert("tools".into(), Value::Array(tools_val));
        }
        if let Some(ref tc) = req.tool_choice {
            obj.insert("tool_choice".into(), tc.clone());
        }

        for (k, v) in &req.extra {
            if matches!(
                k.as_str(),
                "messages" | "input" | "instructions" | "stream" | "store" | "model"
            ) {
                continue;
            }
            obj.entry(k.clone()).or_insert_with(|| v.clone());
        }

        Ok((body, HeaderMap::new()))
    }

    fn egress_path(&self, _model: &str, _stream: bool) -> String {
        "/v1/responses".to_string()
    }
}
