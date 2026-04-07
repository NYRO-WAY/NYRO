use sha2::{Digest, Sha256};

use crate::protocol::types::{ContentBlock, InternalRequest, MessageContent};

pub fn build_cache_key(namespace: Option<&str>, request: &InternalRequest) -> String {
    let mut source = String::new();
    source.push_str("model:");
    source.push_str(request.model.trim());
    source.push('|');
    source.push_str("temperature:");
    if let Some(temperature) = request.temperature {
        source.push_str(&temperature.to_string());
    }
    source.push('|');
    source.push_str("max_tokens:");
    if let Some(max_tokens) = request.max_tokens {
        source.push_str(&max_tokens.to_string());
    }
    source.push('|');
    source.push_str("top_p:");
    if let Some(top_p) = request.top_p {
        source.push_str(&top_p.to_string());
    }
    source.push('|');
    source.push_str("tool_choice:");
    if let Some(tool_choice) = &request.tool_choice {
        source.push_str(&tool_choice.to_string());
    }
    source.push('|');
    source.push_str("tools:");
    if let Some(tools) = &request.tools {
        source.push_str(&serde_json::to_string(tools).unwrap_or_default());
    }
    source.push('|');
    source.push_str("messages:");
    for msg in &request.messages {
        source.push_str(&format!("{:?}:", msg.role));
        match &msg.content {
            MessageContent::Text(text) => source.push_str(text),
            MessageContent::Blocks(blocks) => {
                for block in blocks {
                    match block {
                        ContentBlock::Text { text } => source.push_str(text),
                        ContentBlock::ToolUse { id, name, input } => {
                            source.push_str(id);
                            source.push_str(name);
                            source.push_str(&input.to_string());
                        }
                        ContentBlock::ToolResult { tool_use_id, content } => {
                            source.push_str(tool_use_id);
                            source.push_str(&content.to_string());
                        }
                        ContentBlock::Image { .. } => source.push_str("[image]"),
                    }
                }
            }
        }
        source.push('|');
    }

    let digest = Sha256::digest(source.as_bytes());
    let hash = format!("{:x}", digest);
    if let Some(ns) = namespace.filter(|v| !v.trim().is_empty()) {
        format!("{}:{}", ns.trim(), hash)
    } else {
        hash
    }
}
