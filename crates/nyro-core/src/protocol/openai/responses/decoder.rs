use std::collections::HashMap;

use anyhow::Result;
use serde_json::Value;

use crate::protocol::IngressDecoder;
use crate::protocol::ids::OPENAI_RESPONSES_V1;
use crate::protocol::types::*;

pub struct ResponsesDecoder;

impl IngressDecoder for ResponsesDecoder {
    fn decode_request(&self, body: Value) -> Result<InternalRequest> {
        let obj = body
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("request body must be a JSON object"))?;

        let model = obj
            .get("model")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing 'model' field"))?
            .to_string();

        let stream = obj.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
        let temperature = obj.get("temperature").and_then(|v| v.as_f64());
        let max_tokens = obj
            .get("max_output_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let top_p = obj.get("top_p").and_then(|v| v.as_f64());

        let mut messages = Vec::new();
        let tools = parse_tools(obj.get("tools"))?;
        let tool_choice = obj.get("tool_choice").cloned();

        if let Some(inst) = obj.get("instructions").and_then(|v| v.as_str()) {
            if !inst.is_empty() {
                messages.push(InternalMessage {
                    role: Role::System,
                    content: MessageContent::Text(inst.to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
        }

        let input = obj
            .get("input")
            .ok_or_else(|| anyhow::anyhow!("missing 'input' field"))?;

        match input {
            Value::String(text) => {
                messages.push(InternalMessage {
                    role: Role::User,
                    content: MessageContent::Text(text.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
            Value::Array(items) => {
                for item in items {
                    if let Some(msg) = decode_input_item(item)? {
                        messages.push(msg);
                    }
                }
            }
            _ => anyhow::bail!("'input' must be a string or array"),
        }

        if messages.is_empty() {
            anyhow::bail!("no messages found in input");
        }

        let known: &[&str] = &[
            "model",
            "input",
            "instructions",
            "max_output_tokens",
            "stream",
            "temperature",
            "top_p",
            "tools",
            "tool_choice",
        ];
        let extra: HashMap<String, Value> = obj
            .iter()
            .filter(|(k, _)| !known.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        Ok(InternalRequest {
            messages,
            model,
            stream,
            temperature,
            max_tokens,
            top_p,
            tools,
            tool_choice,
            source_protocol: OPENAI_RESPONSES_V1,
            extra,
        })
    }
}

fn decode_input_item(item: &Value) -> Result<Option<InternalMessage>> {
    let item_type = item
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("message");

    if item_type == "function_call_output" {
        let call_id = item
            .get("call_id")
            .or_else(|| item.get("tool_call_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if call_id.trim().is_empty() {
            anyhow::bail!("function_call_output missing call_id");
        }
        let output = item.get("output").cloned().unwrap_or(Value::Null);
        let output_text = match output {
            Value::String(s) => s,
            Value::Null => String::new(),
            other => other.to_string(),
        };

        return Ok(Some(InternalMessage {
            role: Role::Tool,
            content: MessageContent::Text(output_text),
            tool_calls: None,
            tool_call_id: Some(call_id),
        }));
    }

    if item_type == "function_call" {
        let call_id = item
            .get("call_id")
            .or_else(|| item.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let arguments = item
            .get("arguments")
            .and_then(|v| v.as_str())
            .unwrap_or("{}")
            .to_string();
        if call_id.trim().is_empty() || name.trim().is_empty() {
            anyhow::bail!("function_call item missing call_id or name");
        }
        return Ok(Some(InternalMessage {
            role: Role::Assistant,
            content: MessageContent::Text(String::new()),
            tool_calls: Some(vec![ToolCall {
                id: call_id,
                name,
                arguments,
            }]),
            tool_call_id: None,
        }));
    }

    if item_type != "message" {
        // Ignore other responses item types (reasoning, file_search_call, etc).
        return Ok(None);
    }

    let role_str = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
    let role = match role_str {
        "system" | "developer" => Role::System,
        "user" => Role::User,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        other => anyhow::bail!("unsupported role in responses input: {other}"),
    };

    let content = match item.get("content") {
        Some(Value::String(text)) => MessageContent::Text(text.clone()),
        Some(Value::Array(blocks)) => {
            let mut texts = Vec::new();
            for block in blocks {
                let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("text");
                match block_type {
                    "input_text" | "output_text" | "text" => {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            texts.push(text.to_string());
                        } else {
                            anyhow::bail!("text block missing 'text' field");
                        }
                    }
                    other => {
                        anyhow::bail!(
                            "unsupported content block type in responses input: {other}"
                        );
                    }
                }
            }
            let text = texts.join("");
            if text.is_empty() {
                return Ok(None);
            }
            MessageContent::Text(text)
        }
        Some(_) => anyhow::bail!("unsupported content type in responses input item"),
        None => return Ok(None),
    };

    Ok(Some(InternalMessage {
        role,
        content,
        tool_calls: None,
        tool_call_id: None,
    }))
}

fn parse_tools(raw_tools: Option<&Value>) -> Result<Option<Vec<ToolDef>>> {
    let Some(Value::Array(items)) = raw_tools else {
        return Ok(None);
    };

    let mut tools = Vec::new();
    for item in items {
        let Some(tool_type) = item
            .get("type")
            .and_then(|v| v.as_str())
        else {
            continue;
        };

        if tool_type != "function" {
            // Responses 内置工具（如 web_search/file_search/code_interpreter）当前不在网关实现范围内，
            // 为保证兼容 Codex 请求，不抛错，直接忽略。
            continue;
        }

        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("function tool missing 'name' field"))?
            .to_string();
        let description = item
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from);
        let parameters = item
            .get("parameters")
            .cloned()
            .unwrap_or(Value::Object(Default::default()));

        tools.push(ToolDef {
            name,
            description,
            parameters,
        });
    }

    if tools.is_empty() {
        Ok(None)
    } else {
        Ok(Some(tools))
    }
}
