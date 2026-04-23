use crate::protocol::types::InternalResponse;

pub fn normalize_response_reasoning(resp: &mut InternalResponse) {
    if resp.reasoning_content.is_some() {
        return;
    }

    let (reasoning, text) = split_think_tags(&resp.content);
    if reasoning.is_some() {
        resp.reasoning_content = reasoning;
        resp.content = text;
    }
}

pub(crate) fn split_think_tags(content: &str) -> (Option<String>, String) {
    let mut remaining = content;
    let mut reasoning_parts: Vec<String> = Vec::new();
    let mut text_parts: Vec<String> = Vec::new();

    loop {
        let Some(start_idx) = remaining.find("<think>") else {
            if !remaining.is_empty() {
                text_parts.push(remaining.to_string());
            }
            break;
        };

        let before = &remaining[..start_idx];
        if !before.is_empty() {
            text_parts.push(before.to_string());
        }

        let after_start = &remaining[start_idx + "<think>".len()..];
        let Some(end_rel_idx) = after_start.find("</think>") else {
            text_parts.push(remaining[start_idx..].to_string());
            break;
        };

        let thought = after_start[..end_rel_idx].trim();
        if !thought.is_empty() {
            reasoning_parts.push(thought.to_string());
        }
        remaining = &after_start[end_rel_idx + "</think>".len()..];
    }

    let reasoning = if reasoning_parts.is_empty() {
        None
    } else {
        Some(reasoning_parts.join("\n"))
    };
    (reasoning, text_parts.join("").trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::types::{InternalResponse, TokenUsage};

    fn make_resp(content: &str) -> InternalResponse {
        InternalResponse {
            id: String::new(),
            model: String::new(),
            content: content.to_string(),
            reasoning_content: None,
            tool_calls: vec![],
            response_items: None,
            stop_reason: None,
            usage: TokenUsage::default(),
        }
    }

    #[test]
    fn test_split_think_tags_basic() {
        let (reasoning, text) = split_think_tags("<think>let me think</think>the answer");
        assert_eq!(reasoning.as_deref(), Some("let me think"));
        assert_eq!(text, "the answer");
    }

    #[test]
    fn test_split_think_tags_no_tags() {
        let (reasoning, text) = split_think_tags("just text");
        assert!(reasoning.is_none());
        assert_eq!(text, "just text");
    }

    #[test]
    fn test_split_think_tags_multiple() {
        let (reasoning, text) = split_think_tags("<think>step1</think>middle<think>step2</think>end");
        let r = reasoning.unwrap();
        assert!(r.contains("step1"), "expected step1 in reasoning: {r}");
        assert!(r.contains("step2"), "expected step2 in reasoning: {r}");
        assert_eq!(text, "middleend");
    }

    #[test]
    fn test_split_think_tags_unclosed() {
        // Unclosed <think> is treated as regular text.
        let (reasoning, text) = split_think_tags("<think>incomplete");
        assert!(reasoning.is_none(), "unclosed think should produce no reasoning");
        assert!(text.contains("<think>"), "unclosed think tag should remain in text");
    }

    #[test]
    fn test_normalize_response_reasoning_no_op_when_already_set() {
        let mut resp = make_resp("<think>should be ignored</think>answer");
        resp.reasoning_content = Some("existing reasoning".to_string());
        normalize_response_reasoning(&mut resp);
        // Must not overwrite existing reasoning_content.
        assert_eq!(resp.reasoning_content.as_deref(), Some("existing reasoning"));
    }

    #[test]
    fn test_normalize_response_reasoning_extracts_think_tags() {
        // DeepSeek-style: reasoning wrapped in <think> tags in the content field.
        let mut resp = make_resp("<think>my reasoning</think>final answer");
        normalize_response_reasoning(&mut resp);
        assert_eq!(resp.reasoning_content.as_deref(), Some("my reasoning"));
        assert_eq!(resp.content, "final answer");
    }
}
