use std::collections::BTreeMap;
use std::pin::Pin;

use anyhow::{Context, bail};
use async_stream::try_stream;
use async_trait::async_trait;
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use bytes::{Bytes, BytesMut};
use futures_util::{Stream, StreamExt};
use reqwest::Client;
use serde_json::Value;
use tracing::{debug, warn};

use crate::domain::{AuthStrategy, DownstreamRouteKind};
use crate::group::{CandidateStreamFactory, RaceParticipant};

#[derive(Debug, Clone)]
pub struct ReqwestDownstreamDispatcher {
    client: Client,
}

impl ReqwestDownstreamDispatcher {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .tcp_nodelay(true)
                .build()
                .expect("failed to build reqwest client"),
        }
    }

    pub async fn dispatch_stream(
        &self,
        participant: &RaceParticipant,
        route: &DispatchRoute,
        request_headers: &HeaderMap,
        request_body: Bytes,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<Bytes>> + Send>>> {
        let url = build_request_url(
            &participant.endpoint.base_url,
            route,
            &participant.candidate.upstream_model,
        )?;
        let body =
            rewrite_request_body(route, &participant.candidate.upstream_model, request_body)?;
        debug!(
            candidate = %participant.candidate.name,
            protocol = ?route.route_kind,
            upstream_model = %participant.candidate.upstream_model,
            url = %url,
            "dispatching upstream request"
        );

        let mut builder = self.client.post(url);
        builder = copy_forward_headers(
            builder,
            request_headers,
            &participant.endpoint.extra_headers,
        );
        builder = apply_auth(
            builder,
            &participant.endpoint.auth_strategy,
            &participant.selected_key.secret,
        )?;
        if !participant.endpoint.extra_query.is_empty() {
            builder = builder.query(&participant.endpoint.extra_query);
        }

        let response = if let Some(timeout_ms) = participant.endpoint.request_timeout_ms {
            tokio::time::timeout(
                std::time::Duration::from_millis(timeout_ms),
                builder.body(body).send(),
            )
            .await
            .with_context(|| {
                format!(
                    "timed out sending candidate '{}' request after {} ms",
                    participant.candidate.name, timeout_ms
                )
            })?
            .with_context(|| {
                format!(
                    "failed to dispatch candidate '{}'",
                    participant.candidate.name
                )
            })?
        } else {
            builder.body(body).send().await.with_context(|| {
                format!(
                    "failed to dispatch candidate '{}'",
                    participant.candidate.name
                )
            })?
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            warn!(
                candidate = %participant.candidate.name,
                status = %status,
                "upstream candidate returned non-success response"
            );
            bail!(
                "upstream candidate '{}' returned {}: {}",
                participant.candidate.name,
                status,
                body
            );
        }

        let is_sse = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("text/event-stream"));

        let idle_timeout = participant
            .endpoint
            .request_timeout_ms
            .map(std::time::Duration::from_millis);

        let stream = try_stream! {
            let mut bytes_stream = response.bytes_stream();
            let mut buffer = BytesMut::new();

            while let Some(chunk) = next_stream_chunk(&mut bytes_stream, idle_timeout).await? {
                let chunk = chunk.context("failed to read upstream chunk")?;
                if is_sse {
                    buffer.extend_from_slice(&chunk);
                    while let Some(frame_len) = next_sse_frame_len(&buffer) {
                        yield buffer.split_to(frame_len).freeze();
                    }
                } else {
                    yield chunk;
                }
            }

            if !buffer.is_empty() {
                if is_sse && !ends_with_sse_delimiter(&buffer) {
                    buffer.extend_from_slice(b"\n\n");
                }
                yield buffer.freeze();
            }
        };

        Ok(Box::pin(stream))
    }
}

#[derive(Debug, Clone)]
pub struct DispatchRoute {
    pub route_kind: DownstreamRouteKind,
    pub model_action: Option<String>,
}

#[derive(Clone)]
pub struct DownstreamStreamFactory {
    pub dispatcher: ReqwestDownstreamDispatcher,
    pub route: DispatchRoute,
    pub request_headers: HeaderMap,
    pub request_body: Bytes,
}

