use anyhow::Result;
use serde_json::Value;

use crate::protocol::IngressDecoder;
use crate::protocol::ids::GOOGLE_GENERATE_V1BETA;
use crate::protocol::types::*;

use super::types::*;

pub struct GoogleDecoder;

impl GoogleDecoder {
    pub fn decode_with_model(&self, body: Value, model: &str, stream: bool) -> Result<InternalRequest> {
        let req: GoogleRequest = serde_json::from_value(body)?;

        let mut messages = Vec::new();

        if let Some(sys) = &req.system_instruction {
            let text = sys
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
                });
            }
        }

        for content in &req.contents {
            messages.push(decode_content(content)?);
        }

        let tools = req.tools.as_ref().map(|tools| {
            tools
                .iter()
                .flat_map(|t| {
                    t.function_declarations.iter().map(|fd| ToolDef {
                        name: fd.name.clone(),
                        description: fd.description.clone(),
                        parameters: fd.parameters.clone().unwrap_or(Value::Object(Default::default())),
                    })
                })
                .collect()
        });

        let max_tokens = req
            .generation_config
            .as_ref()
            .and_then(|c| c.max_output_tokens);
        let temperature = req.generation_config.as_ref().and_then(|c| c.temperature);
        let top_p = req.generation_config.as_ref().and_then(|c| c.top_p);

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
            extra: Default::default(),
        })
    }
}

impl IngressDecoder for GoogleDecoder {
    fn decode_request(&self, body: Value) -> Result<InternalRequest> {
        self.decode_with_model(body, "gemini-2.0-flash", false)
    }
}

fn decode_content(content: &GoogleContent) -> Result<InternalMessage> {
    let mut role = match content.role.as_deref() {
        Some("user") | None => Role::User,
        Some("model") => Role::Assistant,
        Some(other) => anyhow::bail!("unknown Gemini role: {other}"),
    };

    let mut text_parts = Vec::new();
    let mut blocks = Vec::new();
    let mut tool_calls = Vec::new();
    let mut has_function_response = false;

    for part in &content.parts {
        match part {
            GooglePart::Text { text } => {
                text_parts.push(text.clone());
                blocks.push(ContentBlock::Text { text: text.clone() });
            }
            GooglePart::InlineData { inline_data } => {
                blocks.push(ContentBlock::Image {
                    source: ImageSource {
                        media_type: inline_data.mime_type.clone(),
                        data: inline_data.data.clone(),
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
                    name: function_call.name.clone(),
                    input: function_call.args.clone(),
                });
            }
            GooglePart::FunctionResponse { function_response } => {
                has_function_response = true;
                blocks.push(ContentBlock::ToolResult {
                    tool_use_id: function_response.name.clone(),
                    content: function_response.response.clone(),
                });
            }
        }
    }

    let content = if blocks.len() == 1 && text_parts.len() == 1 {
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
        content,
        tool_calls: tool_calls_opt,
        tool_call_id: None,
    })
}
