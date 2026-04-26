use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use chrono::{Duration, Utc};
use rand::RngCore;
use reqwest::Url;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::auth::types::{AuthExchangeInput, AuthSession};

const PROVIDER_PRESETS_SNAPSHOT: &str = include_str!("../../../assets/providers.json");

#[derive(Debug, Deserialize)]
struct PresetIndex {
    id: String,
    #[serde(default)]
    channels: Vec<ChannelIndex>,
}

#[derive(Debug, Deserialize)]
struct ChannelIndex {
    id: String,
    #[serde(flatten)]
    raw: Value,
}

/// Locate a single channel from providers.json by preset id + channel id,
/// then deserialize its content into the caller's typed struct.
/// Each driver calls this instead of parsing the entire JSON with its own types.
pub fn load_channel_config<T: DeserializeOwned>(
    preset_id: &str,
    channel_id: &str,
) -> Result<T> {
    let presets: Vec<PresetIndex> = serde_json::from_str(PROVIDER_PRESETS_SNAPSHOT)
        .context("parse provider presets index")?;
    let preset = presets
        .into_iter()
        .find(|p| p.id == preset_id)
        .ok_or_else(|| anyhow!("missing provider preset: {preset_id}"))?;
    let channel = preset
        .channels
        .into_iter()
        .find(|c| c.id == channel_id)
        .ok_or_else(|| anyhow!("missing provider channel: {preset_id}/{channel_id}"))?;
    serde_json::from_value(channel.raw)
        .with_context(|| format!("parse channel config: {preset_id}/{channel_id}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PkceAuthState {
    pub code_verifier: String,
    pub state: String,
    pub redirect_uri: String,
}

#[derive(Debug, Clone, Default)]
pub struct OAuthCallbackPayload {
    pub code: Option<String>,
    pub state: Option<String>,
}

pub fn expires_at_after(seconds: i64) -> String {
    (Utc::now() + Duration::seconds(seconds.max(1))).to_rfc3339()
}

pub fn encode_scopes(scope: Option<&str>) -> Vec<String> {
    scope
        .unwrap_or("")
        .split_whitespace()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub fn generate_code_verifier() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn generate_code_challenge(code_verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize())
}

pub fn generate_state() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn parse_session_state<T: DeserializeOwned>(session: &AuthSession) -> Result<T> {
    let raw = session
        .state_json
        .as_deref()
        .context("auth session missing state_json")?;
    serde_json::from_str(raw).context("parse auth session state")
}

pub fn parse_oauth_callback(input: &AuthExchangeInput) -> Result<OAuthCallbackPayload> {
    let mut payload = OAuthCallbackPayload::default();

    if let Some(raw_callback) = input.callback_url.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        let parsed = parse_callback_like_value(raw_callback);
        if payload.code.is_none() {
            payload.code = parsed.code;
        }
        if payload.state.is_none() {
            payload.state = parsed.state;
        }
    }

    if let Some(raw_code) = input.code.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        let parsed = parse_callback_like_value(raw_code);
        if payload.code.is_none() {
            payload.code = parsed.code.or_else(|| Some(raw_code.split('#').next().unwrap_or(raw_code).to_string()));
        }
        if payload.state.is_none() {
            payload.state = parsed.state;
        }
    }

    if payload.code.as_deref().map(str::trim).filter(|v| !v.is_empty()).is_none() {
        bail!("missing authorization code");
    }

    Ok(payload)
}

pub fn validate_callback_state(expected_state: &str, actual_state: Option<&str>, provider: &str) -> Result<()> {
    if expected_state.trim().is_empty() {
        return Ok(());
    }
    let Some(actual_state) = actual_state.map(str::trim).filter(|v| !v.is_empty()) else {
        bail!("{provider} OAuth state is missing");
    };
    if actual_state != expected_state {
        bail!("{provider} OAuth state mismatch");
    }
    Ok(())
}

pub fn build_authorize_url(base_url: &str, params: &[(&str, &str)]) -> Result<String> {
    let mut url = Url::parse(base_url).with_context(|| format!("parse authorize url: {base_url}"))?;
    {
        let mut pairs = url.query_pairs_mut();
        for (key, value) in params {
            pairs.append_pair(key, value);
        }
    }
    Ok(url.to_string())
}

