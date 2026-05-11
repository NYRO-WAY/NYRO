use std::borrow::Cow;

use serde_json::Value;

use crate::config::{ModelRateLimitRule, OpenAITokenizerEncoding};
use crate::errors::RateLimitError;

pub fn estimate_input_tokens(
    body: &Value,
    actual_model: &str,
    matched_rule: Option<&ModelRateLimitRule>,
) -> Result<u32, RateLimitError> {
    let body_text = body.to_string();
    let mut max_tokens = 0u32;
    for candidate in resolve_openai_token_count_candidates(actual_model, matched_rule)? {
        let tokens = candidate.bpe.encode_with_special_tokens(&body_text).len() as u32;
        max_tokens = max_tokens.max(tokens);
    }

    Ok(max_tokens)
}

struct OpenAITokenCountCandidate<'a> {
    #[allow(dead_code)]
    model_name: Cow<'a, str>,
    bpe: &'static tiktoken_rs::CoreBPE,
}

fn resolve_openai_token_count_candidates<'a>(
    actual_model: &'a str,
    matched_rule: Option<&ModelRateLimitRule>,
) -> Result<Vec<OpenAITokenCountCandidate<'a>>, RateLimitError> {
    let normalized = normalize_openai_model_name(actual_model);

    if let Some(singleton) = bpe_for_openai_model(normalized) {
        return Ok(vec![OpenAITokenCountCandidate {
            model_name: Cow::Borrowed(normalized),
            bpe: singleton,
        }]);
    }

    if let Some(tokenizer_model) = matched_rule
        .and_then(|rule| rule.tokenizer_model.as_deref())
        .map(normalize_openai_model_name)
        && let Some(singleton) = bpe_for_openai_model(tokenizer_model)
    {
        return Ok(vec![OpenAITokenCountCandidate {
            model_name: Cow::Owned(tokenizer_model.to_string()),
            bpe: singleton,
        }]);
    }

    if let Some(encoding) = matched_rule.and_then(|rule| rule.tokenizer_encoding) {
        return Ok(vec![OpenAITokenCountCandidate {
            model_name: Cow::Borrowed(encoding.representative_model()),
            bpe: encoding.bpe()?,
        }]);
    }

    Ok(OpenAITokenizerEncoding::fallback_candidates()
        .iter()
        .copied()
        .filter_map(|encoding| {
            encoding.bpe().ok().map(|bpe| OpenAITokenCountCandidate {
                model_name: Cow::Borrowed(encoding.representative_model()),
                bpe,
            })
        })
        .collect())
}

fn normalize_openai_model_name(actual_model: &str) -> &str {
    let trimmed = actual_model.trim();
    let without_models = trimmed.trim_start_matches("models/");
    without_models
        .rsplit('/')
        .next()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(without_models)
}

fn bpe_for_openai_model(model: &str) -> Option<&'static tiktoken_rs::CoreBPE> {
    let lower = model.to_ascii_lowercase();

    if lower.starts_with("gpt-4o")
        || lower.starts_with("gpt-4.1")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
        || lower.starts_with("chatgpt-4o")
        || lower.starts_with("gpt-oss")
    {
        return Some(tiktoken_rs::o200k_base_singleton());
    }

    if lower.starts_with("gpt-4")
        || lower.starts_with("gpt-3.5")
        || lower.starts_with("text-embedding-ada")
    {
        return Some(tiktoken_rs::cl100k_base_singleton());
    }

    None
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::config::{ModelRateLimitRule, OpenAITokenizerEncoding};

    use super::estimate_input_tokens;

    #[test]
    fn unknown_model_can_use_rule_encoding_fallback() {
        let body = json!({
            "model": "unknown-openai-model",
            "messages": [
                { "role": "user", "content": "Hello world" }
            ]
        });
        let rule = ModelRateLimitRule {
            model: "*".to_string(),
            rpm: None,
            rpd: None,
            tpm: Some(1000),
            tpm_mode: None,
            tokenizer_encoding: Some(OpenAITokenizerEncoding::Cl100kBase),
            tokenizer_model: None,
        };

        let tokens = estimate_input_tokens(&body, "unknown-openai-model", Some(&rule)).unwrap();
        assert!(tokens > 0);
    }
}
