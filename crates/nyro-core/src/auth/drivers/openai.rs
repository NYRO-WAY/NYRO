use std::collections::HashMap;

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use base64::Engine;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::shared::{
    PkceAuthState, build_authorize_url, encode_scopes, expires_at_after, generate_code_challenge,
    generate_code_verifier, generate_state, parse_oauth_callback, parse_session_state,
    required_http_client, validate_callback_state,
};
use crate::auth::types::{
    AuthDriver, AuthDriverMetadata, AuthExchangeInput, AuthScheme, AuthSession, CreateAuthSession,
    CredentialBundle, ExchangeAuthContext, RefreshAuthContext, RuntimeBinding, StartAuthContext,
    StoredCredential,
};
use crate::db::models::Provider;
use crate::protocol::vendor::VendorRegistry;
use crate::protocol::vendor::types::{OAuthConfig, RuntimeConfig};

const OPENAI_PRESET_ID: &str = "openai";
const CODEX_CHANNEL_ID: &str = "codex";

/// Resolved OAuth + runtime config for the OpenAI / Codex channel,
/// sourced from the in-process `VendorRegistry`.
#[derive(Debug, Clone, Copy)]
struct OpenAICodexConfig {
    oauth: &'static OAuthConfig,
    runtime: &'static RuntimeConfig,
}

#[derive(Debug, Default)]
pub struct OpenAIOAuthDriver;

#[derive(Debug, Deserialize)]
struct OpenAITokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIErrorResponse {
    error: Option<String>,
    error_description: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAIAuthState {
    #[serde(flatten)]
    pkce: PkceAuthState,
}

impl OpenAIOAuthDriver {
    fn codex_config() -> Result<OpenAICodexConfig> {
        let metadata = VendorRegistry::global()
            .metadata(OPENAI_PRESET_ID)
            .ok_or_else(|| anyhow!("missing provider preset: {OPENAI_PRESET_ID}"))?;
        let channel = metadata
            .channels
            .iter()
            .find(|c| c.id == CODEX_CHANNEL_ID)
            .ok_or_else(|| {
                anyhow!("missing provider channel: {OPENAI_PRESET_ID}/{CODEX_CHANNEL_ID}")
            })?;
        Ok(OpenAICodexConfig {
            oauth: channel.oauth.as_ref().ok_or_else(|| {
                anyhow!("missing oauth config for {OPENAI_PRESET_ID}/{CODEX_CHANNEL_ID}")
            })?,
            runtime: channel.runtime.as_ref().ok_or_else(|| {
                anyhow!("missing runtime config for {OPENAI_PRESET_ID}/{CODEX_CHANNEL_ID}")
            })?,
        })
    }

    fn normalize_token_response(
        body: &str,
        fallback_refresh_token: Option<&str>,
        runtime: &RuntimeConfig,
    ) -> Result<CredentialBundle> {
        let token: OpenAITokenResponse =
            serde_json::from_str(body).context("parse openai oauth token response")?;
        let access_token = token
            .access_token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow!("openai oauth token response missing access_token"))?;
        let expires_in = token.expires_in.unwrap_or(3600).max(1);

        Ok(CredentialBundle {
            access_token: Some(access_token),
            refresh_token: token
                .refresh_token
                .filter(|value| !value.trim().is_empty())
                .or_else(|| fallback_refresh_token.map(ToString::to_string)),
            expires_at: Some(expires_at_after(expires_in)),
            resource_url: Some(runtime.api_base_url.to_string()),
            subject_id: None,
            scopes: encode_scopes(token.scope.as_deref()),
            raw: serde_json::from_str(body).unwrap_or(serde_json::Value::Null),
        })
    }

    fn parse_error(body: &str) -> Option<String> {
        let parsed: OpenAIErrorResponse = serde_json::from_str(body).ok()?;
        parsed
            .error_description
            .filter(|value| !value.trim().is_empty())
            .or_else(|| parsed.message.filter(|value| !value.trim().is_empty()))
            .or_else(|| parsed.error.filter(|value| !value.trim().is_empty()))
    }

    fn decode_jwt_claims(token: &str) -> Option<Value> {
        let payload = token.split('.').nth(1)?;
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .ok()?;
        serde_json::from_slice(&decoded).ok()
    }

    fn extract_account_id(credential: &StoredCredential) -> Option<String> {
        let access_token = credential.access_token.as_deref()?.trim();
        if access_token.is_empty() {
            return None;
        }

        let claims = Self::decode_jwt_claims(access_token)?;
        claims
            .get("https://api.openai.com/auth")
            .and_then(Value::as_object)
            .and_then(|auth| auth.get("chatgpt_account_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                claims
                    .get("https://api.openai.com/auth.chatgpt_account_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
            })
    }

    fn codex_models_source(runtime: &RuntimeConfig) -> String {
        format!(
            "{}?client_version={}",
            runtime.models_url, runtime.models_client_version
        )
    }
}

#[async_trait]
impl AuthDriver for OpenAIOAuthDriver {
    fn metadata(&self) -> AuthDriverMetadata {
        AuthDriverMetadata {
            key: "codex",
            label: "Codex",
            scheme: AuthScheme::OAuthAuthCodePkce,
            supports_new_provider: true,
            supports_existing_provider: true,
        }
    }

