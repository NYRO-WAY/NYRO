use sha2::{Digest, Sha256};

use crate::protocol::ids::ProtocolId;
use crate::protocol::types::{ContentBlock, InternalRequest, MessageContent};

/// Bump this constant whenever the codec field mapping changes in a way that
/// could cause a cached response to be returned in the wrong format.
///
/// This makes all existing cache entries immediately stale after a deploy
/// that changes the codec schema. Old entries are harmless (they will simply
/// be evicted by TTL / LRU), but they will no longer be served.
/// Bump when: IR mapping changes, codec field mapping changes, or cache key format changes.
/// v1 → v2: dispatch_pipeline now carries RawEnvelope + AiRequest; ingress included in key.
pub const CODEC_SCHEMA_VERSION: u32 = 2;

/// Build a deterministic cache key for an exact-match or semantic-cache
/// lookup.
///
/// The key encodes:
/// - `CODEC_SCHEMA_VERSION` — invalidates the entire cache on codec changes.
/// - `ingress` — the protocol the client used, because cached responses are
///   formatted for the ingress protocol; the same body from different ingress
///   protocols must produce different cache entries.
/// - A SHA-256 hash of the semantically-relevant request fields.
pub fn build_cache_key(request: &InternalRequest, ingress: ProtocolId) -> String {
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
                        ContentBlock::Reasoning { text, signature } => {
                            source.push_str("[thinking]");
                            source.push_str(text);
                            if let Some(sig) = signature {
                                source.push_str("[sig]");
                                source.push_str(sig);
                            }
                        }
                    }
                }
            }
        }
        source.push('|');
    }

    let digest = Sha256::digest(source.as_bytes());
    format!("v{}|{}|{:x}", CODEC_SCHEMA_VERSION, ingress, digest)
}

pub fn build_semantic_partition(model: &str, system_prompt: &str) -> String {
    let source = format!("model:{}|system:{}", model.trim(), system_prompt.trim());
    let digest = Sha256::digest(source.as_bytes());
    format!("{:x}", digest)
}
