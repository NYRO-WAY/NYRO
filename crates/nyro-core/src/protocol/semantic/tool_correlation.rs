use std::collections::VecDeque;

use crate::protocol::types::{ContentBlock, InternalMessage, InternalRequest, MessageContent, Role, ToolCall};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ids::OPENAI_CHAT_V1;
    use crate::protocol::types::MessageContent;

    fn make_req(messages: Vec<InternalMessage>) -> InternalRequest {
        InternalRequest {
            messages,
            model: "test".to_string(),
            stream: false,
            temperature: None,
            max_tokens: None,
            top_p: None,
            tools: None,
            tool_choice: None,
            source_protocol: OPENAI_CHAT_V1,
            extra: Default::default(),
        }
    }

    fn assistant_with_tool(tool_id: &str, tool_name: &str) -> InternalMessage {
        InternalMessage {
            role: Role::Assistant,
            content: MessageContent::Text(String::new()),
            tool_calls: Some(vec![ToolCall {
                id: tool_id.to_string(),
                name: tool_name.to_string(),
                arguments: "{}".to_string(),
            }]),
            tool_call_id: None,
        }
    }

    fn tool_result_with_id(tool_call_id: &str) -> InternalMessage {
        InternalMessage {
            role: Role::Tool,
            content: MessageContent::Text("result".to_string()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.to_string()),
        }
    }

    fn tool_result_no_id() -> InternalMessage {
        InternalMessage {
            role: Role::Tool,
            content: MessageContent::Text("result".to_string()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn test_correlation_by_matching_id() {
        let mut req = make_req(vec![
            assistant_with_tool("call_1", "get_weather"),
            tool_result_with_id("call_1"),
        ]);
        normalize_request_tool_results(&mut req);

        let tool_msg = req.messages.iter().find(|m| m.role == Role::Tool).unwrap();
        assert_eq!(tool_msg.tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn test_correlation_fifo_when_no_id() {
        // Tool result has no tool_call_id — should match the pending call by FIFO.
        let mut req = make_req(vec![
            assistant_with_tool("call_abc", "search"),
            tool_result_no_id(),
        ]);
        normalize_request_tool_results(&mut req);

        let tool_msg = req.messages.iter().find(|m| m.role == Role::Tool).unwrap();
        assert_eq!(
            tool_msg.tool_call_id.as_deref(),
            Some("call_abc"),
            "FIFO fallback should correlate to the single pending call"
        );
    }

    #[test]
    fn test_generated_id_for_empty_tool_call_id() {
        // Assistant tool_call with blank id → must be assigned a generated id.
        let mut req = make_req(vec![
            InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text(String::new()),
                tool_calls: Some(vec![ToolCall {
                    id: "".to_string(),
                    name: "my_tool".to_string(),
                    arguments: "{}".to_string(),
                }]),
                tool_call_id: None,
            },
            tool_result_no_id(),
        ]);
        normalize_request_tool_results(&mut req);

        let asst = req.messages.iter().find(|m| m.role == Role::Assistant).unwrap();
        let tc_id = &asst.tool_calls.as_ref().unwrap()[0].id;
        assert!(!tc_id.is_empty(), "blank tool_call_id must be replaced with generated id");

        let tool_msg = req.messages.iter().find(|m| m.role == Role::Tool).unwrap();
        assert_eq!(
            tool_msg.tool_call_id.as_deref(),
            Some(tc_id.as_str()),
            "tool result id must match the generated assistant tool_call id"
        );
    }

    #[test]
    fn test_multiple_tool_calls_fifo_order() {
        let mut req = make_req(vec![
            InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text(String::new()),
                tool_calls: Some(vec![
                    ToolCall { id: "call_1".to_string(), name: "tool_a".to_string(), arguments: "{}".to_string() },
                    ToolCall { id: "call_2".to_string(), name: "tool_b".to_string(), arguments: "{}".to_string() },
                ]),
                tool_call_id: None,
            },
            tool_result_no_id(), // first result → call_1 (FIFO)
            tool_result_no_id(), // second result → call_2 (FIFO)
        ]);
        normalize_request_tool_results(&mut req);

        let tool_msgs: Vec<_> = req.messages.iter().filter(|m| m.role == Role::Tool).collect();
        assert_eq!(tool_msgs[0].tool_call_id.as_deref(), Some("call_1"), "first tool result should map to call_1");
        assert_eq!(tool_msgs[1].tool_call_id.as_deref(), Some("call_2"), "second tool result should map to call_2");
    }
}
