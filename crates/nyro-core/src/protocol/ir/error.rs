//! `AiError` / `AiErrorKind` — unified cross-protocol error IR.
//!
//! All codec parsers and the dispatcher normalize upstream errors to `AiError`.
//! The `AiErrorKind::is_retryable()` method drives retry and circuit-breaker
//! decisions in the dispatcher (PR-5).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// Normalized error classification across all supported LLM protocols.
///
/// Use `is_retryable()` to determine whether the gateway may automatically
/// retry a request after receiving this error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiErrorKind {
    /// 401 — invalid API key or expired token.
    AuthenticationError,
    /// 403 — account lacks permission for the requested operation.
    AuthorizationError,
    /// 404 — model or resource not found.
    NotFoundError,
    /// 429 (rate-limit) — requests-per-minute or tokens-per-minute exceeded.
    RateLimitError,
    /// 429 (quota) / 529 — spend or usage quota exhausted; not retryable.
    QuotaExceeded,
    /// 400 — malformed request body, unsupported parameters, or schema error.
    InvalidRequest,
    /// 500 — provider-side internal error.
    ServerError,
    /// 503 — provider temporarily unavailable.
    ServiceUnavailable,
    /// 408 / 504 — upstream timed out.
    Timeout,
    /// 200 — response was blocked by content filtering
    /// (`promptFeedback.blockReason` for Google, `content_filter` for OpenAI).
    ContentFiltered,
    /// 400 — request exceeds the model's context window.
    ContextLengthExceeded,
    /// 404 / 503 — model is temporarily unavailable; may be retried.
    ModelNotAvailable,
    /// Mid-stream SSE error detected by the stream parser.
    StreamMidError,
    /// Stream was truncated (no `[DONE]` sentinel received).
    UnexpectedEof,
    /// Any other error that could not be classified.
    Unknown,
}

impl AiErrorKind {
    /// Returns `true` if the gateway *may* automatically retry after this error.
    ///
    /// Retryable errors are transient by nature; non-retryable errors indicate
    /// a permanent failure for this particular request.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimitError
                | Self::ServerError
                | Self::ServiceUnavailable
                | Self::Timeout
                | Self::ModelNotAvailable
                | Self::UnexpectedEof
                | Self::StreamMidError
        )
    }
}

impl fmt::Display for AiErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::AuthenticationError => "authentication_error",
            Self::AuthorizationError => "authorization_error",
            Self::NotFoundError => "not_found_error",
            Self::RateLimitError => "rate_limit_error",
            Self::QuotaExceeded => "quota_exceeded",
            Self::InvalidRequest => "invalid_request",
            Self::ServerError => "server_error",
            Self::ServiceUnavailable => "service_unavailable",
            Self::Timeout => "timeout",
            Self::ContentFiltered => "content_filtered",
            Self::ContextLengthExceeded => "context_length_exceeded",
            Self::ModelNotAvailable => "model_not_available",
            Self::StreamMidError => "stream_mid_error",
            Self::UnexpectedEof => "unexpected_eof",
            Self::Unknown => "unknown",
        };
        f.write_str(s)
    }
}

/// Cross-protocol normalized AI error.
///
/// Produced by codec parsers and the dispatcher when an upstream call fails.
/// Always carries a `kind` for retry / circuit-breaker decisions.  The `raw`
/// field preserves the original vendor error body for logging and passthrough.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiError {
    pub kind: AiErrorKind,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    /// Original vendor error body, preserved verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
}

impl AiError {
    pub fn new(kind: AiErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            status_code: None,
            raw: None,
        }
    }

    pub fn with_status(mut self, status: u16) -> Self {
        self.status_code = Some(status);
        self
    }

    pub fn with_raw(mut self, raw: Value) -> Self {
        self.raw = Some(raw);
        self
    }

    pub fn is_retryable(&self) -> bool {
        self.kind.is_retryable()
    }

    /// Construct an `AiError` from an HTTP status code.
    ///
    /// The caller should override the `kind` if the response body provides more
    /// specific information (e.g. OpenAI `error.type = "context_length_exceeded"`).
    pub fn from_status(status: u16, message: impl Into<String>) -> Self {
        let kind = match status {
            401 => AiErrorKind::AuthenticationError,
            403 => AiErrorKind::AuthorizationError,
            404 => AiErrorKind::NotFoundError,
            408 | 504 => AiErrorKind::Timeout,
            429 => AiErrorKind::RateLimitError,
            500 => AiErrorKind::ServerError,
            503 | 529 => AiErrorKind::ServiceUnavailable,
            _ => AiErrorKind::Unknown,
        };
        Self::new(kind, message).with_status(status)
    }
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.kind, self.message)
    }
}

impl std::error::Error for AiError {}
