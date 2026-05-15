//! Compatibility shims between the old `InternalRequest`/`InternalResponse` and
//! the new `AiRequest`/`AiResponse`.
//!
//! These `From` implementations allow progressive migration: codec decoders can
//! be updated one by one from producing `InternalRequest` to producing
//! `AiRequest`, while the dispatcher still accepts both via the shim.
//!
//! Round-trip property: `InternalRequest → AiRequest → InternalRequest` is
//! lossless for all fields present in `InternalRequest`.

use crate::protocol::ir::request::{
    AiRequest, ContentBlock, GenerationConfig, MediaSource, Message, MessageContent, Role,
    StreamConfig, ToolCall, ToolChoice, ToolSpec,
};
use crate::protocol::ir::response::AiResponse;
use crate::protocol::types::{
    ContentBlock as OldContentBlock, ImageSource, InternalMessage, InternalRequest,
    InternalResponse, MessageContent as OldMessageContent, Role as OldRole,
    ToolCall as OldToolCall, ToolDef,
};
use serde_json::Value;

// ── InternalRequest → AiRequest ───────────────────────────────────────────────

impl From<InternalRequest> for AiRequest {
    fn from(old: InternalRequest) -> Self {
        let messages = old.messages.into_iter().map(msg_from_old).collect();
        let tools = old
            .tools
            .map(|ts| ts.into_iter().map(tool_spec_from_old).collect());
        let mut req = AiRequest::new(old.model, messages);
        req.generation = GenerationConfig {
            temperature: old.temperature,
            max_tokens: old.max_tokens,
            top_p: old.top_p,
            ..Default::default()
        };
        req.stream = StreamConfig {
            enabled: old.stream,
            include_usage: false,
        };
        req.tools = tools;
        req.tool_choice = old
            .tool_choice
            .map(crate::protocol::ir::request::ToolChoice::Raw);
        req.meta.source_protocol = Some(old.source_protocol);
        // Copy unknown extra fields into the ingress vendor bag.
        for (k, v) in old.extra {
            req.meta.vendor.ingress.insert(k, v);
        }
        req
    }
}

fn msg_from_old(old: InternalMessage) -> Message {
    let meta = if old.extra.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(old.extra.into_iter().collect()))
    };
    Message {
        role: role_from_old(old.role),
        content: content_from_old(old.content),
        tool_calls: old
            .tool_calls
            .map(|tcs| tcs.into_iter().map(tc_from_old).collect()),
        tool_call_id: old.tool_call_id,
        meta,
    }
}

fn role_from_old(r: OldRole) -> Role {
    match r {
        OldRole::System => Role::System,
        OldRole::User => Role::User,
        OldRole::Assistant => Role::Assistant,
        OldRole::Tool => Role::Tool,
    }
}

fn content_from_old(c: OldMessageContent) -> MessageContent {
    match c {
        OldMessageContent::Text(t) => MessageContent::Text(t),
        OldMessageContent::Blocks(bs) => {
            MessageContent::Blocks(bs.into_iter().map(block_from_old).collect())
        }
    }
}

fn block_from_old(b: OldContentBlock) -> ContentBlock {
    match b {
        OldContentBlock::Text { text } => ContentBlock::Text {
            text,
            cache_control: None,
        },
        OldContentBlock::Image { source } => ContentBlock::Image {
            source: MediaSource::Base64 {
                media_type: source.media_type,
                data: source.data,
            },
            cache_control: None,
        },
        OldContentBlock::Reasoning { text, signature } => ContentBlock::Thinking {
            thinking: text,
            signature,
        },
        OldContentBlock::ToolUse { id, name, input } => ContentBlock::ToolUse {
            id,
            name,
            input,
            cache_control: None,
        },
        OldContentBlock::ToolResult {
            tool_use_id,
            content,
        } => ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error: None,
            cache_control: None,
        },
    }
}

fn tc_from_old(tc: OldToolCall) -> ToolCall {
    ToolCall {
        id: tc.id,
        name: tc.name,
        arguments: tc.arguments,
    }
}

fn tool_spec_from_old(td: ToolDef) -> ToolSpec {
    ToolSpec {
        name: td.name,
        description: td.description,
        parameters: td.parameters,
        strict: None,
        cache_control: None,
        meta: None,
    }
}

// ── AiRequest → InternalRequest ───────────────────────────────────────────────

impl From<AiRequest> for InternalRequest {
    fn from(new: AiRequest) -> Self {
        let messages = new.messages.into_iter().map(msg_to_old).collect();
        let tools = new
            .tools
            .map(|ts: Vec<_>| ts.into_iter().map(tool_spec_to_old).collect());
        let source_protocol = new
            .meta
            .source_protocol
            .unwrap_or(crate::protocol::ids::OPENAI_CHAT_COMPLETIONS_V1);
        let mut extra = std::collections::HashMap::new();
        for (k, v) in new.meta.vendor.ingress {
            extra.insert(k, v);
        }
        InternalRequest {
            messages,
            model: new.model,
            stream: new.stream.enabled,
            temperature: new.generation.temperature,
            max_tokens: new.generation.max_tokens,
            top_p: new.generation.top_p,
            tools,
            tool_choice: new.tool_choice.map(|tc| match tc {
                crate::protocol::ir::request::ToolChoice::Raw(v) => v,
                _ => serde_json::to_value(&tc).unwrap_or(serde_json::Value::Null),
            }),
            source_protocol,
            extra,
        }
    }
}