fn parse_callback_like_value(raw: &str) -> OAuthCallbackPayload {
    if let Ok(url) = Url::parse(raw) {
        let mut payload = OAuthCallbackPayload::default();
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "code" if payload.code.is_none() => payload.code = Some(value.to_string()),
                "state" if payload.state.is_none() => payload.state = Some(value.to_string()),
                _ => {}
            }
        }
        if let Some(fragment) = url.fragment() {
            let fragment_url = format!("https://callback.local/?{fragment}");
            if let Ok(fragment_parsed) = Url::parse(&fragment_url) {
                for (key, value) in fragment_parsed.query_pairs() {
                    match key.as_ref() {
                        "code" if payload.code.is_none() => payload.code = Some(value.to_string()),
                        "state" if payload.state.is_none() => payload.state = Some(value.to_string()),
                        _ => {}
                    }
                }
            }
        }
        return payload;
    }

    if raw.contains("code=") || raw.contains("state=") {
        let normalized = if raw.starts_with('?') || raw.starts_with('#') {
            format!("https://callback.local/{raw}")
        } else {
            format!("https://callback.local/?{raw}")
        };
        if let Ok(url) = Url::parse(&normalized) {
            let mut payload = OAuthCallbackPayload::default();
            for (key, value) in url.query_pairs() {
                match key.as_ref() {
                    "code" if payload.code.is_none() => payload.code = Some(value.to_string()),
                    "state" if payload.state.is_none() => payload.state = Some(value.to_string()),
                    _ => {}
                }
            }
            return payload;
        }
    }

    OAuthCallbackPayload::default()
}

pub fn required_http_client(client: Option<reqwest::Client>) -> Result<reqwest::Client> {
    client.ok_or_else(|| anyhow!("missing auth http client"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_verifier_is_43_chars_base64url() {
        let v = generate_code_verifier();
        assert_eq!(v.len(), 43);
        assert!(v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn code_challenge_is_sha256_of_verifier() {
        let verifier = "test-verifier-value";
        let challenge = generate_code_challenge(verifier);
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize());
        assert_eq!(challenge, expected);
    }

    #[test]
    fn state_is_43_chars_base64url() {
        let s = generate_state();
        assert_eq!(s.len(), 43);
    }

    #[test]
    fn encode_scopes_splits_and_filters() {
        assert_eq!(encode_scopes(Some("a b  c")), vec!["a", "b", "c"]);
        assert_eq!(encode_scopes(Some("")), Vec::<String>::new());
        assert_eq!(encode_scopes(None), Vec::<String>::new());
        assert_eq!(encode_scopes(Some("single")), vec!["single"]);
    }

    #[test]
    fn build_authorize_url_appends_params() {
        let url = build_authorize_url("https://example.com/auth", &[("a", "1"), ("b", "2")]).unwrap();
        assert!(url.starts_with("https://example.com/auth?"));
        assert!(url.contains("a=1"));
        assert!(url.contains("b=2"));
    }

    #[test]
    fn parse_callback_from_url() {
        let input = AuthExchangeInput {
            callback_url: Some("https://cb.local/?code=abc123&state=xyz".into()),
            ..Default::default()
        };
        let p = parse_oauth_callback(&input).unwrap();
        assert_eq!(p.code.as_deref(), Some("abc123"));
        assert_eq!(p.state.as_deref(), Some("xyz"));
    }

    #[test]
    fn parse_callback_from_raw_code() {
        let input = AuthExchangeInput {
            code: Some("raw-code-value".into()),
            ..Default::default()
        };
        let p = parse_oauth_callback(&input).unwrap();
        assert_eq!(p.code.as_deref(), Some("raw-code-value"));
    }

    #[test]
    fn parse_callback_from_fragment() {
        let input = AuthExchangeInput {
            callback_url: Some("https://cb.local/#code=frag_code&state=frag_state".into()),
            ..Default::default()
        };
        let p = parse_oauth_callback(&input).unwrap();
        assert_eq!(p.code.as_deref(), Some("frag_code"));
        assert_eq!(p.state.as_deref(), Some("frag_state"));
    }

    #[test]
    fn parse_callback_missing_code_errors() {
        let input = AuthExchangeInput::default();
        assert!(parse_oauth_callback(&input).is_err());
    }

    #[test]
    fn validate_state_match() {
        assert!(validate_callback_state("abc", Some("abc"), "test").is_ok());
    }

    #[test]
    fn validate_state_mismatch() {
        assert!(validate_callback_state("abc", Some("xyz"), "test").is_err());
    }

    #[test]
    fn validate_state_empty_expected_skips() {
        assert!(validate_callback_state("", None, "test").is_ok());
    }

    #[test]
    fn validate_state_missing_actual_errors() {
        assert!(validate_callback_state("abc", None, "test").is_err());
    }

    #[test]
    fn expires_at_after_returns_rfc3339() {
        let ts = expires_at_after(3600);
        assert!(chrono::DateTime::parse_from_rfc3339(&ts).is_ok());
    }
}
