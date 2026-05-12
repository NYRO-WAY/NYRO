//! OpenAI Chat Completions ingress decoder (PR-08).
//!
//! Decodes a client `POST /v1/chat/completions` body into `InternalRequest`.
//!
//! Fields added in PR-08 that don't have a first-class slot in the old IR are
//! preserved in `InternalRequest.extra` under their original JSON key names.
//! They will migrate to `AiRequest` proper when the codec is updated to emit
//! `AiRequest` directly (post-PR-06 migration).
//!
//! Zero-cost for fields the client did not send (all `Option`).

use anyhow::Result;
use serde_json::Value;

use crate::protocol::IngressDecoder;
use crate::protocol::ids::OPENAI_CHAT_V1;
use crate::protocol::types::*;

use super::types::*;

pub struct OpenAIDecoder;

impl IngressDecoder for OpenAIDecoder {
    fn decode_request(&self, body: Value) -> Result<InternalRequest> {
        let req: OpenAIRequest = serde_json::from_value(body)?;

        let messages = req
            .messages
            .into_iter()
            .map(decode_message)
            .collect::<Result<Vec<_>>>()?;

        let tools = req.tools.as_ref().map(|tools_val| {
            tools_val
                .iter()
                .filter_map(|t| {
                    let func = t.get("function")?;
                    Some(ToolDef {
                        name: func.get("name")?.as_str()?.to_string(),
                        description: func
                            .get("description")
                            .and_then(|d| d.as_str())
                            .map(String::from),
                        parameters: func
                            .get("parameters")
                            .cloned()
                            .unwrap_or(Value::Object(Default::default())),
                    })
                })
                .collect()
        });

        // ── Extra fields (PR-08) ──────────────────────────────────────────────
        let mut extra = req.extra;

        // Prefer max_completion_tokens (o-models); fall back to max_tokens.
        let effective_max_tokens = req.max_completion_tokens.or(req.max_tokens);

        // Carry PR-08 fields as first-class extras for downstream encoding.
        if let Some(v) = req.stream_options {
            extra
                .entry("stream_options".to_string())
                .or_insert_with(|| serde_json::to_value(v).unwrap_or(Value::Null));
        }
        if let Some(v) = req.parallel_tool_calls {
            extra
                .entry("parallel_tool_calls".to_string())
                .or_insert_with(|| Value::Bool(v));
        }
        if let Some(v) = req.prediction {
            extra
                .entry("prediction".to_string())
                .or_insert_with(|| serde_json::to_value(v).unwrap_or(Value::Null));
        }
        if let Some(v) = req.modalities {
            extra
                .entry("modalities".to_string())
                .or_insert_with(|| Value::Array(v.into_iter().map(Value::String).collect()));
        }
        if let Some(v) = req.audio {
            extra
                .entry("audio".to_string())
                .or_insert_with(|| serde_json::to_value(v).unwrap_or(Value::Null));
        }
        if let Some(v) = req.response_format {
            extra
                .entry("response_format".to_string())
                .or_insert_with(|| serde_json::to_value(v).unwrap_or(Value::Null));
        }
        if let Some(v) = req.seed {
            extra
                .entry("seed".to_string())
                .or_insert_with(|| Value::from(v));
        }
        if let Some(v) = req.stop {
            extra.entry("stop".to_string()).or_insert_with(|| match v {
                StopToken::Single(s) => Value::String(s),
                StopToken::Multiple(vs) => {
                    Value::Array(vs.into_iter().map(Value::String).collect())
                }
            });
        }
        if let Some(v) = req.logit_bias {
            extra.entry("logit_bias".to_string()).or_insert_with(|| {
                Value::Object(v.into_iter().map(|(k, f)| (k, Value::from(f))).collect())
            });
        }
        if let Some(v) = req.service_tier {
            extra
                .entry("service_tier".to_string())
                .or_insert_with(|| Value::String(v));
        }
        if let Some(v) = req.reasoning_effort {
            extra
                .entry("reasoning_effort".to_string())
                .or_insert_with(|| Value::String(v));
        }
        if let Some(v) = req.frequency_penalty {
            extra
                .entry("frequency_penalty".to_string())
                .or_insert_with(|| Value::from(v));
        }
        if let Some(v) = req.presence_penalty {
            extra
                .entry("presence_penalty".to_string())
                .or_insert_with(|| Value::from(v));
        }
        if let Some(v) = req.n {
            extra
                .entry("n".to_string())
                .or_insert_with(|| Value::from(v));
        }
        if let Some(v) = req.user {
            extra
                .entry("user".to_string())
                .or_insert_with(|| Value::String(v));
        }

        Ok(InternalRequest {
            messages,
            model: req.model,
            stream: req.stream,
            temperature: req.temperature,
            max_tokens: effective_max_tokens,
            top_p: req.top_p,
            tools,
            tool_choice: req.tool_choice,
            source_protocol: OPENAI_CHAT_V1,
            extra,
        })
    }
}

fn decode_message(msg: OpenAIMessage) -> Result<InternalMessage> {
    let role = match msg.role.as_str() {
        "system" | "developer" => Role::System,
        "user" => Role::User,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        other => anyhow::bail!("unknown role: {other}"),
    };

    let content = match msg.content {
        Some(OpenAIContent::Text(t)) => MessageContent::Text(t),
        Some(OpenAIContent::Parts(parts)) => {
            let blocks = parts
                .into_iter()
                .map(|p| match p {
                    OpenAIContentPart::Text { text } => ContentBlock::Text { text },
                    OpenAIContentPart::ImageUrl { image_url } => ContentBlock::Image {
                        source: ImageSource {
                            media_type: "image/url".to_string(),
                            data: image_url.url,
                        },
                    },
                    // InputAudio: pass as text; will be handled by audio-aware encoders.
                    OpenAIContentPart::InputAudio { input_audio } => ContentBlock::Text {
                        text: format!(
                            "[audio:{}:{}]",
                            input_audio.format,
                            &input_audio.data[..input_audio.data.len().min(16)]
                        ),
                    },
                })
                .collect();
            MessageContent::Blocks(blocks)
        }
        None => MessageContent::Text(String::new()),
    };

    let tool_calls = msg.tool_calls.map(|tcs| {
        tcs.into_iter()
            .map(|tc| ToolCall {
                id: tc.id,
                name: tc.function.name,
                arguments: tc.function.arguments,
            })
            .collect()
    });

    Ok(InternalMessage {
        role,
        content,
        tool_calls,
        tool_call_id: msg.tool_call_id,
        extra: msg.extra,
    })
}
