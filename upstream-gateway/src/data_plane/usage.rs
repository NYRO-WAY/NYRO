use serde_json::Value;

use crate::provider::UpstreamProtocol;
use crate::runtime::SettlementUsage;

pub fn parse_settlement_usage(protocol: UpstreamProtocol, body: &Value) -> Option<SettlementUsage> {
    let usage = match protocol {
        UpstreamProtocol::OpenAIChatCompletions | UpstreamProtocol::OpenAIEmbeddings => {
            parse_openai_usage(body)
        }
        UpstreamProtocol::OpenAIResponses => parse_openai_responses_usage(body),
        UpstreamProtocol::AnthropicMessages => parse_anthropic_usage(body),
        UpstreamProtocol::GoogleGenerateContent | UpstreamProtocol::GoogleStreamGenerateContent => {
            parse_google_usage(body)
        }
    }?;

    if usage.input_tokens == 0 && usage.output_tokens == 0 {
        return None;
    }

    Some(usage)
}

pub fn parse_stream_settlement_usage(
    protocol: UpstreamProtocol,
    body: &Value,
) -> Option<SettlementUsage> {
    let usage = match protocol {
        UpstreamProtocol::OpenAIChatCompletions | UpstreamProtocol::OpenAIEmbeddings => {
            parse_openai_usage(body)
        }
        UpstreamProtocol::OpenAIResponses => body
            .get("response")
            .and_then(parse_openai_responses_usage)
            .or_else(|| parse_openai_responses_usage(body)),
        UpstreamProtocol::AnthropicMessages => body
            .get("message")
            .and_then(parse_anthropic_usage)
            .or_else(|| parse_anthropic_usage(body)),
        UpstreamProtocol::GoogleGenerateContent | UpstreamProtocol::GoogleStreamGenerateContent => {
            parse_google_usage(body)
        }
    }?;

    if usage.input_tokens == 0 && usage.output_tokens == 0 {
        return None;
    }

    Some(usage)
}

#[derive(Debug)]
pub struct StreamUsageTracker {
    protocol: UpstreamProtocol,
    buffer: String,
    latest_usage: Option<SettlementUsage>,
}

impl StreamUsageTracker {
    pub fn new(protocol: UpstreamProtocol) -> Self {
        Self {
            protocol,
            buffer: String::new(),
            latest_usage: None,
        }
    }

    pub fn observe_chunk(&mut self, chunk: &[u8]) {
        let text = String::from_utf8_lossy(chunk);
        self.buffer.push_str(&text);
        self.parse_complete_blocks();
    }

    pub fn finish(&mut self) -> Option<SettlementUsage> {
        if !self.buffer.trim().is_empty() {
            self.buffer.push_str("\n\n");
            self.parse_complete_blocks();
        }
        self.latest_usage
    }

    fn parse_complete_blocks(&mut self) {
        while let Some(pos) = self.buffer.find("\n\n") {
            let block = self.buffer[..pos].to_owned();
            self.buffer.drain(..pos + 2);
            self.parse_sse_block(&block);
        }
    }

    fn parse_sse_block(&mut self, block: &str) {
        let mut payload_lines = Vec::new();
        for line in block.lines() {
            if let Some(data) = line.strip_prefix("data:") {
                payload_lines.push(data.trim_start());
            }
        }
        if payload_lines.is_empty() {
            return;
        }

        let payload = payload_lines.join("\n");
        if payload.trim() == "[DONE]" {
            return;
        }

        if let Ok(value) = serde_json::from_str::<Value>(&payload)
            && let Some(usage) = parse_stream_settlement_usage(self.protocol, &value)
        {
            self.latest_usage = Some(usage);
        }
    }
}

fn parse_openai_usage(body: &Value) -> Option<SettlementUsage> {
    let usage = body.get("usage")?;
    Some(SettlementUsage {
        input_tokens: optional_u32(usage, &["prompt_tokens"]).unwrap_or(0),
        output_tokens: optional_u32(usage, &["completion_tokens"]).unwrap_or(0),
    })
}

fn parse_openai_responses_usage(body: &Value) -> Option<SettlementUsage> {
    let usage = body.get("usage")?;
    Some(SettlementUsage {
        input_tokens: optional_u32(usage, &["input_tokens"])
            .or_else(|| optional_u32(usage, &["prompt_tokens"]))
            .unwrap_or(0),
        output_tokens: optional_u32(usage, &["output_tokens"])
            .or_else(|| optional_u32(usage, &["completion_tokens"]))
            .unwrap_or(0),
    })
}

