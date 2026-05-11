use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{Cursor, Read};

use serde::Deserialize;
use serde_json::Value;

use crate::config::ModelRateLimitRule;
use crate::errors::RateLimitError;

const IMAGE_HEADER_READ_LIMIT: usize = 128 * 1024;

pub fn estimate_input_tokens(
    body: &Value,
    actual_model: &str,
    matched_rule: Option<&ModelRateLimitRule>,
) -> Result<u32, RateLimitError> {
    let request = serde_json::from_value::<GoogleRequest>(body.clone()).ok();
    let mut body_text = None;
    let mut max_tokens: Option<usize> = None;

    for model_name in resolve_google_token_count_candidates(actual_model, matched_rule) {
        let Ok(tokenizer) = gemini_tokenizer::LocalTokenizer::new(model_name.as_ref()) else {
            continue;
        };
        let tokens = match request.as_ref() {
            Some(request) => count_google_request_tokens(&tokenizer, request),
            None => {
                tokenizer
                    .count_tokens(
                        body_text.get_or_insert_with(|| body.to_string()).as_str(),
                        None,
                    )
                    .total_tokens
            }
        };
        max_tokens = Some(max_tokens.map_or(tokens, |current| current.max(tokens)));
    }

    let Some(max_tokens) = max_tokens else {
        return Err(RateLimitError::token_estimation(format!(
            "gemini tokenizer unavailable for model '{actual_model}' and no fallback tokenizer model could be loaded"
        )));
    };

    u32::try_from(max_tokens).map_err(|_| {
        RateLimitError::token_estimation(format!(
            "google request is too large to estimate safely for model '{actual_model}'"
        ))
    })
}

fn resolve_google_token_count_candidates<'a>(
    actual_model: &'a str,
    matched_rule: Option<&ModelRateLimitRule>,
) -> Vec<Cow<'a, str>> {
    let normalized = normalize_google_model_name(actual_model);
    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::new();

    if gemini_tokenizer::supported_models().contains(&normalized) {
        seen.insert(normalized.to_string());
        candidates.push(Cow::Borrowed(normalized));
    }

    if let Some(tokenizer_model) = matched_rule
        .and_then(|rule| rule.tokenizer_model.as_deref())
        .map(normalize_google_model_name)
        && seen.insert(tokenizer_model.to_string())
    {
        candidates.push(Cow::Owned(tokenizer_model.to_string()));
    }

    for fallback in google_fallback_tokenizer_models() {
        if seen.insert((*fallback).to_string()) {
            candidates.push(Cow::Borrowed(*fallback));
        }
    }

    candidates
}

fn normalize_google_model_name(actual_model: &str) -> &str {
    let trimmed = actual_model.trim();
    let without_models = trimmed.trim_start_matches("models/");
    without_models
        .rsplit('/')
        .next()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(without_models)
}

fn google_fallback_tokenizer_models() -> &'static [&'static str] {
    &["gemini-2.5-pro", "gemini-2.5-flash", "gemini-3-pro-preview"]
}

fn count_google_request_tokens(
    tokenizer: &gemini_tokenizer::LocalTokenizer,
    request: &GoogleRequest,
) -> usize {
    let (contents, inline_data_tokens) = request
        .contents
        .iter()
        .map(|content| convert_google_content(tokenizer, content))
        .fold(
            (Vec::new(), 0usize),
            |(mut contents, total), (content, extra)| {
                contents.push(content);
                (contents, total + extra)
            },
        );

    let (system_instruction, system_inline_tokens) = request
        .system_instruction
        .as_ref()
        .map(|content| convert_google_content(tokenizer, content))
        .map(|(content, extra)| (Some(content), extra))
        .unwrap_or((None, 0));

    let response_schema = request
        .generation_config
        .as_ref()
        .and_then(|config| config.response_schema.as_ref())
        .and_then(|value| serde_json::from_value::<gemini_tokenizer::Schema>(value.clone()).ok());

    let config = gemini_tokenizer::CountTokensConfig {
        tools: request.tools.as_deref().map(convert_google_tools),
        system_instruction,
        response_schema,
    };

    tokenizer
        .count_tokens(contents.as_slice(), Some(&config))
        .total_tokens
        + inline_data_tokens
        + system_inline_tokens
}

