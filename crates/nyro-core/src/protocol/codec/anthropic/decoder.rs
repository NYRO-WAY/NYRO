use anyhow::Result;
use serde_json::Value;

use crate::protocol::IngressDecoder;
use crate::protocol::ids::ANTHROPIC_MESSAGES_2023_06_01;
use crate::protocol::types::*;

use super::types::*;

pub struct AnthropicDecoder;

impl IngressDecoder for AnthropicDecoder {
    fn decode_request(&self, body: Value) -> Result<InternalRequest> {
        let req: AnthropicRequest = serde_json::from_value(body)?;

        // ── Cache-control / exotic-block detection ────────────────────────────
        // If any message contains cache_control or non-standard blocks
        // (Document, InputAudio), preserve the original wire-format so the
        // encoder can round-trip them faithfully.
        let needs_raw_msgs = req.messages.iter().any(|m| {
            if let AnthropicContent::Blocks(blocks) = &m.content {
                blocks
                    .iter()
                    .any(|b| b.cache_control().is_some() || b.is_exotic())
            } else {
                false
            }
        });

        let needs_raw_system = match &req.system {
            Some(AnthropicSystem::Blocks(blocks)) => {
                blocks.iter().any(|b| b.cache_control.is_some())
            }
            _ => false,
        };

        let needs_raw_tools = req
            .tools
            .as_ref()
            .map(|ts| ts.iter().any(|t| t.cache_control.is_some()))
            .unwrap_or(false);

        // Snapshot raw anthropic wire values before consuming req.
        let raw_messages: Option<Value> = if needs_raw_msgs {
            serde_json::to_value(&req.messages).ok()
        } else {
            None
        };
        let raw_system: Option<Value> = if needs_raw_system {
            req.system
                .as_ref()
                .and_then(|s| serde_json::to_value(s).ok())
        } else {
            None
        };
        let raw_tools: Option<Value> = if needs_raw_tools {
            req.tools
                .as_ref()
                .and_then(|t| serde_json::to_value(t).ok())
        } else {
            None
        };

        // ── Decode messages ───────────────────────────────────────────────────
        let mut messages = Vec::new();

        if let Some(system) = &req.system {
            let text = match system {
                AnthropicSystem::Text(t) => t.clone(),
                AnthropicSystem::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|b| {
                        if b.kind == "text" {
                            Some(b.text.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            };
            if !text.is_empty() {
                messages.push(InternalMessage {
                    role: Role::System,
                    content: MessageContent::Text(text),
                    tool_calls: None,
                    tool_call_id: None,
                    extra: Default::default(),
                });
            }
        }

        for msg in req.messages {
            messages.extend(decode_message(msg)?);
        }

        // ── Decode tools ──────────────────────────────────────────────────────
        let tools = req.tools.map(|tools| {
            tools
                .into_iter()
                .filter_map(|t| {
                    // Built-in tools (computer_use, text_editor, bash) have no
                    // input_schema; represent them with a sentinel name so the
                    // encoder can reconstruct them.
                    if let Some(tool_type) = &t.tool_type {
                        return Some(ToolDef {
                            name: format!("__builtin__{}", tool_type),
                            description: t.description,
                            parameters: t.input_schema.unwrap_or(Value::Object(Default::default())),
                        });
                    }
                    t.input_schema.map(|schema| ToolDef {
                        name: t.name,
                        description: t.description,
                        parameters: schema,
                    })
                })
                .collect()
        });

        // ── Build extra map ───────────────────────────────────────────────────
        let mut extra: std::collections::HashMap<String, Value> = Default::default();

        if let Some(v) = raw_messages {
            extra.insert("__anthropic_raw_messages".into(), v);
        }
        if let Some(v) = raw_system {
            extra.insert("__anthropic_raw_system".into(), v);
        }
        if let Some(v) = raw_tools {
            extra.insert("__anthropic_raw_tools".into(), v);
        }

        // PR-10 named extra fields ─────────────────────────────────────────────
        if let Some(thinking) = req.thinking
            && let Ok(v) = serde_json::to_value(&thinking)
        {
            extra.insert("__anthropic_thinking".into(), v);
        }
        if let Some(cm) = req.context_management
            && let Ok(v) = serde_json::to_value(&cm)
        {
            extra.insert("__anthropic_context_management".into(), v);
        }
        if let Some(c) = req.container {
            extra.insert("__anthropic_container".into(), Value::String(c));
        }
        if let Some(st) = req.service_tier {
            extra.insert("__anthropic_service_tier".into(), Value::String(st));
        }
        if let Some(meta) = req.metadata {
            extra.insert("__anthropic_metadata".into(), meta);
        }
        if let Some(stops) = req.stop_sequences
            && let Ok(v) = serde_json::to_value(&stops)
        {
            extra.insert("__anthropic_stop_sequences".into(), v);
        }
        if let Some(k) = req.top_k {
            extra.insert("__anthropic_top_k".into(), Value::Number(k.into()));
        }

        Ok(InternalRequest {
            messages,
            model: req.model,
            stream: req.stream,
            temperature: req.temperature,
            max_tokens: Some(req.max_tokens),
            top_p: req.top_p,
            tools,
            tool_choice: req.tool_choice,
            source_protocol: ANTHROPIC_MESSAGES_2023_06_01,
            extra,
        })
    }
}

// ── Message decoding helpers ──────────────────────────────────────────────────

fn decode_message(msg: AnthropicMessage) -> Result<Vec<InternalMessage>> {
    let role = match msg.role.as_str() {
        "user" => Role::User,
        "assistant" => Role::Assistant,
        other => anyhow::bail!("unknown Anthropic role: {other}"),
    };

    if role == Role::User
        && let AnthropicContent::Blocks(blocks) = msg.content
    {
        return decode_user_blocks(blocks);
    }

    let (content, tool_calls, tool_call_id) = match msg.content {
        AnthropicContent::Text(t) => (MessageContent::Text(t), None, None),
        AnthropicContent::Blocks(blocks) => {
            let mut content_blocks = Vec::new();
            let mut tcs = Vec::new();
            let mut tc_id = None;

            for block in blocks {
                match block {
                    AnthropicContentBlock::Text { text, .. } => {
                        content_blocks.push(ContentBlock::Text { text });
                    }
                    AnthropicContentBlock::Thinking {
                        thinking,
                        signature,
                        ..
                    } => {
                        content_blocks.push(ContentBlock::Reasoning {
                            text: thinking,
                            signature,
                        });
                    }
                    AnthropicContentBlock::Image { source, .. } => {
                        content_blocks.push(ContentBlock::Image {
                            source: ImageSource {
                                media_type: source.media_type.unwrap_or_default(),
                                data: source.data.unwrap_or_default(),
                            },
                        });
                    }
                    AnthropicContentBlock::ToolUse {
                        id, name, input, ..
                    } => {
                        tcs.push(ToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: input.to_string(),
                        });
                        content_blocks.push(ContentBlock::ToolUse { id, name, input });
                    }
                    AnthropicContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => {
                        tc_id = Some(tool_use_id.clone());
                        content_blocks.push(ContentBlock::ToolResult {
                            tool_use_id,
                            content: content.unwrap_or(Value::Null),
                        });
                    }
                    AnthropicContentBlock::Document { title, .. } => {
                        // Best-effort IR representation; encoder uses raw bytes.
                        let text = title.unwrap_or_else(|| "[document]".to_string());
                        content_blocks.push(ContentBlock::Text { text });
                    }
                    AnthropicContentBlock::InputAudio { .. } => {
                        content_blocks.push(ContentBlock::Text {
                            text: "[audio]".to_string(),
                        });
                    }
                }
            }

            let tool_calls_opt = if tcs.is_empty() { None } else { Some(tcs) };

            if content_blocks.len() == 1
                && let ContentBlock::Text { text } = &content_blocks[0]
            {
                return Ok(vec![InternalMessage {
                    role,
                    content: MessageContent::Text(text.clone()),
                    tool_calls: tool_calls_opt,
                    tool_call_id: tc_id,
                    extra: Default::default(),
                }]);
            }

            (
                MessageContent::Blocks(content_blocks),
                tool_calls_opt,
                tc_id,
            )
        }
    };

    Ok(vec![InternalMessage {
        role,
        content,
        tool_calls,
        tool_call_id,
        extra: Default::default(),
    }])
}

fn decode_user_blocks(blocks: Vec<AnthropicContentBlock>) -> Result<Vec<InternalMessage>> {
    let mut messages: Vec<InternalMessage> = Vec::new();
    let mut user_blocks: Vec<ContentBlock> = Vec::new();

    for block in blocks {
        match block {
            AnthropicContentBlock::ToolResult {
                tool_use_id,
                content,
                ..
            } => {
                let tool_text = match content.unwrap_or(Value::Null) {
                    Value::String(s) => s,
                    Value::Null => String::new(),
                    other => other.to_string(),
                };
                messages.push(InternalMessage {
                    role: Role::Tool,
                    content: MessageContent::Text(tool_text),
                    tool_calls: None,
                    tool_call_id: Some(tool_use_id),
                    extra: Default::default(),
                });
            }
            AnthropicContentBlock::Text { text, .. } => {
                user_blocks.push(ContentBlock::Text { text })
            }
            AnthropicContentBlock::Thinking {
                thinking,
                signature,
                ..
            } => user_blocks.push(ContentBlock::Reasoning {
                text: thinking,
                signature,
            }),
            AnthropicContentBlock::Image { source, .. } => user_blocks.push(ContentBlock::Image {
                source: ImageSource {
                    media_type: source.media_type.unwrap_or_default(),
                    data: source.data.unwrap_or_default(),
                },
            }),
            AnthropicContentBlock::ToolUse {
                id, name, input, ..
            } => user_blocks.push(ContentBlock::ToolUse { id, name, input }),
            AnthropicContentBlock::Document { title, .. } => user_blocks.push(ContentBlock::Text {
                text: title.unwrap_or_else(|| "[document]".to_string()),
            }),
            AnthropicContentBlock::InputAudio { .. } => user_blocks.push(ContentBlock::Text {
                text: "[audio]".to_string(),
            }),
        }
    }

    if !user_blocks.is_empty() {
        let content = if user_blocks.len() == 1 {
            if let ContentBlock::Text { text } = &user_blocks[0] {
                MessageContent::Text(text.clone())
            } else {
                MessageContent::Blocks(user_blocks)
            }
        } else {
            MessageContent::Blocks(user_blocks)
        };
        messages.insert(
            0,
            InternalMessage {
                role: Role::User,
                content,
                tool_calls: None,
                tool_call_id: None,
                extra: Default::default(),
            },
        );
    }

    Ok(messages)
}