#[async_trait]
impl CandidateStreamFactory for DownstreamStreamFactory {
    async fn open_stream(
        &self,
        participant: RaceParticipant,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<Bytes>> + Send>>> {
        self.dispatcher
            .dispatch_stream(
                &participant,
                &self.route,
                &self.request_headers,
                self.request_body.clone(),
            )
            .await
    }
}

fn build_request_url(
    base_url: &str,
    route: &DispatchRoute,
    upstream_model: &str,
) -> anyhow::Result<String> {
    let url = match route.route_kind {
        DownstreamRouteKind::OpenAiChatCompletions => format!("{base_url}/chat/completions"),
        DownstreamRouteKind::OpenAiResponses => format!("{base_url}/responses"),
        DownstreamRouteKind::AnthropicMessages => format!("{base_url}/v1/messages"),
        DownstreamRouteKind::GoogleV1BetaModels => format!(
            "{base_url}/v1beta/models/{}",
            replace_google_model_action(route.model_action.as_deref(), upstream_model)
        ),
        DownstreamRouteKind::GoogleV1Models => format!(
            "{base_url}/models/{}",
            replace_google_model_action(route.model_action.as_deref(), upstream_model)
        ),
    };
    Ok(url)
}

fn replace_google_model_action(model_action: Option<&str>, upstream_model: &str) -> String {
    let value = model_action.unwrap_or("streamGenerateContent");
    if let Some((_, action)) = value.split_once(':') {
        format!("{upstream_model}:{action}")
    } else {
        format!("{upstream_model}:{value}")
    }
}

fn rewrite_request_body(
    route: &DispatchRoute,
    upstream_model: &str,
    request_body: Bytes,
) -> anyhow::Result<Bytes> {
    let mut value =
        serde_json::from_slice::<Value>(&request_body).context("invalid json request body")?;
    match route.route_kind {
        DownstreamRouteKind::OpenAiChatCompletions | DownstreamRouteKind::OpenAiResponses => {
            value["model"] = Value::String(upstream_model.to_string());
            value["stream"] = Value::Bool(true);
        }
        DownstreamRouteKind::AnthropicMessages => {
            value["model"] = Value::String(upstream_model.to_string());
            value["stream"] = Value::Bool(true);
        }
        DownstreamRouteKind::GoogleV1BetaModels | DownstreamRouteKind::GoogleV1Models => {}
    }
    Ok(Bytes::from(
        serde_json::to_vec(&value).context("failed to serialize rewritten request body")?,
    ))
}

fn copy_forward_headers(
    mut builder: reqwest::RequestBuilder,
    source: &HeaderMap,
    extra_headers: &BTreeMap<String, String>,
) -> reqwest::RequestBuilder {
    for (name, value) in source {
        if is_hop_by_hop_header(name)
            || name == axum::http::header::AUTHORIZATION
            || name == "x-api-key"
        {
            continue;
        }
        builder = builder.header(name, value);
    }
    for (name, value) in extra_headers {
        if let (Ok(header_name), Ok(header_value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            builder = builder.header(header_name, header_value);
        }
    }
    builder
}

fn apply_auth(
    mut builder: reqwest::RequestBuilder,
    auth_strategy: &AuthStrategy,
    secret: &str,
) -> anyhow::Result<reqwest::RequestBuilder> {
    match auth_strategy {
        AuthStrategy::Bearer => {
            builder = builder.bearer_auth(secret);
        }
        AuthStrategy::HeaderApiKey { header_name } => {
            builder = builder.header(
                HeaderName::from_bytes(header_name.as_bytes())
                    .with_context(|| format!("invalid auth header name '{header_name}'"))?,
                HeaderValue::from_str(secret).context("invalid auth header value")?,
            );
        }
        AuthStrategy::QueryApiKey { parameter_name } => {
            builder = builder.query(&[(parameter_name.as_str(), secret)]);
        }
    }
    Ok(builder)
}

fn is_hop_by_hop_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "host"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "content-length"
    )
}

async fn next_stream_chunk(
    bytes_stream: &mut (impl Stream<Item = Result<Bytes, reqwest::Error>> + Unpin),
    idle_timeout: Option<std::time::Duration>,
) -> anyhow::Result<Option<Result<Bytes, reqwest::Error>>> {
    if let Some(idle_timeout) = idle_timeout {
        tokio::time::timeout(idle_timeout, bytes_stream.next())
            .await
            .with_context(|| {
                format!(
                    "upstream stream stalled for more than {} ms",
                    idle_timeout.as_millis()
                )
            })
    } else {
        Ok(bytes_stream.next().await)
    }
}

fn next_sse_frame_len(buffer: &[u8]) -> Option<usize> {
    let lf = find_bytes(buffer, b"\n\n").map(|index| index + 2);
    let crlf = find_bytes(buffer, b"\r\n\r\n").map(|index| index + 4);
    match (lf, crlf) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn ends_with_sse_delimiter(buffer: &[u8]) -> bool {
    buffer.ends_with(b"\n\n") || buffer.ends_with(b"\r\n\r\n")
}

#[cfg(test)]
mod tests {
    use super::next_sse_frame_len;

    #[test]
    fn detects_lf_delimited_sse_frame() {
        let buffer = b"event: message\ndata: hello\n\nrest";
        assert_eq!(
            next_sse_frame_len(buffer),
            Some("event: message\ndata: hello\n\n".len())
        );
    }

    #[test]
    fn detects_crlf_delimited_sse_frame() {
        let buffer = b"event: message\r\ndata: hello\r\n\r\nrest";
        assert_eq!(
            next_sse_frame_len(buffer),
            Some("event: message\r\ndata: hello\r\n\r\n".len())
        );
    }

    #[test]
    fn prefers_earliest_frame_boundary() {
        let buffer = b"data: one\n\ndata: two\r\n\r\n";
        assert_eq!(next_sse_frame_len(buffer), Some("data: one\n\n".len()));
    }
}