fn convert_google_content(
    tokenizer: &gemini_tokenizer::LocalTokenizer,
    content: &GoogleContent,
) -> (gemini_tokenizer::Content, usize) {
    let (parts, inline_data_tokens) =
        content
            .parts
            .iter()
            .fold((Vec::new(), 0usize), |(mut parts, total), part| {
                let (part, extra) = convert_google_part(tokenizer, part);
                if let Some(part) = part {
                    parts.push(part);
                }
                (parts, total + extra)
            });

    (
        gemini_tokenizer::Content {
            role: content.role.clone(),
            parts: Some(parts),
        },
        inline_data_tokens,
    )
}

fn convert_google_part(
    tokenizer: &gemini_tokenizer::LocalTokenizer,
    part: &GooglePart,
) -> (Option<gemini_tokenizer::Part>, usize) {
    if let Some(text) = &part.text {
        return (
            Some(gemini_tokenizer::Part {
                text: Some(text.clone()),
                ..Default::default()
            }),
            0,
        );
    }

    if let Some(function_call) = &part.function_call {
        return (
            Some(gemini_tokenizer::Part {
                function_call: Some(gemini_tokenizer::FunctionCall {
                    name: Some(function_call.name.clone()),
                    args: json_object_to_hash_map(&function_call.args),
                }),
                ..Default::default()
            }),
            0,
        );
    }

    if let Some(function_response) = &part.function_response {
        return (
            Some(gemini_tokenizer::Part {
                function_response: Some(gemini_tokenizer::FunctionResponse {
                    name: Some(function_response.name.clone()),
                    response: json_object_to_hash_map(&function_response.response),
                }),
                ..Default::default()
            }),
            0,
        );
    }

    if let Some(inline_data) = &part.inline_data {
        return (
            None,
            count_google_inline_data_tokens(tokenizer, &inline_data),
        );
    }

    (None, 0)
}

fn convert_google_tools(tools: &[GoogleTool]) -> Vec<gemini_tokenizer::Tool> {
    tools
        .iter()
        .map(|tool| gemini_tokenizer::Tool {
            function_declarations: Some(
                tool.function_declarations
                    .iter()
                    .map(convert_google_function_decl)
                    .collect(),
            ),
        })
        .collect()
}

fn convert_google_function_decl(
    decl: &GoogleFunctionDecl,
) -> gemini_tokenizer::FunctionDeclaration {
    gemini_tokenizer::FunctionDeclaration {
        name: Some(decl.name.clone()),
        description: decl.description.clone(),
        parameters: decl.parameters.as_ref().and_then(|value| {
            serde_json::from_value::<gemini_tokenizer::Schema>(value.clone()).ok()
        }),
        response: None,
    }
}

fn json_object_to_hash_map(
    value: &serde_json::Map<String, Value>,
) -> Option<HashMap<String, Value>> {
    if value.is_empty() {
        None
    } else {
        Some(
            value
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        )
    }
}

fn count_google_inline_data_tokens(
    tokenizer: &gemini_tokenizer::LocalTokenizer,
    inline_data: &GoogleInlineData,
) -> usize {
    let mime_type = inline_data.mime_type.trim().to_ascii_lowercase();
    let modality_estimate = if mime_type.starts_with("image/") {
        image_dimensions_from_base64_payload(&mime_type, &inline_data.data)
            .map(count_google_image_inline_data_tokens)
            .unwrap_or(0)
    } else {
        0
    };

    let fallback_tokens = tokenizer
        .count_tokens(inline_data.data.as_str(), None)
        .total_tokens
        + tokenizer
            .count_tokens(inline_data.mime_type.as_str(), None)
            .total_tokens;

    modality_estimate.max(fallback_tokens)
}

