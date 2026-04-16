use anyhow::Result;
use reqwest::header::HeaderMap;
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;

use crate::protocol::EgressEncoder;
use crate::protocol::types::*;

pub struct OpenAIEncoder;

impl EgressEncoder for OpenAIEncoder {
    fn encode_request(&self, req: &InternalRequest) -> Result<(Value, HeaderMap)> {
        let normalized_messages =
            normalize_messages_for_openai(&req.messages, req.tools.as_deref());
        let messages: Vec<Value> = normalized_messages
            .iter()
            .map(encode_message)
            .collect::<Result<Vec<_>>>()?;

        let mut body = serde_json::json!({
            "model": req.model,
            "messages": messages,
            "stream": req.stream,
        });

        let obj = body.as_object_mut().unwrap();

        if let Some(t) = req.temperature {
            obj.insert("temperature".into(), t.into());
        }
        if let Some(m) = req.max_tokens {
            obj.insert("max_tokens".into(), m.into());
        }
        if let Some(p) = req.top_p {
            obj.insert("top_p".into(), p.into());
        }

        if let Some(ref tools) = req.tools {
            let tools_val: Vec<Value> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters,
                        }
                    })
                })
                .collect();
            obj.insert("tools".into(), Value::Array(tools_val));
        }
        if let Some(ref tc) = req.tool_choice {
            obj.insert("tool_choice".into(), tc.clone());
        }

        if req.stream {
            obj.insert(
                "stream_options".into(),
                serde_json::json!({"include_usage": true}),
            );
        }

        for (k, v) in &req.extra {
            obj.entry(k.clone()).or_insert_with(|| v.clone());
        }

        Ok((body, HeaderMap::new()))
    }

    fn egress_path(&self, _model: &str, _stream: bool) -> String {
        "/v1/chat/completions".to_string()
    }
}

fn normalize_messages_for_openai(
    messages: &[InternalMessage],
    tools: Option<&[ToolDef]>,
) -> Vec<InternalMessage> {
    let preprocessed = remap_duplicate_tool_call_ids(messages);

    let mut out: Vec<InternalMessage> = Vec::with_capacity(preprocessed.len() + 2);
    let mut seen_tool_call_ids: HashSet<String> = HashSet::new();
    let mut consumed_tool_result_ids: HashSet<String> = HashSet::new();
    let mut generated_seq: usize = 0;
    let fallback_tool_name = tools
        .and_then(|defs| defs.first())
        .map(|d| d.name.clone())
        .unwrap_or_else(|| "tool".to_string());

    for msg in &preprocessed {
        let mut msg = msg.clone();

        if msg.role == Role::Assistant {
            if let Some(tool_calls) = &mut msg.tool_calls {
                for tc in tool_calls.iter_mut() {
                    if tc.id.trim().is_empty() {
                        generated_seq += 1;
                        tc.id = format!("call_enc_{generated_seq}");
                    }
                    if tc.name.trim().is_empty() {
                        tc.name = fallback_tool_name.clone();
                    }
                    seen_tool_call_ids.insert(tc.id.clone());
                }
            }
            out.push(msg);
            continue;
        }

        if msg.role != Role::Tool {
            out.push(msg);
            continue;
        }

        let hinted_id = tool_message_payload(&msg).1;
        let mut resolved_id = msg
            .tool_call_id
            .clone()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| hinted_id.clone().filter(|v| !v.trim().is_empty()));

        if resolved_id.is_none() {
            generated_seq += 1;
            resolved_id = Some(format!("call_enc_{generated_seq}"));
        }
        let mut final_id = resolved_id.expect("tool_call_id should always exist");
        if consumed_tool_result_ids.contains(&final_id) {
            generated_seq += 1;
            final_id = format!("call_enc_{generated_seq}");
        }

        let extracted_call = take_matching_tool_call_from_history(&mut out, &final_id);
        if let Some((tc, source_idx)) = extracted_call {
            trim_trailing_assistant_text_after_index(&mut out, source_idx);
            out.push(InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text(String::new()),
                tool_calls: Some(vec![tc]),
                tool_call_id: None,
            });
            seen_tool_call_ids.insert(final_id.clone());
        } else {
            let has_adjacent_matching_call = out
                .last()
                .is_some_and(|prev| assistant_has_tool_call_id(prev, &final_id));

            let has_adjacent_matching_call = if has_adjacent_matching_call {
                true
            } else {
                make_matching_call_adjacent(&mut out, &final_id)
            };

            if !has_adjacent_matching_call {
                if seen_tool_call_ids.contains(&final_id) {
                    generated_seq += 1;
                    final_id = format!("call_enc_{generated_seq}");
                }
                let synth_name = hinted_id
                    .as_deref()
                    .filter(|v| !v.trim().is_empty())
                    .map(|_| fallback_tool_name.clone())
                    .unwrap_or_else(|| fallback_tool_name.clone());
                out.push(InternalMessage {
                    role: Role::Assistant,
                    content: MessageContent::Text(String::new()),
                    tool_calls: Some(vec![ToolCall {
                        id: final_id.clone(),
                        name: synth_name,
                        arguments: "{}".to_string(),
                    }]),
                    tool_call_id: None,
                });
                seen_tool_call_ids.insert(final_id.clone());
            }
        }

        msg.tool_call_id = Some(final_id.clone());
        consumed_tool_result_ids.insert(final_id);
        out.push(msg);
    }

    out = prune_orphan_assistant_tool_calls(out);

    out.retain(|msg| {
        if msg.role != Role::Assistant {
            return true;
        }
        let has_calls = msg.tool_calls.as_ref().is_some_and(|c| !c.is_empty());
        if has_calls {
            return true;
        }
        !msg.content.as_text().trim().is_empty()
    });

    out
}

