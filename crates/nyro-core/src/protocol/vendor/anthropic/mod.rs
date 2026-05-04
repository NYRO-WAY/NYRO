//! Anthropic vendor (direct API). Reuses the Anthropic family default
//! for headers/URL.
//!
//! Includes the Claude Code OAuth channel. Channel-specific runtime
//! headers are injected by the auth driver through `RuntimeBinding`.

pub mod claude_code;

use reqwest::header::HeaderMap;

use crate::protocol::vendor::defaults::AnthropicDefault;
use crate::protocol::vendor::types::{
    AuthMode, ChannelDef, Label, OAuthCompletionMode, OAuthConfig, ProtocolBaseUrl,
    VendorMetadata,
};
use crate::protocol::vendor::{VendorCtx, VendorExtension, VendorRegistration, VendorScope};

const METADATA: VendorMetadata = VendorMetadata {
    id: "anthropic",
    label: Label {
        zh: "Anthropic",
        en: "Anthropic",
    },
    icon: "anthropic",
    default_protocol: "anthropic",
    channels: &[
        ChannelDef {
            id: "default",
            label: Label {
                zh: "默认",
                en: "Default",
            },
            base_urls: &[ProtocolBaseUrl {
                protocol: "anthropic",
                base_url: "https://api.anthropic.com",
            }],
            api_key: None,
            models_source: Some("https://api.anthropic.com/v1/models"),
            capabilities_source: Some("ai://models.dev/anthropic"),
            static_models: &[],
            auth_mode: AuthMode::ApiKey,
            oauth: None,
            runtime: None,
        },
        ChannelDef {
            id: "claude-code",
            label: Label {
                zh: "Claude Code",
                en: "Claude Code",
            },
            base_urls: &[ProtocolBaseUrl {
                protocol: "anthropic",
                base_url: "https://api.anthropic.com",
            }],
            api_key: None,
            models_source: Some("https://api.anthropic.com/v1/models"),
            capabilities_source: Some("ai://models.dev/anthropic"),
            static_models: &[],
            auth_mode: AuthMode::OAuth,
            oauth: Some(OAuthConfig {
                auth_base_url: "https://claude.ai",
                authorize_url: "https://claude.ai/oauth/authorize",
                token_url: "https://console.anthropic.com/v1/oauth/token",
                client_id: "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
                redirect_uri: "https://platform.claude.com/oauth/code/callback",
                scope: "org:create_api_key user:profile user:inference user:sessions:claude_code",
                completion_mode: OAuthCompletionMode::CodeOnly,
            }),
            runtime: None,
        },
    ],
};

pub struct AnthropicVendor;

impl VendorExtension for AnthropicVendor {
    fn scope(&self) -> VendorScope {
        VendorScope::Vendor {
            vendor_id: "anthropic",
        }
    }

    fn metadata(&self) -> Option<&'static VendorMetadata> {
        Some(&METADATA)
    }

    fn auth_headers(&self, ctx: &VendorCtx<'_>) -> HeaderMap {
        AnthropicDefault.auth_headers(ctx)
    }

    fn build_url(&self, ctx: &VendorCtx<'_>, base_url: &str, path: &str) -> String {
        AnthropicDefault.build_url(ctx, base_url, path)
    }
}

inventory::submit! {
    VendorRegistration { make: || Box::new(AnthropicVendor) }
}