fn count_google_image_inline_data_tokens(dimensions: ImageDimensions) -> usize {
    let width = dimensions.width.max(1);
    let height = dimensions.height.max(1);

    if width <= 384 && height <= 384 {
        return 258;
    }

    let tiles_w = ceil_div(width, 768) as usize;
    let tiles_h = ceil_div(height, 768) as usize;
    tiles_w * tiles_h * 258
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ImageDimensions {
    width: u32,
    height: u32,
}

fn image_dimensions_from_base64_payload(
    media_type: &str,
    payload: &str,
) -> Option<ImageDimensions> {
    let bytes = decode_base64_prefix(payload, IMAGE_HEADER_READ_LIMIT)?;
    image_dimensions_from_bytes(media_type, &bytes)
        .or_else(|| sniff_image_dimensions_from_bytes(&bytes))
}

fn decode_base64_prefix(payload: &str, max_decoded: usize) -> Option<Vec<u8>> {
    let reader = base64::read::DecoderReader::new(
        Cursor::new(payload.as_bytes()),
        &base64::engine::general_purpose::STANDARD,
    );
    let mut bytes = Vec::new();
    reader
        .take(max_decoded as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes)
}

fn image_dimensions_from_bytes(media_type: &str, bytes: &[u8]) -> Option<ImageDimensions> {
    match media_type.trim().to_ascii_lowercase().as_str() {
        "image/png" => parse_png_dimensions(bytes),
        "image/jpeg" | "image/jpg" => parse_jpeg_dimensions(bytes),
        "image/gif" => parse_gif_dimensions(bytes),
        "image/webp" => parse_webp_dimensions(bytes),
        _ => None,
    }
}

fn sniff_image_dimensions_from_bytes(bytes: &[u8]) -> Option<ImageDimensions> {
    parse_png_dimensions(bytes)
        .or_else(|| parse_jpeg_dimensions(bytes))
        .or_else(|| parse_gif_dimensions(bytes))
        .or_else(|| parse_webp_dimensions(bytes))
}

fn parse_png_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }

    Some(ImageDimensions {
        width: u32::from_be_bytes(bytes[16..20].try_into().ok()?),
        height: u32::from_be_bytes(bytes[20..24].try_into().ok()?),
    })
}

fn parse_gif_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    if bytes.len() < 10 || (&bytes[..6] != b"GIF87a" && &bytes[..6] != b"GIF89a") {
        return None;
    }

    Some(ImageDimensions {
        width: u16::from_le_bytes(bytes[6..8].try_into().ok()?) as u32,
        height: u16::from_le_bytes(bytes[8..10].try_into().ok()?) as u32,
    })
}

fn parse_jpeg_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }

    let mut index = 2usize;
    while index + 9 < bytes.len() {
        if bytes[index] != 0xFF {
            index += 1;
            continue;
        }

        let marker = bytes[index + 1];
        index += 2;

        if marker == 0xD8 || marker == 0xD9 {
            continue;
        }
        if marker == 0xDA || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }

        if index + 2 > bytes.len() {
            return None;
        }
        let segment_len = u16::from_be_bytes(bytes[index..index + 2].try_into().ok()?) as usize;
        if segment_len < 2 || index + segment_len > bytes.len() {
            return None;
        }

        if matches!(
            marker,
            0xC0 | 0xC1
                | 0xC2
                | 0xC3
                | 0xC5
                | 0xC6
                | 0xC7
                | 0xC9
                | 0xCA
                | 0xCB
                | 0xCD
                | 0xCE
                | 0xCF
        ) && segment_len >= 7
        {
            let height = u16::from_be_bytes(bytes[index + 3..index + 5].try_into().ok()?) as u32;
            let width = u16::from_be_bytes(bytes[index + 5..index + 7].try_into().ok()?) as u32;
            return Some(ImageDimensions { width, height });
        }

        index += segment_len;
    }

    None
}

fn parse_webp_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    if bytes.len() < 16 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return None;
    }

    match &bytes[12..16] {
        b"VP8X" if bytes.len() >= 30 => {
            let width = 1 + u32::from_le_bytes([bytes[24], bytes[25], bytes[26], 0]);
            let height = 1 + u32::from_le_bytes([bytes[27], bytes[28], bytes[29], 0]);
            Some(ImageDimensions { width, height })
        }
        b"VP8 " if bytes.len() >= 30 => {
            if bytes[23] != 0x9D || bytes[24] != 0x01 || bytes[25] != 0x2A {
                return None;
            }
            let width = u16::from_le_bytes(bytes[26..28].try_into().ok()?) as u32 & 0x3FFF;
            let height = u16::from_le_bytes(bytes[28..30].try_into().ok()?) as u32 & 0x3FFF;
            Some(ImageDimensions { width, height })
        }
        b"VP8L" if bytes.len() >= 25 => {
            if bytes[20] != 0x2F {
                return None;
            }
            let bits = u32::from_le_bytes(bytes[21..25].try_into().ok()?);
            let width = (bits & 0x3FFF) + 1;
            let height = ((bits >> 14) & 0x3FFF) + 1;
            Some(ImageDimensions { width, height })
        }
        _ => None,
    }
}

