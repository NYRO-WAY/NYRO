use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, clap::ValueEnum)]
pub enum ProtocolKind {
    #[value(name = "openai-chat")]
    OpenAiChat,
    #[value(name = "openai-responses")]
    OpenAiResponses,
    #[value(name = "anthropic-messages")]
    AnthropicMessages,
    #[value(name = "google-content")]
    GoogleContent,
}

impl ProtocolKind {
    pub fn as_short_name(self) -> &'static str {
        match self {
            ProtocolKind::OpenAiChat => "openai-chat",
            ProtocolKind::OpenAiResponses => "openai-responses",
            ProtocolKind::AnthropicMessages => "anthropic-messages",
            ProtocolKind::GoogleContent => "google-content",
        }
    }

    /// Ingress URL path nyro routes to.
    /// `google-content` is special: model is embedded in the path.
    pub fn ingress_path_template(self) -> &'static str {
        match self {
            ProtocolKind::OpenAiChat => "/v1/chat/completions",
            ProtocolKind::OpenAiResponses => "/v1/responses",
            ProtocolKind::AnthropicMessages => "/v1/messages",
            ProtocolKind::GoogleContent => "/v1beta/models/{model}:{action}",
        }
    }

    /// Extract the request model name from path + body, depending on protocol.
    /// For `google-content` the model is in the path; for others it lives in `body.model`.
    pub fn extract_request_model(self, path: &str, body: &Value) -> Result<String> {
        match self {
            ProtocolKind::GoogleContent => parse_google_content_model(path),
            _ => body
                .get("model")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .with_context(|| format!("request body has no `model` field for {self}")),
        }
    }

    /// Suffix appended to the user-supplied `--upstream-endpoint` (which must already
    /// carry the API version segment, e.g. `/v1`, `/v1beta`, `/api/coding/paas/v4`).
    /// Used by `record` to construct the real-LLM URL.
    pub fn record_path_suffix(self, stream: bool, real_model: &str) -> (String, Option<String>) {
        match self {
            ProtocolKind::OpenAiChat => ("/chat/completions".to_string(), None),
            ProtocolKind::OpenAiResponses => ("/responses".to_string(), None),
            ProtocolKind::AnthropicMessages => ("/messages".to_string(), None),
            ProtocolKind::GoogleContent => {
                let action = if stream { "streamGenerateContent" } else { "generateContent" };
                let query = stream.then(|| "alt=sse".to_string());
                (format!("/models/{real_model}:{action}"), query)
            }
        }
    }

    /// Strip the protocol's standard version prefix from an incoming client path.
    /// Used by `proxy` to convert what an OpenAI/Anthropic/Google SDK actually sends
    /// (e.g. `/v1/chat/completions`) into the suffix expected by the user-supplied
    /// versioned `--upstream-endpoint` (e.g. `/chat/completions`).
    pub fn strip_client_version_prefix(self, client_path: &str) -> Result<String> {
        match self {
            ProtocolKind::OpenAiChat => {
                if client_path == "/v1/chat/completions" {
                    Ok("/chat/completions".to_string())
                } else {
                    bail!(
                        "openai-chat client path must be `/v1/chat/completions`, got `{client_path}`"
                    )
                }
            }
            ProtocolKind::OpenAiResponses => {
                if client_path == "/v1/responses" {
                    Ok("/responses".to_string())
                } else {
                    bail!("openai-responses client path must be `/v1/responses`, got `{client_path}`")
                }
            }
            ProtocolKind::AnthropicMessages => {
                if client_path == "/v1/messages" {
                    Ok("/messages".to_string())
                } else {
                    bail!(
                        "anthropic-messages client path must be `/v1/messages`, got `{client_path}`"
                    )
                }
            }
            ProtocolKind::GoogleContent => {
                const PREFIX: &str = "/v1beta";
                let rest = client_path.strip_prefix(PREFIX).with_context(|| {
                    format!(
                        "google-content client path must start with `{PREFIX}/models/...`, got `{client_path}`"
                    )
                })?;
                if !rest.starts_with("/models/") {
                    bail!(
                        "google-content client path must continue with `/models/...`, got `{client_path}`"
                    );
                }
                Ok(rest.to_string())
            }
        }
    }
}

/// Validate that `-e/--upstream-endpoint` carries a non-empty path component.
/// The new (since record/proxy v2) semantic requires the user to embed the API
/// version segment directly in the endpoint URL, so a bare host like
/// `https://api.deepseek.com` is rejected fast.
pub fn validate_upstream_endpoint(url: &url::Url) -> Result<()> {
    let path = url.path();
    if path.is_empty() || path == "/" {
        bail!(
            "--upstream-endpoint must include the API version path \
             (e.g. https://api.deepseek.com/v1, https://open.bigmodel.cn/api/coding/paas/v4, \
             https://generativelanguage.googleapis.com/v1beta); got `{url}`"
        );
    }
    Ok(())
}

impl fmt::Display for ProtocolKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_short_name())
    }
}

impl FromStr for ProtocolKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "openai-chat" => Ok(ProtocolKind::OpenAiChat),
            "openai-responses" => Ok(ProtocolKind::OpenAiResponses),
            "anthropic-messages" => Ok(ProtocolKind::AnthropicMessages),
            "google-content" => Ok(ProtocolKind::GoogleContent),
            other => Err(format!("unknown protocol: {other}")),
        }
    }
}