fn prune_orphan_assistant_tool_calls(messages: Vec<InternalMessage>) -> Vec<InternalMessage> {
    let referenced_tool_ids: HashSet<String> = messages
        .iter()
        .filter(|m| m.role == Role::Tool)
        .filter_map(|m| m.tool_call_id.clone())
        .filter(|id| !id.trim().is_empty())
        .collect();

    let mut out: Vec<InternalMessage> = Vec::with_capacity(messages.len());
    for mut msg in messages {
        if msg.role == Role::Assistant {
            if let Some(calls) = msg.tool_calls.take() {
                let kept: Vec<ToolCall> = calls
                    .into_iter()
                    .filter(|tc| referenced_tool_ids.contains(&tc.id))
                    .collect();
                if !kept.is_empty() {
                    msg.tool_calls = Some(kept);
                }
            }
        }
        out.push(msg);
    }
    out
}

fn assistant_has_tool_call_id(msg: &InternalMessage, tool_call_id: &str) -> bool {
    if msg.role != Role::Assistant {
        return false;
    }
    msg.tool_calls.as_ref().is_some_and(|calls| {
        calls
            .iter()
            .any(|tc| !tc.id.trim().is_empty() && tc.id == tool_call_id)
    })
}

fn remap_duplicate_tool_call_ids(messages: &[InternalMessage]) -> Vec<InternalMessage> {
    let mut out = messages.to_vec();
    let mut seen_counts: HashMap<String, usize> = HashMap::new();
    let mut pending_by_original: HashMap<String, Vec<String>> = HashMap::new();
    let mut generated_seq: usize = 0;

    for msg in &mut out {
        if msg.role == Role::Assistant {
            if let Some(tool_calls) = &mut msg.tool_calls {
                for tc in tool_calls.iter_mut() {
                    let original = if tc.id.trim().is_empty() {
                        generated_seq += 1;
                        format!("call_enc_{generated_seq}")
                    } else {
                        tc.id.clone()
                    };

                    let count = seen_counts.entry(original.clone()).or_insert(0);
                    *count += 1;
                    let unique = if *count == 1 {
                        original.clone()
                    } else {
                        format!("{}_dup{}", original, *count)
                    };
                    tc.id = unique.clone();
                    pending_by_original
                        .entry(original)
                        .or_default()
                        .push(unique);
                }
            }
            continue;
        }

        if msg.role != Role::Tool {
            continue;
        }

        let Some(original_id) = msg
            .tool_call_id
            .as_ref()
            .filter(|v| !v.trim().is_empty())
            .cloned()
        else {
            continue;
        };

        if let Some(stack) = pending_by_original.get_mut(&original_id) {
            if let Some(unique_id) = stack.pop() {
                msg.tool_call_id = Some(unique_id);
            }
        }
    }

    out
}

