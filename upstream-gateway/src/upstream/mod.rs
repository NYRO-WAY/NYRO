use axum::body::Bytes;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use reqwest::{Method, Url};
use serde::Serialize;

use crate::provider::{GatewayAuthStrategy, GatewayProvider};
use crate::runtime::SelectedUpstreamKey;

const ANTHROPIC_VERSION: &str = "2023-06-01";
const SSE_ALT: &str = "sse";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamRouteKind {
    OpenAIChatCompletions,
    OpenAIResponses,
    OpenAIEmbeddings,
    AnthropicMessages,
    GoogleV1BetaModels,
    GoogleModels,
}

#[derive(Debug, Clone)]
pub struct OutboundRequestParts {
    pub route_kind: UpstreamRouteKind,
    pub model_action: Option<String>,
    pub stream: bool,
    pub body: Bytes,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeaderPreview {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutboundRequestPreview {
    pub method: String,
    pub url: String,
    pub headers: Vec<HeaderPreview>,
    pub body_len: usize,
}

pub fn build_upstream_request(
    client: &reqwest::Client,
    provider: &GatewayProvider,
    selected_key: &SelectedUpstreamKey,
    parts: OutboundRequestParts,
) -> anyhow::Result<reqwest::Request> {
    let url = build_upstream_url(
        provider,
        selected_key,
        parts.route_kind,
        parts.model_action.as_deref(),
    )?;
    let headers = build_upstream_headers(provider, selected_key, parts.route_kind, parts.stream)?;

    let request = client
        .request(Method::POST, url)
        .headers(headers)
        .body(parts.body)
        .build()?;

    Ok(request)
}

pub fn preview_request(request: &reqwest::Request) -> OutboundRequestPreview {
    let mut headers = request
        .headers()
        .iter()
        .map(|(name, value)| HeaderPreview {
            name: name.as_str().to_string(),
            value: mask_header_value(name, value),
        })
        .collect::<Vec<_>>();
    headers.sort_by(|left, right| left.name.cmp(&right.name));

    OutboundRequestPreview {
        method: request.method().as_str().to_string(),
        url: request.url().to_string(),
        headers,
        body_len: request
            .body()
            .and_then(|body| body.as_bytes())
            .map(|bytes| bytes.len())
            .unwrap_or(0),
    }
}

fn build_upstream_url(
    provider: &GatewayProvider,
    selected_key: &SelectedUpstreamKey,
    route_kind: UpstreamRouteKind,
    model_action: Option<&str>,
) -> anyhow::Result<Url> {
    let mut url = Url::parse(&provider.base_url)?;
    let path = match route_kind {
        UpstreamRouteKind::OpenAIChatCompletions => "/v1/chat/completions".to_string(),
        UpstreamRouteKind::OpenAIResponses => "/v1/responses".to_string(),
        UpstreamRouteKind::OpenAIEmbeddings => "/v1/embeddings".to_string(),
        UpstreamRouteKind::AnthropicMessages => "/v1/messages".to_string(),
        UpstreamRouteKind::GoogleV1BetaModels => {
            format!("/v1beta/models/{}", required_model_action(model_action)?)
        }
        UpstreamRouteKind::GoogleModels => {
            format!("/models/{}", required_model_action(model_action)?)
        }
    };
    url.set_path(&path);

    if is_google_stream_route(route_kind, model_action) {
        url.query_pairs_mut().append_pair("alt", SSE_ALT);
    }
    if let GatewayAuthStrategy::QueryApiKey { parameter_name } = &provider.auth_strategy {
        url.query_pairs_mut()
            .append_pair(parameter_name, &selected_key.api_key);
    }

    Ok(url)
}

fn build_upstream_headers(
    provider: &GatewayProvider,
    selected_key: &SelectedUpstreamKey,
    route_kind: UpstreamRouteKind,
    stream: bool,
) -> anyhow::Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    match &provider.auth_strategy {
        GatewayAuthStrategy::Bearer => {
            let value = HeaderValue::from_str(&format!("Bearer {}", selected_key.api_key))?;
            headers.insert(AUTHORIZATION, value);
        }
        GatewayAuthStrategy::HeaderApiKey { header_name } => {
            let name = HeaderName::from_bytes(header_name.as_bytes())?;
            let value = HeaderValue::from_str(&selected_key.api_key)?;
            headers.insert(name, value);
        }
        GatewayAuthStrategy::QueryApiKey { .. } => {}
    }

    if matches!(route_kind, UpstreamRouteKind::AnthropicMessages) {
        headers.insert(
            HeaderName::from_static("anthropic-version"),
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );
    }
    if stream {
        headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
    }

    Ok(headers)
}

fn required_model_action(model_action: Option<&str>) -> anyhow::Result<&str> {
    model_action
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("model_action is required for google upstream routes"))
}

fn is_google_stream_route(route_kind: UpstreamRouteKind, model_action: Option<&str>) -> bool {
    matches!(
        route_kind,
        UpstreamRouteKind::GoogleV1BetaModels | UpstreamRouteKind::GoogleModels
    ) && model_action
        .map(|value| value.contains(":streamGenerateContent"))
        .unwrap_or(false)
}

fn mask_header_value(name: &HeaderName, value: &HeaderValue) -> String {
    let raw = value.to_str().unwrap_or("<binary>");
    if name == AUTHORIZATION || name.as_str().eq_ignore_ascii_case("x-api-key") {
        return mask_secret(raw);
    }
    raw.to_string()
}