fn ceil_div(value: u32, divisor: u32) -> u32 {
    value.div_ceil(divisor)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleRequest {
    #[serde(default)]
    contents: Vec<GoogleContent>,
    #[serde(default)]
    system_instruction: Option<GoogleContent>,
    #[serde(default)]
    tools: Option<Vec<GoogleTool>>,
    #[serde(default)]
    generation_config: Option<GoogleGenerationConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleGenerationConfig {
    #[serde(default)]
    response_schema: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleContent {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    parts: Vec<GooglePart>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GooglePart {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    function_call: Option<GoogleFunctionCall>,
    #[serde(default)]
    function_response: Option<GoogleFunctionResponse>,
    #[serde(default)]
    inline_data: Option<GoogleInlineData>,
}

#[derive(Debug, Clone, Deserialize)]
struct GoogleFunctionCall {
    name: String,
    #[serde(default)]
    args: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct GoogleFunctionResponse {
    name: String,
    #[serde(default)]
    response: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleInlineData {
    mime_type: String,
    data: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleTool {
    #[serde(default)]
    function_declarations: Vec<GoogleFunctionDecl>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleFunctionDecl {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    parameters: Option<Value>,
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use serde_json::json;

    use super::{GoogleRequest, count_google_request_tokens, estimate_input_tokens};
    use crate::config::ModelRateLimitRule;

    fn fake_png_bytes(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 2, 0, 0, 0]);
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(b"IEND");
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes
    }

    fn fake_png_base64(width: u32, height: u32) -> String {
        base64::engine::general_purpose::STANDARD.encode(fake_png_bytes(width, height))
    }

    #[test]
    fn structured_google_count_uses_gemini_tokenizer() {
        let body = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [
                        { "text": "What is the weather in Shanghai?" },
                        {
                            "functionCall": {
                                "name": "lookup_weather",
                                "args": {
                                    "city": "Shanghai",
                                    "units": "metric"
                                }
                            }
                        }
                    ]
                }
            ],
            "tools": [
                {
                    "functionDeclarations": [
                        {
                            "name": "lookup_weather",
                            "description": "Look up weather",
                            "parameters": {
                                "type": "OBJECT",
                                "properties": {
                                    "city": { "type": "STRING" }
                                }
                            }
                        }
                    ]
                }
            ]
        });

        let structured = estimate_input_tokens(&body, "gemini-2.5-pro", None).unwrap();
        let tokenizer = gemini_tokenizer::LocalTokenizer::new("gemini-2.5-pro").unwrap();
        let request = serde_json::from_value::<GoogleRequest>(body.clone()).unwrap();
        let expected = count_google_request_tokens(&tokenizer, &request) as u32;

        assert_eq!(structured, expected);
    }

    #[test]
    fn inline_data_adds_conservative_tokens() {
        let image_data = fake_png_base64(1000, 1000);
        let with_image = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [
                        { "text": "Describe the image." },
                        {
                            "inlineData": {
                                "mimeType": "image/png",
                                "data": image_data
                            }
                        }
                    ]
                }
            ]
        });
        let without_image = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [
                        { "text": "Describe the image." }
                    ]
                }
            ]
        });

        let with_tokens = estimate_input_tokens(&with_image, "gemini-2.5-pro", None).unwrap();
        let without_tokens = estimate_input_tokens(&without_image, "gemini-2.5-pro", None).unwrap();
        assert!(with_tokens > without_tokens);
    }

    #[test]
    fn unknown_model_can_use_rule_tokenizer_model_fallback() {
        let body = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [{ "text": "Hello Gemini" }]
                }
            ]
        });
        let rule = ModelRateLimitRule {
            model: "*".to_string(),
            rpm: None,
            rpd: None,
            tpm: Some(1000),
            tpm_mode: None,
            tokenizer_encoding: None,
            tokenizer_model: Some("gemini-2.5-pro".to_string()),
        };

        let tokens = estimate_input_tokens(&body, "unknown-gemini-model", Some(&rule)).unwrap();
        assert!(tokens > 0);
    }
}
