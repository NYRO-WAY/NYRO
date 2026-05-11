use anyhow::Result;
use serde_json::Value;

use crate::protocol::IngressDecoder;
use crate::protocol::ids::GOOGLE_GENERATE_V1BETA;
use crate::protocol::types::*;

use super::types::*;

pub struct GoogleDecoder;

impl GoogleDecoder {
    pub fn decode_with_model(
        &self,
        body: Value,
        model: &str,
        stream: bool,
    ) -> Result<InternalRequest> {
        let req: GoogleRequest = serde_json::from_value(body)?;

        // ── System instruction ────────────────────────────────────────────────
        // If the system instruction has non-text parts, preserve raw value.
        let needs_raw_system = req.system_instruction.as_ref().is_some_and(|si| {
            si.parts.len() > 1
                || si
                    .parts
                    .iter()
                    .any(|p| !matches!(p, GooglePart::Text { .. }))
        });

        let raw_system: Option<Value> = if needs_raw_system {
            req.system_instruction
                .as_ref()
                .and_then(|s| serde_json::to_value(s).ok())
        } else {
            None
        };

        // ── Tools: detect built-ins ───────────────────────────────────────────
        let has_builtin_tools = req.tools.as_ref().is_some_and(|ts| {
            ts.iter().any(|t| {
                t.google_search.is_some()
                    || t.code_execution.is_some()
                    || t.google_search_retrieval.is_some()
            })
        });

        let raw_tools: Option<Value> = if has_builtin_tools {
            req.tools
                .as_ref()
                .and_then(|t| serde_json::to_value(t).ok())
        } else {
            None
        };

        let mut messages = Vec::new();

        // System message from system_instruction
        if let Some(si) = &req.system_instruction {
            let text: String = si
                .parts
                .iter()
                .filter_map(|p| match p {
                    GooglePart::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
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

        for content in req.contents {
            messages.push(decode_content(content)?);
        }

        // ── Tools: function declarations + sentinel built-ins ─────────────────
        let tools = req.tools.as_ref().map(|entries| {
            let mut defs: Vec<ToolDef> = Vec::new();
            for entry in entries {
                if let Some(decls) = &entry.function_declarations {
                    for fd in decls {
                        defs.push(ToolDef {
                            name: fd.name.clone(),
                            description: fd.description.clone(),
                            parameters: fd
                                .parameters
                                .clone()
                                .unwrap_or(Value::Object(Default::default())),
                        });
                    }
                }
                if entry.google_search.is_some() {
                    defs.push(ToolDef {
                        name: "__builtin__google_search".into(),
                        description: None,
                        parameters: Value::Object(Default::default()),
                    });
                }
                if entry.code_execution.is_some() {
                    defs.push(ToolDef {
                        name: "__builtin__code_execution".into(),
                        description: None,
                        parameters: Value::Object(Default::default()),
                    });
                }
                if entry.google_search_retrieval.is_some() {
                    defs.push(ToolDef {
                        name: "__builtin__google_search_retrieval".into(),
                        description: None,
                        parameters: Value::Object(Default::default()),
                    });
                }
            }
            defs
        });

        // ── generationConfig → InternalRequest fields + extra ─────────────────
        let gc = req.generation_config.as_ref();
        let max_tokens = gc.and_then(|c| c.max_output_tokens);
        let temperature = gc.and_then(|c| c.temperature);
        let top_p = gc.and_then(|c| c.top_p);

        let mut extra: std::collections::HashMap<String, Value> = Default::default();

        // Preserve full generationConfig so encoder can re-emit extended fields.
        if let Some(gen_cfg) = &req.generation_config
            && let Ok(v) = serde_json::to_value(gen_cfg)
        {
            extra.insert("__google_generation_config".into(), v);
        }

        if let Some(v) = raw_system {
            extra.insert("__google_raw_system_instruction".into(), v);
        }
        if let Some(v) = raw_tools {
            extra.insert("__google_raw_tools".into(), v);
        }
        if let Some(ref ss) = req.safety_settings
            && let Ok(v) = serde_json::to_value(ss)
        {
            extra.insert("__google_safety_settings".into(), v);
        }
        if let Some(ref tc) = req.tool_config {
            extra.insert("__google_tool_config".into(), tc.clone());
        }
        if let Some(ref cc) = req.cached_content {
            extra.insert("__google_cached_content".into(), Value::String(cc.clone()));
        }

        Ok(InternalRequest {
            messages,
            model: model.to_string(),
            stream,
            temperature,
            max_tokens,
            top_p,
            tools,
            tool_choice: None,
            source_protocol: GOOGLE_GENERATE_V1BETA,
            extra,
        })
    }
}

impl IngressDecoder for GoogleDecoder {
    fn decode_request(&self, body: Value) -> Result<InternalRequest> {
        self.decode_with_model(body, "gemini-2.0-flash", false)
    }
}

// ── Content decoding ──────────────────────────────────────────────────────────

fn decode_content(content: GoogleContent) -> Result<InternalMessage> {
    let mut role = match content.role.as_deref() {
        Some("user") | None => Role::User,
        Some("model") => Role::Assistant,
        Some(other) => anyhow::bail!("unknown Gemini role: {other}"),
    };

    let mut text_parts: Vec<String> = Vec::new();
    let mut blocks: Vec<ContentBlock> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut has_function_response = false;

    for part in content.parts {
        match part {
            GooglePart::Text { text } => {
                text_parts.push(text.clone());
                blocks.push(ContentBlock::Text { text });
            }
            GooglePart::InlineData { inline_data } => {
                blocks.push(ContentBlock::Image {
                    source: ImageSource {
                        media_type: inline_data.mime_type,
                        data: inline_data.data,
                    },
                });
            }
            GooglePart::FileData { file_data } => {
                // Represent file references as Image with URI as data.
                blocks.push(ContentBlock::Image {
                    source: ImageSource {
                        media_type: file_data.mime_type.unwrap_or_default(),
                        data: file_data.file_uri,
                    },
                });
            }
            GooglePart::FunctionCall { function_call } => {
                let id = format!("call_{}", uuid::Uuid::new_v4().simple());
                tool_calls.push(ToolCall {
                    id: id.clone(),
                    name: function_call.name.clone(),
                    arguments: function_call.args.to_string(),
                });
                blocks.push(ContentBlock::ToolUse {
                    id,
                    name: function_call.name,
                    input: function_call.args,
                });
            }
            GooglePart::FunctionResponse { function_response } => {
                has_function_response = true;
                blocks.push(ContentBlock::ToolResult {
                    tool_use_id: function_response.name,
                    content: function_response.response,
                });
            }
            GooglePart::ExecutableCode { executable_code } => {
                blocks.push(ContentBlock::Text {
                    text: executable_code.code,
                });
            }
            GooglePart::CodeExecutionResult {
                code_execution_result,
            } => {
                let output = code_execution_result.output.unwrap_or_default();
                blocks.push(ContentBlock::Text { text: output });
            }
            GooglePart::Other(v) => {
                // Thought parts or future types: surface text if available.
                if let Some(text) = v.get("text").and_then(|t| t.as_str()) {
                    text_parts.push(text.to_string());
                    blocks.push(ContentBlock::Text {
                        text: text.to_string(),
                    });
                }
            }
        }
    }

    let msg_content = if blocks.len() == 1 && text_parts.len() == 1 {
        MessageContent::Text(text_parts.into_iter().next().unwrap())
    } else {
        MessageContent::Blocks(blocks)
    };

    let tool_calls_opt = if tool_calls.is_empty() {
        None
    } else {
        Some(tool_calls)
    };
    if has_function_response {
        role = Role::Tool;
    }

    Ok(InternalMessage {
        role,
        content: msg_content,
        tool_calls: tool_calls_opt,
        tool_call_id: None,
        extra: Default::default(),
    })
}
