//! OpenAI Responses API ingress decoder (PR-09).
//!
//! Added in PR-09:
//! - `background` (bool) — run in background
//! - `previous_response_id` — link to prior response (multi-turn stateful)
//! - Built-in tools: `web_search_preview`, `file_search`, `computer_use_preview`
//! - `store` — whether to store in conversation history
//! - `include` — list of fields to include in the response
//! - `truncation` — input truncation strategy
//! - `metadata` / `text` / `temperature` / `top_p` (pre-existing, now explicit)
//! - Full reasoning item handling (passed through as extra, skip silently)

use std::collections::HashMap;

use anyhow::Result;
use serde_json::Value;

use crate::protocol::IngressDecoder;
use crate::protocol::ids::OPENAI_RESPONSES_V1;
use crate::protocol::types::*;

pub struct ResponsesDecoder;

// Fields decoded into named IR fields (not extra).
const KNOWN_FIELDS: &[&str] = &[
    "model",
    "input",
    "instructions",
    "max_output_tokens",
    "stream",
    "temperature",
    "top_p",
    "tools",
    "tool_choice",
    // PR-09
    "background",
    "previous_response_id",
    "store",
    "include",
    "truncation",
    "metadata",
    "text",
    "reasoning",
    "parallel_tool_calls",
    "service_tier",
    "user",
];

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

        if let Some(inst) = obj.get("instructions").and_then(|v| v.as_str())
            && !inst.is_empty()
        {
            messages.push(InternalMessage {
                role: Role::System,
                content: MessageContent::Text(inst.to_string()),
                tool_calls: None,
                tool_call_id: None,
                extra: HashMap::new(),
            });
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
                    extra: HashMap::new(),
                });
            }
            Value::Array(items) => {
                let mut pending_reasoning: Option<String> = None;
                for item in items {
                    // Preserve reasoning_content from Responses API "reasoning" items.
                    // DeepSeek requires that reasoning_content be passed back on the
                    // assistant message that produced it. Codex represents reasoning as
                    // a separate "reasoning" item in the input array; we attach it to the
                    // next assistant message via the `extra` field.
                    if item
                        .get("type")
                        .and_then(|v| v.as_str())
                        .is_some_and(|t| t == "reasoning")
                    {
                        if let Some(summary) = item.get("summary").and_then(|v| v.as_array()) {
                            for s in summary {
                                if let Some(text) = s.get("text").and_then(|v| v.as_str()) {
                                    if !text.is_empty() {
                                        pending_reasoning = Some(text.to_string());
                                        break;
                                    }
                                }
                            }
                        }
                        continue;
                    }

                    match decode_input_item(item)? {
                        Some(mut msg) => {
                            if msg.role == Role::Assistant {
                                // Clone reasoning to EACH consecutive assistant message.
                                // Parallel function_calls produce multiple assistant messages
                                // that all belong to the same reasoning turn; each must carry
                                // reasoning_content. Tool results interleaved between
                                // function_calls are NOT turn boundaries — keep reasoning alive.
                                if let Some(ref reasoning) = pending_reasoning {
                                    msg.extra.insert(
                                        "reasoning_content".to_string(),
                                        Value::String(reasoning.clone()),
                                    );
                                }
                            } else if msg.role == Role::User || msg.role == Role::System {
                                // User/System messages are true conversation turn boundaries.
                                // If pending reasoning was never attached to an assistant
                                // message, emit it as a standalone assistant now.
                                if let Some(reasoning) = pending_reasoning.take() {
                                    let mut extra = HashMap::new();
                                    extra.insert(
                                        "reasoning_content".to_string(),
                                        Value::String(reasoning),
                                    );
                                    messages.push(InternalMessage {
                                        role: Role::Assistant,
                                        content: MessageContent::Text(String::new()),
                                        tool_calls: None,
                                        tool_call_id: None,
                                        extra,
                                    });
                                }
                            }
                            // Tool outputs and other non-assistant types: leave
                            // pending_reasoning intact — they sit between calls of the
                            // same reasoning turn and do not end the turn.
                            messages.push(msg);
                        }
                        None => {
                            // If the item produced no message (e.g., an ignored type)
                            // but we have pending reasoning, keep it for the next one.
                        }
                    }
                }
                // Flush any remaining pending reasoning at end of input
                if let Some(reasoning) = pending_reasoning.take() {
                    let mut extra = HashMap::new();
                    extra.insert("reasoning_content".to_string(), Value::String(reasoning));
                    messages.push(InternalMessage {
                        role: Role::Assistant,
                        content: MessageContent::Text(String::new()),
                        tool_calls: None,
                        tool_call_id: None,
                        extra,
                    });
                }
            }
            _ => anyhow::bail!("'input' must be a string or array"),
        }

        if messages.is_empty() {
            anyhow::bail!("no messages found in input");
        }

        // ── Build extra from unknown + PR-09 named fields ─────────────────────
        let mut extra: HashMap<String, Value> = obj
            .iter()
            .filter(|(k, _)| !KNOWN_FIELDS.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        // Carry PR-09 named fields explicitly so the encoder can forward them.
        for key in &[
            "background",
            "previous_response_id",
            "store",
            "include",
            "truncation",
            "metadata",
            "text",
            "reasoning",
            "parallel_tool_calls",
            "service_tier",
            "user",
        ] {
            if let Some(v) = obj.get(*key) {
                extra.entry(key.to_string()).or_insert_with(|| v.clone());
            }
        }

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

    match item_type {
        "function_call_output" => {
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
            Ok(Some(InternalMessage {
                role: Role::Tool,
                content: MessageContent::Text(output_text),
                tool_calls: None,
                tool_call_id: Some(call_id),
                extra: HashMap::new(),
            }))
        }

        "function_call" => {
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
            Ok(Some(InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text(String::new()),
                tool_calls: Some(vec![ToolCall {
                    id: call_id,
                    name,
                    arguments,
                }]),
                tool_call_id: None,
                extra: HashMap::new(),
            }))
        }

        // PR-09: web_search_call, file_search_call, computer_call — pass through
        // silently; their results are provided via `function_call_output`.
        "web_search_call" | "file_search_call" | "computer_call" | "reasoning" => Ok(None),

        "message" => decode_message_item(item),

        _ => Ok(None),
    }
}

fn decode_message_item(item: &Value) -> Result<Option<InternalMessage>> {
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
                        }
                    }
                    "image_url" => {
                        // Skip images in text accumulator; they'll be handled by multimodal codecs.
                    }
                    _ => {
                        // Ignore unknown block types (e.g. `input_audio`, `reasoning_summary`).
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
        extra: HashMap::new(),
    }))
}

fn parse_tools(raw_tools: Option<&Value>) -> Result<Option<Vec<ToolDef>>> {
    let Some(Value::Array(items)) = raw_tools else {
        return Ok(None);
    };

    let mut tools = Vec::new();
    for item in items {
        let tool_type = item
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("function");

        match tool_type {
            "function" => {
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
            // PR-09: built-in tools preserved as sentinel ToolDef entries
            // so the encoder can reconstruct them on the egress side.
            "web_search_preview" | "file_search" | "computer_use_preview" | "code_interpreter" => {
                tools.push(ToolDef {
                    name: format!("__builtin__{}", tool_type),
                    description: Some(format!("built-in tool: {}", tool_type)),
                    parameters: item.clone(),
                });
            }
            _ => {
                // Ignore unknown tool types.
            }
        }
    }

    if tools.is_empty() {
        Ok(None)
    } else {
        Ok(Some(tools))
    }
}
