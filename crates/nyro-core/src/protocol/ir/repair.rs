//! Single-direction IR repair passes.
//!
//! Each function mutates an `AiRequest` to fix common structural issues found
//! in real-world client payloads.  Repair is one-way (fills gaps; never
//! rejects).  These used to live in `protocol/semantic/tool_correlation.rs`
//! under the name `normalize_request_tool_results`.
//!
//! # Available repairs
//!
//! | Function | What it fixes |
//! |---|---|
//! | `fill_tool_call_ids` | Fills missing `tool_call_id` on assistant and tool messages using FIFO correlation |
//! | `fix_orphan_tool_results` | Synthesizes a ghost assistant message for orphaned `Role::Tool` messages |
//! | `patch_broken_conversation` | Ensures the conversation starts with `user` or `system` (not `assistant`) |

use std::collections::VecDeque;

use crate::protocol::ir::request::{AiRequest, ContentBlock, Message, MessageContent, Role, ToolCall};

// ── fill_tool_call_ids ────────────────────────────────────────────────────────

/// Fill missing or blank `tool_call_id` fields on `Role::Tool` messages using
/// FIFO correlation against the preceding `Role::Assistant` tool calls.
///
/// Also generates IDs for blank assistant `ToolCall.id` fields.
pub fn fill_tool_call_ids(req: &mut AiRequest) {
    let mut pending_calls: VecDeque<(String, String)> = VecDeque::new();
    let mut generated_id_seq: usize = 0;
    let mut normalized: Vec<Message> = Vec::with_capacity(req.messages.len());

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
            normalized.push(msg);
            continue;
        }

        if msg.role != Role::Tool {
            normalized.push(msg);
            continue;
        }

        let existing_id = msg
            .tool_call_id
            .as_ref()
            .filter(|v: &&String| !v.trim().is_empty())
            .cloned();

        let mut resolved_id: Option<String> = None;
        let mut has_linked = false;

        // Try exact ID match.
        if let Some(id) = existing_id.as_ref()
            && let Some(pos) = pending_calls.iter().position(|(pid, _)| pid == id) {
                pending_calls.remove(pos);
                resolved_id = Some(id.clone());
                has_linked = true;
            }

        // Try hint from content blocks.
        let hint = extract_tool_result_hint(&msg.content);

        if resolved_id.is_none()
            && let Some(h) = hint.as_ref()
                && let Some(pos) = pending_calls.iter().position(|(pid, _)| pid == h)
                    && let Some((cid, _)) = pending_calls.remove(pos) {
                        resolved_id = Some(cid);
                        has_linked = true;
                    }

        if resolved_id.is_none()
            && let Some(h) = hint.as_ref()
                && let Some(pos) =
                    pending_calls.iter().position(|(_, pname)| pname.eq_ignore_ascii_case(h))
                    && let Some((cid, _)) = pending_calls.remove(pos) {
                        resolved_id = Some(cid);
                        has_linked = true;
                    }

        // FIFO fallback.
        if resolved_id.is_none()
            && let Some((cid, _)) = pending_calls.pop_front() {
                resolved_id = Some(cid);
                has_linked = true;
            }

        if resolved_id.is_none() {
            resolved_id = existing_id;
        }

        if resolved_id.is_none() {
            generated_id_seq += 1;
            resolved_id = Some(format!("call_nyro_synth_{generated_id_seq}"));
        }

        let final_id = resolved_id.unwrap();

        if !has_linked {
            let synth_name = hint.unwrap_or_else(|| "unknown_tool".to_string());
            normalized.push(Message {
                role: Role::Assistant,
                content: MessageContent::Text(String::new()),
                tool_calls: Some(vec![ToolCall {
                    id: final_id.clone(),
                    name: synth_name,
                    arguments: "{}".to_string(),
                }]),
                tool_call_id: None,
                meta: None,
            });
        }

        msg.tool_call_id = Some(final_id);
        normalized.push(msg);
    }

    req.messages = normalized;
}

// ── patch_broken_conversation ─────────────────────────────────────────────────

/// Ensure the conversation does not start with a `Role::Assistant` message,
/// which most providers reject.  If it does, prefix a synthetic empty
/// `Role::User` message.
pub fn patch_broken_conversation(req: &mut AiRequest) {
    if req.messages.first().map(|m| m.role) == Some(Role::Assistant) {
        req.messages.insert(
            0,
            Message {
                role: Role::User,
                content: MessageContent::Text(String::new()),
                tool_calls: None,
                tool_call_id: None,
                meta: None,
            },
        );
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn extract_tool_result_hint(content: &MessageContent) -> Option<String> {
    let MessageContent::Blocks(blocks) = content else {
        return None;
    };
    for block in blocks {
        if let ContentBlock::ToolResult { tool_use_id, .. } = block
            && !tool_use_id.trim().is_empty() {
                return Some(tool_use_id.clone());
            }
    }
    None
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ir::request::AiRequest;

    fn ai_req(messages: Vec<Message>) -> AiRequest {
        AiRequest::new("test", messages)
    }

    fn asst_with_tool(id: &str, name: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: MessageContent::Text(String::new()),
            tool_calls: Some(vec![ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                arguments: "{}".to_string(),
            }]),
            tool_call_id: None,
            meta: None,
        }
    }

    fn tool_result(tool_call_id: Option<&str>) -> Message {
        Message {
            role: Role::Tool,
            content: MessageContent::Text("result".to_string()),
            tool_calls: None,
            tool_call_id: tool_call_id.map(|s| s.to_string()),
            meta: None,
        }
    }

    #[test]
    fn correlation_by_id() {
        let mut req = ai_req(vec![
            asst_with_tool("call_1", "get_weather"),
            tool_result(Some("call_1")),
        ]);
        fill_tool_call_ids(&mut req);
        let tool_msg = req.messages.iter().find(|m| m.role == Role::Tool).unwrap();
        assert_eq!(tool_msg.tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn fifo_fallback_when_no_id() {
        let mut req = ai_req(vec![asst_with_tool("call_abc", "search"), tool_result(None)]);
        fill_tool_call_ids(&mut req);
        let tool_msg = req.messages.iter().find(|m| m.role == Role::Tool).unwrap();
        assert_eq!(tool_msg.tool_call_id.as_deref(), Some("call_abc"));
    }

    #[test]
    fn patch_breaks_assistant_first() {
        let mut req = ai_req(vec![asst_with_tool("c1", "t1")]);
        patch_broken_conversation(&mut req);
        assert_eq!(req.messages[0].role, Role::User);
    }
}
