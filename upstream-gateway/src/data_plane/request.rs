use serde::Serialize;
use serde_json::Value;

use crate::provider::UpstreamProtocol;

#[derive(Debug, Clone, Serialize)]
pub struct NativeRequestMetadata {
    pub upstream_protocol: UpstreamProtocol,
    pub actual_model: String,
    pub stream: bool,
    pub request_output_reservation: u32,
}

pub fn extract_request_metadata(
    upstream_protocol: UpstreamProtocol,
    model_action: Option<&str>,
    body: &Value,
) -> Result<NativeRequestMetadata, String> {
    match upstream_protocol {
        UpstreamProtocol::OpenAIChatCompletions => Ok(NativeRequestMetadata {
            upstream_protocol,
            actual_model: required_string(body, &["model"])?,
            stream: optional_bool(body, &["stream"]).unwrap_or(false),
            request_output_reservation: optional_u32(
                body,
                &[&["max_completion_tokens"], &["max_tokens"]],
            )
            .unwrap_or(0),
        }),
        UpstreamProtocol::OpenAIResponses => Ok(NativeRequestMetadata {
            upstream_protocol,
            actual_model: required_string(body, &["model"])?,
            stream: optional_bool(body, &["stream"]).unwrap_or(false),
            request_output_reservation: optional_u32(body, &[&["max_output_tokens"]]).unwrap_or(0),
        }),
        UpstreamProtocol::OpenAIEmbeddings => Ok(NativeRequestMetadata {
            upstream_protocol,
            actual_model: required_string(body, &["model"])?,
            stream: false,
            request_output_reservation: 0,
        }),
        UpstreamProtocol::AnthropicMessages => Ok(NativeRequestMetadata {
            upstream_protocol,
            actual_model: required_string(body, &["model"])?,
            stream: optional_bool(body, &["stream"]).unwrap_or(false),
            request_output_reservation: optional_u32(body, &[&["max_tokens"]]).unwrap_or(0),
        }),
        UpstreamProtocol::GoogleGenerateContent | UpstreamProtocol::GoogleStreamGenerateContent => {
            let (actual_model, stream) = parse_google_model_action(model_action)?;
            Ok(NativeRequestMetadata {
                upstream_protocol,
                actual_model,
                stream,
                request_output_reservation: optional_u32(
                    body,
                    &[&["generationConfig", "maxOutputTokens"]],
                )
                .unwrap_or(0),
            })
        }
    }
}

pub fn parse_google_model_action(model_action: Option<&str>) -> Result<(String, bool), String> {
    let raw = model_action
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "google model action is required".to_string())?;

    let mut parts = raw.splitn(2, ':');
    let model = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("invalid google model action: {raw}"))?;
    let action = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("invalid google model action: {raw}"))?;
    let stream = matches!(action, "streamGenerateContent" | "streamGenerateContent?");
    Ok((model.to_string(), stream))
}

fn required_string(body: &Value, path: &[&str]) -> Result<String, String> {
    lookup(body, path)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| format!("required string field '{}' is missing", path.join(".")))
}

fn optional_bool(body: &Value, path: &[&str]) -> Option<bool> {
    lookup(body, path).and_then(Value::as_bool)
}

fn optional_u32(body: &Value, paths: &[&[&str]]) -> Option<u32> {
    paths
        .iter()
        .find_map(|path| lookup(body, path).and_then(value_as_u32))
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

    use super::*;

    #[test]
    fn extracts_openai_chat_metadata() {
        let body = json!({
            "model": "gpt-4o",
            "stream": true,
            "max_completion_tokens": 512
        });
        let metadata =
            extract_request_metadata(UpstreamProtocol::OpenAIChatCompletions, None, &body).unwrap();

        assert_eq!(metadata.actual_model, "gpt-4o");
        assert!(metadata.stream);
        assert_eq!(metadata.request_output_reservation, 512);
    }

    #[test]
    fn extracts_google_model_and_output_reservation() {
        let body = json!({
            "generationConfig": {
                "maxOutputTokens": 2048
            }
        });
        let metadata = extract_request_metadata(
            UpstreamProtocol::GoogleStreamGenerateContent,
            Some("gemini-2.5-pro:streamGenerateContent"),
            &body,
        )
        .unwrap();

        assert_eq!(metadata.actual_model, "gemini-2.5-pro");
        assert!(metadata.stream);
        assert_eq!(metadata.request_output_reservation, 2048);
    }
}