fn mask_secret(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(secret) = trimmed.strip_prefix("Bearer ") {
        return format!("Bearer {}", short_mask(secret));
    }
    short_mask(trimmed)
}

fn short_mask(raw: &str) -> String {
    if raw.len() <= 8 {
        return "********".to_string();
    }
    format!("{}***{}", &raw[..4], &raw[raw.len() - 4..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{GatewayAuthStrategy, GatewayProvider, ProviderVendor};
    use crate::runtime::{RateLimitLease, SelectedUpstreamKey};

    fn sample_provider(
        vendor: ProviderVendor,
        base_url: &str,
        auth_strategy: GatewayAuthStrategy,
    ) -> GatewayProvider {
        GatewayProvider {
            id: "provider-1".to_string(),
            name: "Provider 1".to_string(),
            vendor,
            base_url: base_url.to_string(),
            auth_strategy,
            enabled: true,
        }
    }

    fn sample_key(api_key: &str) -> SelectedUpstreamKey {
        SelectedUpstreamKey {
            key_id: "key-a".to_string(),
            api_key: api_key.to_string(),
            lease: RateLimitLease {
                lease_id: "lease-1".to_string(),
                provider_id: "provider-1".to_string(),
                key_id: "key-a".to_string(),
                model: "model-1".to_string(),
                reserved_input_tokens: 0,
                reserved_output_tokens: 0,
                tpm_mode: crate::config::TpmMode::InputOnly,
                request_started_at_ms: 0,
            },
        }
    }

    #[test]
    fn builds_openai_bearer_request() {
        let provider = sample_provider(
            ProviderVendor::OpenAI,
            "https://api.openai.com/",
            GatewayAuthStrategy::Bearer,
        );
        let request = build_upstream_request(
            &reqwest::Client::new(),
            &provider,
            &sample_key("sk-openai-secret"),
            OutboundRequestParts {
                route_kind: UpstreamRouteKind::OpenAIChatCompletions,
                model_action: None,
                stream: false,
                body: Bytes::from_static(br#"{"model":"gpt-4o"}"#),
            },
        )
        .unwrap();

        assert_eq!(
            request.url().as_str(),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            request.headers().get(AUTHORIZATION).unwrap(),
            "Bearer sk-openai-secret"
        );
    }

    #[test]
    fn builds_anthropic_request_with_version_header() {
        let provider = sample_provider(
            ProviderVendor::Anthropic,
            "https://api.anthropic.com",
            GatewayAuthStrategy::HeaderApiKey {
                header_name: "x-api-key".to_string(),
            },
        );
        let request = build_upstream_request(
            &reqwest::Client::new(),
            &provider,
            &sample_key("sk-ant-secret"),
            OutboundRequestParts {
                route_kind: UpstreamRouteKind::AnthropicMessages,
                model_action: None,
                stream: true,
                body: Bytes::from_static(br#"{"model":"claude-sonnet-4-20250514"}"#),
            },
        )
        .unwrap();

        assert_eq!(
            request.url().as_str(),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(request.headers().get("x-api-key").unwrap(), "sk-ant-secret");
        assert_eq!(
            request.headers().get("anthropic-version").unwrap(),
            ANTHROPIC_VERSION
        );
        assert_eq!(request.headers().get(ACCEPT).unwrap(), "text/event-stream");
    }

    #[test]
    fn builds_google_stream_request_with_query_key_and_alt_sse() {
        let provider = sample_provider(
            ProviderVendor::Gemini,
            "https://generativelanguage.googleapis.com",
            GatewayAuthStrategy::QueryApiKey {
                parameter_name: "key".to_string(),
            },
        );
        let request = build_upstream_request(
            &reqwest::Client::new(),
            &provider,
            &sample_key("gem-secret-1234"),
            OutboundRequestParts {
                route_kind: UpstreamRouteKind::GoogleV1BetaModels,
                model_action: Some("gemini-2.5-pro:streamGenerateContent".to_string()),
                stream: true,
                body: Bytes::from_static(br#"{"contents":[]}"#),
            },
        )
        .unwrap();

        let url = request.url().as_str();
        assert!(url.starts_with("https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:streamGenerateContent?"));
        assert!(url.contains("alt=sse"));
        assert!(url.contains("key=gem-secret-1234"));
        assert_eq!(request.headers().get(ACCEPT).unwrap(), "text/event-stream");
    }

    #[test]
    fn preview_masks_authorization_header() {
        let provider = sample_provider(
            ProviderVendor::OpenAI,
            "https://api.openai.com",
            GatewayAuthStrategy::Bearer,
        );
        let request = build_upstream_request(
            &reqwest::Client::new(),
            &provider,
            &sample_key("sk-preview-secret"),
            OutboundRequestParts {
                route_kind: UpstreamRouteKind::OpenAIResponses,
                model_action: None,
                stream: false,
                body: Bytes::from_static(br#"{"model":"gpt-4.1"}"#),
            },
        )
        .unwrap();

        let preview = preview_request(&request);
        let auth = preview
            .headers
            .iter()
            .find(|header| header.name == "authorization")
            .unwrap();
        assert_eq!(auth.value, "Bearer sk-p***cret");
    }
}