    async fn start(&self, ctx: StartAuthContext) -> Result<CreateAuthSession> {
        let config = Self::codex_config()?;
        let code_verifier = generate_code_verifier();
        let code_challenge = generate_code_challenge(&code_verifier);
        let state = generate_state();
        let redirect_uri = ctx
            .redirect_uri
            .as_deref()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or(config.oauth.redirect_uri);
        let auth_url = build_authorize_url(
            config.oauth.authorize_url,
            &[
                ("response_type", "code"),
                ("client_id", config.oauth.client_id),
                ("redirect_uri", redirect_uri),
                ("scope", config.oauth.scope),
                ("code_challenge", &code_challenge),
                ("code_challenge_method", "S256"),
                ("state", &state),
                ("id_token_add_organizations", "true"),
                ("codex_cli_simplified_flow", "true"),
            ],
        )?;
        let session_state = serde_json::to_string(&OpenAIAuthState {
            pkce: PkceAuthState {
                code_verifier,
                state,
                redirect_uri: redirect_uri.to_string(),
            },
        })?;

        Ok(CreateAuthSession {
            provider_id: ctx.provider_id,
            driver_key: self.metadata().key.to_string(),
            scheme: self.metadata().scheme.as_str().to_string(),
            status: "pending".to_string(),
            use_proxy: ctx.use_proxy,
            user_code: None,
            verification_uri: Some(config.oauth.auth_base_url.to_string()),
            verification_uri_complete: Some(auth_url),
            state_json: Some(session_state),
            context_json: None,
            result_json: None,
            expires_at: Some(expires_at_after(10 * 60)),
            poll_interval_seconds: Some(2),
            last_error: None,
        })
    }

    async fn exchange(
        &self,
        session: &AuthSession,
        input: AuthExchangeInput,
        ctx: ExchangeAuthContext,
    ) -> Result<CredentialBundle> {
        let config = Self::codex_config()?;
        let state: OpenAIAuthState = parse_session_state(session)?;
        let callback = parse_oauth_callback(&input)?;
        validate_callback_state(&state.pkce.state, callback.state.as_deref(), "openai")?;
        let code = callback
            .code
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("missing authorization code"))?;

        let client = required_http_client(ctx.http_client)?;
        let response = client
            .post(config.oauth.token_url)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(ACCEPT, "application/json")
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", state.pkce.redirect_uri.as_str()),
                ("client_id", config.oauth.client_id),
                ("code_verifier", state.pkce.code_verifier.as_str()),
            ])
            .send()
            .await
            .context("exchange openai authorization code")?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            let detail = Self::parse_error(&body).unwrap_or(body);
            bail!("openai oauth token exchange failed: HTTP {status} {detail}");
        }

        Self::normalize_token_response(&body, None, config.runtime)
    }

    async fn refresh(
        &self,
        credential: &StoredCredential,
        ctx: RefreshAuthContext,
    ) -> Result<CredentialBundle> {
        let config = Self::codex_config()?;
        let refresh_token = credential
            .refresh_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("openai oauth refresh token is missing"))?;
        let client = required_http_client(ctx.http_client)?;

        let response = client
            .post(config.oauth.token_url)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(ACCEPT, "application/json")
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", config.oauth.client_id),
                ("refresh_token", refresh_token),
                ("scope", config.oauth.scope),
            ])
            .send()
            .await
            .context("refresh openai oauth token")?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            let detail = Self::parse_error(&body).unwrap_or(body);
            bail!("openai oauth token refresh failed: HTTP {status} {detail}");
        }

        Self::normalize_token_response(&body, Some(refresh_token), config.runtime)
    }

    fn bind_runtime(
        &self,
        provider: &Provider,
        credential: &StoredCredential,
    ) -> Result<RuntimeBinding> {
        let config = Self::codex_config()?;
        let account_id = Self::extract_account_id(credential);
        let mut extra_headers = HashMap::new();
        if let Some(account_id) = account_id {
            extra_headers.insert("chatgpt-account-id".to_string(), account_id);
        }
        let base_url_override = credential
            .resource_url
            .clone()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| Some(config.runtime.api_base_url.to_string()));

        let models_source_override = Some(Self::codex_models_source(config.runtime));
        let capabilities_source_override = provider
            .capabilities_source
            .clone()
            .filter(|value| !value.trim().is_empty());

        Ok(RuntimeBinding {
            base_url_override,
            extra_headers,
            model_aliases: HashMap::new(),
            models_source_override,
            capabilities_source_override,
            disable_default_auth: false,
        })
    }
}
