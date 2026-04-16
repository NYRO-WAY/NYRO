use std::collections::VecDeque;

use crate::protocol::types::{
    ContentBlock, InternalMessage, InternalRequest, MessageContent, Role, ToolCall,
};

pub fn normalize_request_tool_results(req: &mut InternalRequest) {
    let mut pending_calls: VecDeque<(String, String)> = VecDeque::new();
    let mut generated_id_seq: usize = 0;
    let mut normalized_messages: Vec<InternalMessage> = Vec::with_capacity(req.messages.len());

    for mut msg in req.messages.drain(..) {
        if msg.role == Role::Assistant {
            if let Some(tool_calls) = &mut msg.tool_calls {
                for tc in tool_calls.iter_mut() {
                    if tc.id.trim().is_empty() {
                        generated_id_seq += 1;
                        tc.id = format!("call_nyro_{generated_id_seq}");
                    }
                    pending_calls.push_back((tc.id.clone(), tc.name.clone()));
                }
            }
            normalized_messages.push(msg);
            continue;
        }

        if msg.role != Role::Tool {
            normalized_messages.push(msg);
            continue;
        }

        let existing_id = msg
            .tool_call_id
            .as_ref()
            .filter(|v| !v.trim().is_empty())
            .cloned();

        let mut resolved_id: Option<String> = None;
        let mut has_linked_pending_call = false;

        if let Some(id) = existing_id.as_ref() {
            if let Some(pos) = pending_calls
                .iter()
                .position(|(pending_id, _)| pending_id == id)
            {
                let _ = pending_calls.remove(pos);
                resolved_id = Some(id.clone());
                has_linked_pending_call = true;
            }
        }

        let hinted_value = extract_tool_result_hint(&msg.content);

        if resolved_id.is_none() {
            if let Some(hint) = hinted_value.clone() {
                if let Some(pos) = pending_calls
                    .iter()
                    .position(|(pending_id, _)| pending_id == &hint)
                {
                    if let Some((call_id, _)) = pending_calls.remove(pos) {
                        resolved_id = Some(call_id);
                        has_linked_pending_call = true;
                    }
                }
            }
        }

        if resolved_id.is_none() {
            if let Some(hint) = hinted_value.clone() {
                if let Some(pos) = pending_calls
                    .iter()
                    .position(|(_, pending_name)| pending_name.eq_ignore_ascii_case(&hint))
                {
                    if let Some((call_id, _)) = pending_calls.remove(pos) {
                        resolved_id = Some(call_id);
                        has_linked_pending_call = true;
                    }
                }
            }
        }

        if resolved_id.is_none() {
            // Fallback: correlate by FIFO pending tool call when client omitted tool_call_id.
            if let Some((call_id, _name)) = pending_calls.pop_front() {
                resolved_id = Some(call_id);
                has_linked_pending_call = true;
            }
        }

        if resolved_id.is_none() {
            resolved_id = existing_id;
        }

        if resolved_id.is_none() {
            generated_id_seq += 1;
            resolved_id = Some(format!("call_nyro_synth_{generated_id_seq}"));
        }

        let final_id = resolved_id.expect("final tool_call_id should always exist");
        if !has_linked_pending_call {
            let synth_name = hinted_value.unwrap_or_else(|| "unknown_tool".to_string());
            normalized_messages.push(InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text(String::new()),
                tool_calls: Some(vec![ToolCall {
                    id: final_id.clone(),
                    name: synth_name.clone(),
                    arguments: "{}".to_string(),
                }]),
                tool_call_id: None,
            });
        }

        msg.tool_call_id = Some(final_id);
        normalized_messages.push(msg);
    }

    req.messages = normalized_messages;
}

fn extract_tool_result_hint(content: &MessageContent) -> Option<String> {
    let MessageContent::Blocks(blocks) = content else {
        return None;
    };
    for block in blocks {
        if let ContentBlock::ToolResult { tool_use_id, .. } = block {
            if !tool_use_id.trim().is_empty() {
                return Some(tool_use_id.clone());
            }
        }
    }
    None
}