fn make_matching_call_adjacent(out: &mut Vec<InternalMessage>, tool_call_id: &str) -> bool {
    if out.is_empty() {
        return false;
    }

    loop {
        let Some(last) = out.last() else {
            return false;
        };
        if assistant_has_tool_call_id(last, tool_call_id) {
            return true;
        }

        // Drop trailing assistant text-only messages between a tool_call and its result.
        // This keeps OpenAI/MiniMax strict "tool result follows tool call" ordering.
        let drop_candidate = last.role == Role::Assistant
            && last
                .tool_calls
                .as_ref()
                .map_or(true, |calls| calls.is_empty())
            && last
                .tool_call_id
                .as_ref()
                .map_or(true, |id| id.trim().is_empty());
        if drop_candidate {
            let _ = out.pop();
            continue;
        }
        return false;
    }
}

fn take_matching_tool_call_from_history(
    out: &mut [InternalMessage],
    tool_call_id: &str,
) -> Option<(ToolCall, usize)> {
    for (idx, msg) in out.iter_mut().enumerate().rev() {
        if msg.role != Role::Assistant {
            continue;
        }
        let Some(calls) = msg.tool_calls.as_mut() else {
            continue;
        };
        if let Some(pos) = calls.iter().position(|tc| tc.id == tool_call_id) {
            let tc = calls.remove(pos);
            if calls.is_empty() {
                msg.tool_calls = None;
            }
            return Some((tc, idx));
        }
    }
    None
}

fn trim_trailing_assistant_text_after_index(out: &mut Vec<InternalMessage>, source_idx: usize) {
    while out.len() > source_idx + 1 {
        let Some(last) = out.last() else {
            break;
        };
        let drop_candidate = last.role == Role::Assistant
            && last
                .tool_calls
                .as_ref()
                .map_or(true, |calls| calls.is_empty())
            && last
                .tool_call_id
                .as_ref()
                .map_or(true, |id| id.trim().is_empty());
        if drop_candidate {
            let _ = out.pop();
            continue;
        }
        break;
    }
}

fn encode_message(msg: &InternalMessage) -> Result<Value> {
    let role = match msg.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };

    let mut obj = serde_json::json!({ "role": role });
    let map = obj.as_object_mut().unwrap();

    if msg.role == Role::Tool {
        let (tool_content, hinted_tool_call_id) = tool_message_payload(msg);
        map.insert("content".into(), Value::String(tool_content));
        let resolved_tool_call_id = msg
            .tool_call_id
            .clone()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| hinted_tool_call_id.filter(|v| !v.trim().is_empty()));
        if let Some(tool_call_id) = resolved_tool_call_id {
            map.insert("tool_call_id".into(), Value::String(tool_call_id));
        }
        return Ok(obj);
    }

    match &msg.content {
        MessageContent::Text(t) => {
            map.insert("content".into(), Value::String(t.clone()));
        }
        MessageContent::Blocks(blocks) => {
            let parts: Vec<Value> = blocks
                .iter()
                .map(|b| match b {
                    ContentBlock::Text { text } => {
                        serde_json::json!({"type": "text", "text": text})
                    }
                    ContentBlock::Image { source } => {
                        serde_json::json!({
                            "type": "image_url",
                            "image_url": {"url": &source.data}
                        })
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        serde_json::json!({
                            "type": "function",
                            "id": id,
                            "function": {"name": name, "arguments": input.to_string()}
                        })
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                    } => {
                        serde_json::json!({
                            "type": "text",
                            "text": content.to_string(),
                            "tool_call_id": tool_use_id,
                        })
                    }
                })
                .collect();
            map.insert("content".into(), Value::Array(parts));
        }
    }

    if let Some(ref tcs) = msg.tool_calls {
        let arr: Vec<Value> = tcs
            .iter()
            .map(|tc| {
                serde_json::json!({
                    "id": tc.id,
                    "type": "function",
                    "function": {
                        "name": tc.name,
                        "arguments": tc.arguments,
                    }
                })
            })
            .collect();
        map.insert("tool_calls".into(), Value::Array(arr));
    }
    if let Some(ref tid) = msg.tool_call_id {
        map.insert("tool_call_id".into(), Value::String(tid.clone()));
    }

    Ok(obj)
}

fn tool_message_payload(msg: &InternalMessage) -> (String, Option<String>) {
    match &msg.content {
        MessageContent::Text(t) => (t.clone(), None),
        MessageContent::Blocks(blocks) => {
            for block in blocks {
                if let ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                } = block
                {
                    let text = match content {
                        Value::String(s) => s.clone(),
                        Value::Null => String::new(),
                        other => other.to_string(),
                    };
                    let hinted_id = if tool_use_id.trim().is_empty() {
                        None
                    } else {
                        Some(tool_use_id.clone())
                    };
                    return (text, hinted_id);
                }
            }
            (msg.content.as_text(), None)
        }
    }
}
