use anyhow::Result;
use serde_json::Value;

use crate::protocol::types::TokenUsage;
use crate::protocol::{ResponseTranscoder, SseEvent, StreamTranscoder};

pub struct OpenAITranscoder;

impl ResponseTranscoder for OpenAITranscoder {
    fn transcode_response(
        &self,
        resp: Value,
    ) -> Result<(Value, TokenUsage)> {
        let usage = extract_usage(&resp);
        Ok((resp, usage))
    }

    fn stream_transcoder(&self) -> Box<dyn StreamTranscoder + Send> {
        Box::new(OpenAIStreamTranscoder {
            usage: TokenUsage::default(),
            buffer: String::new(),
        })
    }
}

struct OpenAIStreamTranscoder {
    usage: TokenUsage,
    buffer: String,
}

impl StreamTranscoder for OpenAIStreamTranscoder {
    fn process_chunk(&mut self, raw: &str) -> Result<Vec<SseEvent>> {
        self.buffer.push_str(raw);
        let mut events = Vec::new();

        while let Some(pos) = self.buffer.find("\n\n") {
            let block = self.buffer[..pos].to_string();
            self.buffer = self.buffer[pos + 2..].to_string();

            for line in block.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    let data = data.trim();
                    if data == "[DONE]" {
                        events.push(SseEvent::new(None, "[DONE]"));
                        continue;
                    }
                    if let Ok(chunk) = serde_json::from_str::<Value>(data) {
                        let u = extract_usage(&chunk);
                        if u.input_tokens > 0 {
                            self.usage.input_tokens = u.input_tokens;
                        }
                        if u.output_tokens > 0 {
                            self.usage.output_tokens = u.output_tokens;
                        }
                        events.push(SseEvent::new(None, chunk.to_string()));
                    }
                }
            }
        }

        Ok(events)
    }

    fn finish(&mut self) -> Result<Vec<SseEvent>> {
        if self.buffer.trim().is_empty() {
            return Ok(vec![]);
        }
        let remaining = std::mem::take(&mut self.buffer);
        self.process_chunk(&format!("{remaining}\n\n"))
    }

    fn usage(&self) -> TokenUsage {
        self.usage.clone()
    }
}

fn extract_usage(v: &Value) -> TokenUsage {
    if let Some(u) = v.get("usage") {
        TokenUsage {
            input_tokens: u
                .get("prompt_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            output_tokens: u
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
        }
    } else {
        TokenUsage::default()
    }
}