fn msg_to_old(msg: Message) -> InternalMessage {
    let extra = match msg.meta {
        Some(serde_json::Value::Object(obj)) => obj.into_iter().collect(),
        _ => Default::default(),
    };
    InternalMessage {
        role: role_to_old(msg.role),
        content: content_to_old(msg.content),
        tool_calls: msg
            .tool_calls
            .map(|tcs| tcs.into_iter().map(tc_to_old).collect()),
        tool_call_id: msg.tool_call_id,
        extra,
    }
}

fn role_to_old(r: Role) -> OldRole {
    match r {
        Role::System => OldRole::System,
        Role::User => OldRole::User,
        Role::Assistant => OldRole::Assistant,
        Role::Tool => OldRole::Tool,
    }
}

fn content_to_old(c: MessageContent) -> OldMessageContent {
    match c {
        MessageContent::Text(t) => OldMessageContent::Text(t),
        MessageContent::Blocks(bs) => {
            OldMessageContent::Blocks(bs.into_iter().filter_map(block_to_old).collect())
        }
    }
}

fn block_to_old(b: ContentBlock) -> Option<OldContentBlock> {
    match b {
        ContentBlock::Text { text, .. } => Some(OldContentBlock::Text { text }),
        ContentBlock::Image { source, .. } => match source {
            MediaSource::Base64 { media_type, data } => Some(OldContentBlock::Image {
                source: crate::protocol::types::ImageSource { media_type, data },
            }),
            MediaSource::Url(url) => Some(OldContentBlock::Image {
                source: crate::protocol::types::ImageSource {
                    media_type: "image/url".to_string(),
                    data: url,
                },
            }),
            MediaSource::FileId { file_id, .. } => Some(OldContentBlock::Image {
                source: crate::protocol::types::ImageSource {
                    media_type: "image/file_id".to_string(),
                    data: file_id,
                },
            }),
        },
        ContentBlock::Thinking {
            thinking,
            signature,
        } => Some(OldContentBlock::Reasoning {
            text: thinking,
            signature,
        }),
        ContentBlock::ToolUse {
            id, name, input, ..
        } => Some(OldContentBlock::ToolUse { id, name, input }),
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            ..
        } => Some(OldContentBlock::ToolResult {
            tool_use_id,
            content,
        }),
        // New variants not representable in old IR are dropped.
        _ => None,
    }
}

fn tc_to_old(tc: ToolCall) -> OldToolCall {
    OldToolCall {
        id: tc.id,
        name: tc.name,
        arguments: tc.arguments,
    }
}

fn tool_spec_to_old(ts: ToolSpec) -> ToolDef {
    ToolDef {
        name: ts.name,
        description: ts.description,
        parameters: ts.parameters,
    }
}

// ── InternalResponse → AiResponse ────────────────────────────────────────────

impl From<InternalResponse> for AiResponse {
    fn from(old: InternalResponse) -> Self {
        let mut resp = AiResponse::new(old.id, old.model);
        resp.content = old.content;
        resp.reasoning_content = old.reasoning_content;
        resp.reasoning_signature = old.reasoning_signature;
        resp.tool_calls = old
            .tool_calls
            .into_iter()
            .map(|tc| ToolCall {
                id: tc.id,
                name: tc.name,
                arguments: tc.arguments,
            })
            .collect();
        resp.stop_reason = old.stop_reason;
        resp.usage = old.usage;
        resp
    }
}

// ── AiResponse → InternalResponse ────────────────────────────────────────────

impl From<AiResponse> for InternalResponse {
    fn from(new: AiResponse) -> Self {
        let tool_calls = new
            .tool_calls
            .into_iter()
            .map(|tc| crate::protocol::types::ToolCall {
                id: tc.id,
                name: tc.name,
                arguments: tc.arguments,
            })
            .collect();
        let response_items = new.items.map(|items| {
            items
                .into_iter()
                .filter_map(|item| match item {
                    crate::protocol::ir::response::ResponseItem::OutputText { text } => {
                        Some(crate::protocol::types::ResponseItem::Message { text })
                    }
                    crate::protocol::ir::response::ResponseItem::Thinking { text } => {
                        Some(crate::protocol::types::ResponseItem::Reasoning { text })
                    }
                    crate::protocol::ir::response::ResponseItem::FunctionCall {
                        call_id,
                        name,
                        arguments,
                    } => Some(crate::protocol::types::ResponseItem::FunctionCall {
                        call_id,
                        name,
                        arguments,
                    }),
                    _ => None,
                })
                .collect()
        });
        InternalResponse {
            id: new.id,
            model: new.model,
            content: new.content,
            reasoning_content: new.reasoning_content,
            reasoning_signature: new.reasoning_signature,
            tool_calls,
            response_items,
            stop_reason: new.stop_reason,
            usage: new.usage,
        }
    }
}

