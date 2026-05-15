//! Compatibility helpers for progressive codec migration.
//!
//! `InternalRequest`/`InternalResponse` have been removed in PR-6.
//! This module now provides:
//! - `AiStreamDelta` ↔ old `StreamDelta` conversions (used by `LegacyStreamParserAdapter`)
//! - By-ref helpers for encoders that still use `InternalMessage` as an internal helper type

use crate::protocol::ir::request::{
    ContentBlock, MediaSource, Message, MessageContent, Role, ToolChoice, ToolSpec,
};
use crate::protocol::ir::stream::StreamDelta as AiStreamDelta;
use crate::protocol::types::{
    ContentBlock as OldContentBlock, ImageSource, InternalMessage,
    MessageContent as OldMessageContent, Role as OldRole, StreamDelta as OldStreamDelta,
    ToolCall as OldToolCall, ToolDef,
};
use serde_json::Value;

// ── StreamDelta ↔ AiStreamDelta conversions ───────────────────────────────────

/// Convert an old `StreamDelta` to the new IR `AiStreamDelta`.
pub fn old_stream_delta_to_new(d: &OldStreamDelta) -> AiStreamDelta {
    match d {
        OldStreamDelta::MessageStart { id, model } => AiStreamDelta::MessageStart {
            id: id.clone(),
            model: model.clone(),
        },
        OldStreamDelta::ReasoningDelta(s) => AiStreamDelta::ThinkingDelta(s.clone()),
        OldStreamDelta::ReasoningSignature(s) => AiStreamDelta::ThinkingSignature(s.clone()),
        OldStreamDelta::TextDelta(s) => AiStreamDelta::TextDelta(s.clone()),
        OldStreamDelta::ToolCallStart { index, id, name } => AiStreamDelta::ToolCallStart {
            index: *index,
            id: id.clone(),
            name: name.clone(),
        },
        OldStreamDelta::ToolCallDelta { index, arguments } => AiStreamDelta::ToolCallDelta {
            index: *index,
            arguments: arguments.clone(),
        },
        OldStreamDelta::Usage(u) => AiStreamDelta::Usage(u.clone()),
        OldStreamDelta::Done { stop_reason } => AiStreamDelta::Done {
            stop_reason: stop_reason.clone(),
        },
        OldStreamDelta::RawEvent { event_type, data } => AiStreamDelta::Unknown {
            raw: format!("event: {event_type}\ndata: {data}"),
        },
    }
}

/// Convert a new IR `AiStreamDelta` back to the old `StreamDelta`.
pub fn ai_stream_delta_to_old(d: &AiStreamDelta) -> OldStreamDelta {
    match d {
        AiStreamDelta::MessageStart { id, model } => OldStreamDelta::MessageStart {
            id: id.clone(),
            model: model.clone(),
        },
        AiStreamDelta::TextDelta(s) => OldStreamDelta::TextDelta(s.clone()),
        AiStreamDelta::ThinkingDelta(s) => OldStreamDelta::ReasoningDelta(s.clone()),
        AiStreamDelta::ThinkingSignature(s) => OldStreamDelta::ReasoningSignature(s.clone()),
        AiStreamDelta::ToolCallStart { index, id, name } => OldStreamDelta::ToolCallStart {
            index: *index,
            id: id.clone(),
            name: name.clone(),
        },
        AiStreamDelta::ToolCallDelta { index, arguments } => OldStreamDelta::ToolCallDelta {
            index: *index,
            arguments: arguments.clone(),
        },
        AiStreamDelta::ToolCallComplete { index, tool_call } => OldStreamDelta::ToolCallDelta {
            index: *index,
            arguments: tool_call.arguments.clone(),
        },
        AiStreamDelta::Usage(u) => OldStreamDelta::Usage(u.clone()),
        AiStreamDelta::Done { stop_reason } => OldStreamDelta::Done {
            stop_reason: stop_reason.clone(),
        },
        AiStreamDelta::StreamError { .. } => OldStreamDelta::Done {
            stop_reason: "error".to_string(),
        },
        AiStreamDelta::UnexpectedEof => OldStreamDelta::Done {
            stop_reason: "error".to_string(),
        },
        AiStreamDelta::Unknown { raw } => {
            let mut lines = raw.splitn(2, '\n');
            let event_type = lines
                .next()
                .and_then(|l| l.strip_prefix("event: "))
                .unwrap_or("unknown")
                .to_string();
            let data_str = lines
                .next()
                .and_then(|l| l.strip_prefix("data: "))
                .unwrap_or("{}");
            let data = serde_json::from_str(data_str)
                .unwrap_or_else(|_| Value::String(data_str.to_string()));
            OldStreamDelta::RawEvent { event_type, data }
        }
    }
}

// ── By-ref helpers used by encoders ──────────────────────────────────────────

/// Convert an IR `Message` to the old `InternalMessage` format (borrows; clones fields).
pub fn ai_msg_to_old_ref(msg: &Message) -> InternalMessage {
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

fn role_to_old(r: Role) -> OldRole {
    match r {
        Role::System => OldRole::System,
        Role::User => OldRole::User,
        Role::Assistant => OldRole::Assistant,
        Role::Tool => OldRole::Tool,
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
pub fn ai_tool_choice_to_value(tc: &ToolChoice) -> Value {
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
pub fn ai_tool_spec_to_old_ref(ts: &ToolSpec) -> ToolDef {
    ToolDef {
        name: ts.name.clone(),
        description: ts.description.clone(),
        parameters: ts.parameters.clone(),
    }
}