/// Parse model name out of a google-content path like
/// `/v1beta/models/<model>:generateContent` or `:streamGenerateContent`.
/// The model segment may itself contain `--` (our compound replay_model),
/// the only forbidden character is `:` since that delimits the action.
fn parse_google_content_model(path: &str) -> Result<String> {
    const PREFIX: &str = "/v1beta/models/";
    let rest = path
        .strip_prefix(PREFIX)
        .with_context(|| format!("google-content path missing `{PREFIX}` prefix: {path}"))?;
    let (model, action) = rest
        .split_once(':')
        .with_context(|| format!("google-content path missing `:<action>` segment: {path}"))?;
    if model.is_empty() {
        bail!("google-content path has empty model segment: {path}");
    }
    if !matches!(action, "generateContent" | "streamGenerateContent") {
        bail!("google-content path has unsupported action `{action}` (expected generateContent or streamGenerateContent)");
    }
    Ok(model.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_short_name() {
        for kind in [
            ProtocolKind::OpenAiChat,
            ProtocolKind::OpenAiResponses,
            ProtocolKind::AnthropicMessages,
            ProtocolKind::GoogleContent,
        ] {
            let s = kind.as_short_name();
            assert_eq!(s.parse::<ProtocolKind>().unwrap(), kind);
        }
    }

    #[test]
    fn extract_model_from_body() {
        let body = serde_json::json!({"model": "deepseek--openai-chat--basic-stream"});
        let m = ProtocolKind::OpenAiChat
            .extract_request_model("/v1/chat/completions", &body)
            .unwrap();
        assert_eq!(m, "deepseek--openai-chat--basic-stream");
    }

    #[test]
    fn extract_model_from_google_path() {
        for action in ["generateContent", "streamGenerateContent"] {
            let path = format!("/v1beta/models/google-aistudio--google-content--basic-stream:{action}");
            let m = ProtocolKind::GoogleContent
                .extract_request_model(&path, &serde_json::json!({}))
                .unwrap();
            assert_eq!(m, "google-aistudio--google-content--basic-stream");
        }
    }

    #[test]
    fn rejects_bad_google_path() {
        assert!(parse_google_content_model("/v1/chat/completions").is_err());
        assert!(parse_google_content_model("/v1beta/models/:generateContent").is_err());
        assert!(parse_google_content_model("/v1beta/models/x").is_err());
        assert!(parse_google_content_model("/v1beta/models/x:bogusAction").is_err());
    }

    #[test]
    fn record_suffix_openai_family() {
        let (path, q) = ProtocolKind::OpenAiChat.record_path_suffix(true, "deepseek-chat");
        assert_eq!(path, "/chat/completions");
        assert!(q.is_none());

        let (path, q) = ProtocolKind::OpenAiResponses.record_path_suffix(false, "gpt-4o");
        assert_eq!(path, "/responses");
        assert!(q.is_none());

        let (path, q) = ProtocolKind::AnthropicMessages.record_path_suffix(false, "claude-3-5");
        assert_eq!(path, "/messages");
        assert!(q.is_none());
    }

    #[test]
    fn record_suffix_google_stream_vs_nonstream() {
        let (p, q) = ProtocolKind::GoogleContent.record_path_suffix(true, "gemini-2.0-flash");
        assert_eq!(p, "/models/gemini-2.0-flash:streamGenerateContent");
        assert_eq!(q.as_deref(), Some("alt=sse"));

        let (p, q) = ProtocolKind::GoogleContent.record_path_suffix(false, "gemini-2.0-flash");
        assert_eq!(p, "/models/gemini-2.0-flash:generateContent");
        assert!(q.is_none());
    }

    #[test]
    fn strip_client_version_prefix_happy_path() {
        assert_eq!(
            ProtocolKind::OpenAiChat
                .strip_client_version_prefix("/v1/chat/completions")
                .unwrap(),
            "/chat/completions"
        );
        assert_eq!(
            ProtocolKind::OpenAiResponses
                .strip_client_version_prefix("/v1/responses")
                .unwrap(),
            "/responses"
        );
        assert_eq!(
            ProtocolKind::AnthropicMessages
                .strip_client_version_prefix("/v1/messages")
                .unwrap(),
            "/messages"
        );
        assert_eq!(
            ProtocolKind::GoogleContent
                .strip_client_version_prefix("/v1beta/models/x:generateContent")
                .unwrap(),
            "/models/x:generateContent"
        );
        assert_eq!(
            ProtocolKind::GoogleContent
                .strip_client_version_prefix("/v1beta/models/x:streamGenerateContent")
                .unwrap(),
            "/models/x:streamGenerateContent"
        );
    }

    #[test]
    fn strip_client_version_prefix_rejects_garbage() {
        assert!(
            ProtocolKind::OpenAiChat
                .strip_client_version_prefix("/v2/chat/completions")
                .is_err()
        );
        assert!(
            ProtocolKind::AnthropicMessages
                .strip_client_version_prefix("/messages")
                .is_err()
        );
        assert!(
            ProtocolKind::GoogleContent
                .strip_client_version_prefix("/v1/models/x:generateContent")
                .is_err()
        );
        assert!(
            ProtocolKind::GoogleContent
                .strip_client_version_prefix("/v1beta/foo/x:generateContent")
                .is_err()
        );
    }

    #[test]
    fn validate_upstream_endpoint_requires_path() {
        assert!(
            validate_upstream_endpoint(&url::Url::parse("https://api.deepseek.com").unwrap())
                .is_err()
        );
        assert!(
            validate_upstream_endpoint(&url::Url::parse("https://api.deepseek.com/").unwrap())
                .is_err()
        );
        assert!(
            validate_upstream_endpoint(&url::Url::parse("https://api.deepseek.com/v1").unwrap())
                .is_ok()
        );
        assert!(
            validate_upstream_endpoint(
                &url::Url::parse("https://open.bigmodel.cn/api/coding/paas/v4").unwrap()
            )
            .is_ok()
        );
    }
}
