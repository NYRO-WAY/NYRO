mod anthropic;
mod google;
mod openai;

use serde_json::Value;

use crate::config::ModelRateLimitRule;
use crate::errors::RateLimitError;
use crate::provider::UpstreamProtocol;

pub fn estimate_input_tokens(
    upstream_protocol: UpstreamProtocol,
    actual_model: &str,
    body: &Value,
    matched_rule: Option<&ModelRateLimitRule>,
) -> Result<u32, RateLimitError> {
    match upstream_protocol {
        UpstreamProtocol::OpenAIChatCompletions
        | UpstreamProtocol::OpenAIResponses
        | UpstreamProtocol::OpenAIEmbeddings => {
            openai::estimate_input_tokens(body, actual_model, matched_rule)
        }
        UpstreamProtocol::AnthropicMessages => anthropic::estimate_input_tokens(body, actual_model),
        UpstreamProtocol::GoogleGenerateContent | UpstreamProtocol::GoogleStreamGenerateContent => {
            google::estimate_input_tokens(body, actual_model, matched_rule)
        }
    }
}
