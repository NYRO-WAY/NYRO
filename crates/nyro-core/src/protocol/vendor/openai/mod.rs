//! OpenAI vendor — direct API plus the Codex channel (OAuth via
//! ChatGPT). Default-channel behaviour is identical to the OpenAI
//! family fallback; the codex-specific HTTP headers (e.g.
//! `chatgpt-account-id`) are injected by `OpenAIOAuthDriver` through
//! `RuntimeBinding.extra_headers`, so this module currently only
//! supplies metadata + dispatch — see PR3 for the runtime wiring.

pub mod codex;

use reqwest::header::HeaderMap;

use crate::protocol::vendor::defaults::OpenAiDefault;
use crate::protocol::vendor::types::{
    AuthMode, ChannelDef, Label, OAuthCompletionMode, OAuthConfig, ProtocolBaseUrl,
    RuntimeConfig, VendorMetadata,
};
use crate::protocol::vendor::{VendorCtx, VendorExtension, VendorRegistration, VendorScope};

const METADATA: VendorMetadata = VendorMetadata {
    id: "openai",
    label: Label {
        zh: "OpenAI",
        en: "OpenAI",
    },
    icon: "openai",
    default_protocol: "openai",
    channels: &[
        ChannelDef {
            id: "default",
            label: Label {
                zh: "默认",
                en: "Default",
            },
            base_urls: &[ProtocolBaseUrl {
                protocol: "openai",
                base_url: "https://api.openai.com/v1",
            }],
            api_key: None,
            models_source: Some("https://api.openai.com/v1/models"),
            capabilities_source: Some("ai://models.dev/openai"),
            static_models: &[],
            auth_mode: AuthMode::ApiKey,
            oauth: None,
            runtime: None,
        },
        ChannelDef {
            id: "codex",
            label: Label {
                zh: "Codex",
                en: "Codex",
            },
            base_urls: &[ProtocolBaseUrl {
                protocol: "openai_responses",
                base_url: "https://chatgpt.com/backend-api/codex",
            }],
            api_key: None,
            models_source: Some("https://chatgpt.com/backend-api/codex/models"),
            capabilities_source: Some("ai://models.dev/openai"),
            static_models: &[],
            auth_mode: AuthMode::OAuth,
            oauth: Some(OAuthConfig {
                auth_base_url: "https://auth.openai.com",
                authorize_url: "https://auth.openai.com/oauth/authorize",
                token_url: "https://auth.openai.com/oauth/token",
                client_id: "app_EMoamEEZ73f0CkXaXp7hrann",
                redirect_uri: "http://localhost:1455/auth/callback",
                scope: "openid profile email offline_access",
                completion_mode: OAuthCompletionMode::CallbackOrCode,
            }),
            runtime: Some(RuntimeConfig {
                api_base_url: "https://chatgpt.com/backend-api/codex",
                models_url: "https://chatgpt.com/backend-api/codex/models",
                models_client_version: "0.99.0",
            }),
        },
    ],
};

pub struct OpenAiVendor;

impl VendorExtension for OpenAiVendor {
    fn scope(&self) -> VendorScope {
        VendorScope::Vendor { vendor_id: "openai" }
    }

    fn metadata(&self) -> Option<&'static VendorMetadata> {
        Some(&METADATA)
    }

    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap {
        OpenAiDefault.auth_headers(ctx)
    }

    fn build_url(&self, ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String {
        OpenAiDefault.build_url(ctx, base_url, path)
    }
}

inventory::submit! {
    VendorRegistration { make: || Box::new(OpenAiVendor) }
}