// ── By-ref helpers used by encoders after PR-3 ───────────────────────────────

/// Convert an IR `Message` to the old `InternalMessage` format (borrows; clones fields).
pub(crate) fn ai_msg_to_old_ref(msg: &Message) -> InternalMessage {
    let extra = match &msg.meta {
        Some(Value::Object(obj)) => obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        _ => Default::default(),
    };
    InternalMessage {
        role: role_to_old(msg.role),
        content: content_to_old_ref(&msg.content),
        tool_calls: msg.tool_calls.as_ref().map(|tcs| {
            tcs.iter()
                .map(|tc| OldToolCall {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                })
                .collect()
        }),
        tool_call_id: msg.tool_call_id.clone(),
        extra,
    }
}

fn content_to_old_ref(c: &MessageContent) -> OldMessageContent {
    match c {
        MessageContent::Text(t) => OldMessageContent::Text(t.clone()),
        MessageContent::Blocks(bs) => {
            OldMessageContent::Blocks(bs.iter().filter_map(block_to_old_ref).collect())
        }
    }
}

fn block_to_old_ref(b: &ContentBlock) -> Option<OldContentBlock> {
    match b {
        ContentBlock::Text { text, .. } => Some(OldContentBlock::Text { text: text.clone() }),
        ContentBlock::Image { source, .. } => match source {
            MediaSource::Base64 { media_type, data } => Some(OldContentBlock::Image {
                source: ImageSource {
                    media_type: media_type.clone(),
                    data: data.clone(),
                },
            }),
            MediaSource::Url(url) => Some(OldContentBlock::Image {
                source: ImageSource {
                    media_type: "image/url".to_string(),
                    data: url.clone(),
                },
            }),
            MediaSource::FileId { file_id, .. } => Some(OldContentBlock::Image {
                source: ImageSource {
                    media_type: "image/file_id".to_string(),
                    data: file_id.clone(),
                },
            }),
        },
        ContentBlock::Thinking {
            thinking,
            signature,
        } => Some(OldContentBlock::Reasoning {
            text: thinking.clone(),
            signature: signature.clone(),
        }),
        ContentBlock::ToolUse {
            id, name, input, ..
        } => Some(OldContentBlock::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        }),
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            ..
        } => Some(OldContentBlock::ToolResult {
            tool_use_id: tool_use_id.clone(),
            content: content.clone(),
        }),
        _ => None,
    }
}

/// Convert an IR `ToolChoice` to a raw JSON `Value` for legacy encoders.
pub(crate) fn ai_tool_choice_to_value(tc: &ToolChoice) -> Value {
    match tc {
        ToolChoice::Auto => Value::String("auto".into()),
        ToolChoice::None => Value::String("none".into()),
        ToolChoice::Required => Value::String("required".into()),
        ToolChoice::Named { name } => {
            serde_json::json!({ "type": "function", "function": { "name": name } })
        }
        ToolChoice::Raw(v) => v.clone(),
    }
}

/// Convert an IR `ToolSpec` to the old `ToolDef` format.
pub(crate) fn ai_tool_spec_to_old_ref(ts: &ToolSpec) -> ToolDef {
    ToolDef {
        name: ts.name.clone(),
        description: ts.description.clone(),
        parameters: ts.parameters.clone(),
    }
}

// ── Round-trip tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ids::OPENAI_CHAT_COMPLETIONS_V1;

    fn sample_old_request() -> InternalRequest {
        InternalRequest {
            messages: vec![InternalMessage {
                role: OldRole::User,
                content: OldMessageContent::Text("hello".into()),
                tool_calls: None,
                tool_call_id: None,
                extra: Default::default(),
            }],
            model: "gpt-4o".to_string(),
            stream: true,
            temperature: Some(0.7),
            max_tokens: Some(1024),
            top_p: None,
            tools: None,
            tool_choice: None,
            source_protocol: OPENAI_CHAT_COMPLETIONS_V1,
            extra: Default::default(),
        }
    }

    #[test]
    fn round_trip_internal_request() {
        let old = sample_old_request();
        let new: AiRequest = old.clone().into();
        let back: InternalRequest = new.into();

        assert_eq!(back.model, old.model);
        assert_eq!(back.stream, old.stream);
        assert_eq!(back.temperature, old.temperature);
        assert_eq!(back.max_tokens, old.max_tokens);
        assert_eq!(back.source_protocol, old.source_protocol);
        assert_eq!(back.messages.len(), old.messages.len());
    }
}