fn parse_anthropic_usage(body: &Value) -> Option<SettlementUsage> {
    let usage = body.get("usage")?;
    Some(SettlementUsage {
        input_tokens: optional_u32(usage, &["input_tokens"]).unwrap_or(0),
        output_tokens: optional_u32(usage, &["output_tokens"]).unwrap_or(0),
    })
}

fn parse_google_usage(body: &Value) -> Option<SettlementUsage> {
    let usage = body.get("usageMetadata")?;
    Some(SettlementUsage {
        input_tokens: optional_u32(usage, &["promptTokenCount"]).unwrap_or(0),
        output_tokens: optional_u32(usage, &["candidatesTokenCount"])
            .or_else(|| optional_u32(usage, &["outputTokenCount"]))
            .unwrap_or(0),
    })
}

fn optional_u32(body: &Value, path: &[&str]) -> Option<u32> {
    lookup(body, path).and_then(value_as_u32)
}

fn value_as_u32(value: &Value) -> Option<u32> {
    if let Some(v) = value.as_u64() {
        return u32::try_from(v).ok();
    }
    if let Some(v) = value.as_i64() {
        return u32::try_from(v).ok();
    }
    None
}

fn lookup<'a>(body: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = body;
    for segment in path {
        current = current.get(*segment)?;
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{StreamUsageTracker, parse_settlement_usage, parse_stream_settlement_usage};
    use crate::provider::UpstreamProtocol;
    use crate::runtime::SettlementUsage;

    #[test]
    fn parses_openai_chat_usage() {
        let body = json!({
            "usage": {
                "prompt_tokens": 11,
                "completion_tokens": 7
            }
        });

        assert_eq!(
            parse_settlement_usage(UpstreamProtocol::OpenAIChatCompletions, &body),
            Some(SettlementUsage {
                input_tokens: 11,
                output_tokens: 7,
            })
        );
    }

    #[test]
    fn parses_openai_responses_usage() {
        let body = json!({
            "usage": {
                "input_tokens": 13,
                "output_tokens": 5
            }
        });

        assert_eq!(
            parse_settlement_usage(UpstreamProtocol::OpenAIResponses, &body),
            Some(SettlementUsage {
                input_tokens: 13,
                output_tokens: 5,
            })
        );
    }

    #[test]
    fn parses_anthropic_usage() {
        let body = json!({
            "usage": {
                "input_tokens": 19,
                "output_tokens": 9
            }
        });

        assert_eq!(
            parse_settlement_usage(UpstreamProtocol::AnthropicMessages, &body),
            Some(SettlementUsage {
                input_tokens: 19,
                output_tokens: 9,
            })
        );
    }

    #[test]
    fn parses_google_usage() {
        let body = json!({
            "usageMetadata": {
                "promptTokenCount": 17,
                "candidatesTokenCount": 6,
                "totalTokenCount": 23
            }
        });

        assert_eq!(
            parse_settlement_usage(UpstreamProtocol::GoogleGenerateContent, &body),
            Some(SettlementUsage {
                input_tokens: 17,
                output_tokens: 6,
            })
        );
    }

    #[test]
    fn parses_openai_responses_stream_usage_from_response_completed_event() {
        let body = json!({
            "type": "response.completed",
            "response": {
                "usage": {
                    "input_tokens": 21,
                    "output_tokens": 4
                }
            }
        });

        assert_eq!(
            parse_stream_settlement_usage(UpstreamProtocol::OpenAIResponses, &body),
            Some(SettlementUsage {
                input_tokens: 21,
                output_tokens: 4,
            })
        );
    }

    #[test]
    fn parses_anthropic_stream_usage_from_message_start() {
        let body = json!({
            "type": "message_start",
            "message": {
                "usage": {
                    "input_tokens": 14,
                    "output_tokens": 0
                }
            }
        });

        assert_eq!(
            parse_stream_settlement_usage(UpstreamProtocol::AnthropicMessages, &body),
            Some(SettlementUsage {
                input_tokens: 14,
                output_tokens: 0,
            })
        );
    }

    #[test]
    fn stream_usage_tracker_collects_latest_usage_from_sse_blocks() {
        let mut tracker = StreamUsageTracker::new(UpstreamProtocol::OpenAIChatCompletions);
        tracker.observe_chunk(
            b"data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
        );
        tracker.observe_chunk(
            b"data: {\"id\":\"chatcmpl-1\",\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":3}}\n\n",
        );

        assert_eq!(
            tracker.finish(),
            Some(SettlementUsage {
                input_tokens: 9,
                output_tokens: 3,
            })
        );
    }
}
