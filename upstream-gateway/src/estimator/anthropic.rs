use serde_json::Value;

use crate::errors::RateLimitError;

pub fn estimate_input_tokens(body: &Value, actual_model: &str) -> Result<u32, RateLimitError> {
    let body_text = body.to_string();
    claude_tokenizer::count_tokens(&body_text)
        .map(|count| count as u32)
        .map_err(|error| {
            RateLimitError::token_estimation(format!(
                "anthropic tokenizer failed for model '{actual_model}': {error}"
            ))
        })
}
