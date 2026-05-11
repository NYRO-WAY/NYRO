use thiserror::Error;

#[derive(Debug, Error)]
pub enum RateLimitError {
    #[error("upstream rate limit config is invalid: {message}")]
    InvalidConfig { message: String },
    #[error(
        "upstream key pool is exhausted for provider '{provider_name}' model '{model}'{reason}"
    )]
    NoAvailableKey {
        provider_name: String,
        model: String,
        reason: String,
    },
    #[error("upstream rate limit lease not found: {lease_id}")]
    LeaseNotFound { lease_id: String },
    #[error("upstream token estimation failed: {message}")]
    TokenEstimation { message: String },
}

impl RateLimitError {
    pub fn invalid_config(message: impl Into<String>) -> Self {
        Self::InvalidConfig {
            message: message.into(),
        }
    }

    pub fn token_estimation(message: impl Into<String>) -> Self {
        Self::TokenEstimation {
            message: message.into(),
        }
    }
}
