use std::time::Duration;

use async_trait::async_trait;
use reqwest::Url;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;

use crate::Gateway;
use crate::db::models::Provider;
use crate::protocol::Protocol;
use crate::protocol::types::InternalRequest;

const OLLAMA_CAPABILITY_CACHE_TTL_SECS: u64 = 3600;

// ── Trait ──

#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    fn auth_headers(&self, api_key: &str) -> HeaderMap;
    fn build_url(&self, base_url: &str, path: &str, api_key: &str) -> String;

    async fn pre_request(
        &self,
        _req: &mut InternalRequest,
        _actual_model: &str,
        _gw: &Gateway,
        _provider: &Provider,
    ) {
    }
}

// ── OpenAI-compatible (xAI, DeepSeek, Moonshot, Groq …) ──

pub struct OpenAICompatAdapter;

#[async_trait]
impl ProviderAdapter for OpenAICompatAdapter {
    fn auth_headers(&self, api_key: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        let val = format!("Bearer {api_key}");
        if let Ok(v) = HeaderValue::from_str(&val) {
            h.insert("Authorization", v);
        }
        h
    }

    fn build_url(&self, base_url: &str, path: &str, _api_key: &str) -> String {
        let base = base_url.trim_end_matches('/');
        let adjusted = if has_non_root_path(base) && path.starts_with("/v1/") {
            &path[3..]
        } else {
            path
        };
        format!("{base}{adjusted}")
    }
}

// ── Anthropic ──

pub struct AnthropicAdapter;

#[async_trait]
impl ProviderAdapter for AnthropicAdapter {
    fn auth_headers(&self, api_key: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            "x-api-key",
            HeaderValue::from_str(api_key).unwrap_or_else(|_| HeaderValue::from_static("")),
        );
        h.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        h
    }

    fn build_url(&self, base_url: &str, path: &str, _api_key: &str) -> String {
        format!("{}{path}", base_url.trim_end_matches('/'))
    }
}

// ── Gemini ──

pub struct GeminiAdapter;

#[async_trait]
impl ProviderAdapter for GeminiAdapter {
    fn auth_headers(&self, _api_key: &str) -> HeaderMap {
        HeaderMap::new()
    }

    fn build_url(&self, base_url: &str, path: &str, api_key: &str) -> String {
        let url = format!("{}{path}", base_url.trim_end_matches('/'));
        if url.contains('?') {
            format!("{url}&key={api_key}")
        } else {
            format!("{url}?key={api_key}")
        }
    }
}

// ── Ollama ──

pub struct OllamaAdapter;

#[async_trait]
impl ProviderAdapter for OllamaAdapter {
    fn auth_headers(&self, api_key: &str) -> HeaderMap {
        OpenAICompatAdapter.auth_headers(api_key)
    }

    fn build_url(&self, base_url: &str, path: &str, api_key: &str) -> String {
        OpenAICompatAdapter.build_url(base_url, path, api_key)
    }

    async fn pre_request(
        &self,
        req: &mut InternalRequest,
        actual_model: &str,
        gw: &Gateway,
        provider: &Provider,
    ) {
        if req.tools.is_none() && req.tool_choice.is_none() {
            return;
        }

        let model = actual_model;
        let caps = match get_ollama_capabilities(gw, provider, model).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    "failed to fetch capabilities for model {model}, skipping tools check: {e}"
                );
                return;
            }
        };

        let supports_tools = caps.iter().any(|c| c == "tools");
        if !supports_tools {
            tracing::warn!(
                "tools stripped for model {model} (tools not supported, capabilities: {caps:?})"
            );
            req.tools = None;
            req.tool_choice = None;
            req.extra.remove("tools");
            req.extra.remove("tool_choice");
        }
    }
}

async fn get_ollama_capabilities(
    gw: &Gateway,
    provider: &Provider,
    model: &str,
) -> anyhow::Result<Vec<String>> {
    let ttl = Duration::from_secs(OLLAMA_CAPABILITY_CACHE_TTL_SECS);
    if let Some(cached) = gw
        .get_ollama_capabilities_cached(&provider.id, model, ttl)
        .await
    {
        return Ok(cached);
    }

    let caps = fetch_ollama_capabilities(&gw.http_client, &provider.base_url, model).await?;
    gw.set_ollama_capabilities_cache(&provider.id, model, caps.clone())
        .await;
    Ok(caps)
}

async fn fetch_ollama_capabilities(
    http: &reqwest::Client,
    base_url: &str,
    model: &str,
) -> anyhow::Result<Vec<String>> {
    let url = build_ollama_show_url(base_url)?;

    let resp = http
        .post(url)
        .json(&serde_json::json!({ "name": model }))
        .timeout(Duration::from_secs(5))
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("ollama /api/show returned status {}", resp.status());
    }

    let json: Value = resp.json().await?;
    let caps = json
        .get("capabilities")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c.as_str().map(ToString::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(caps)
}

fn build_ollama_show_url(base_url: &str) -> anyhow::Result<Url> {
    let mut url = Url::parse(base_url)?;
    let raw_path = url.path().trim_end_matches('/');
    let path = if raw_path.is_empty() {
        "/api/show".to_string()
    } else if raw_path.ends_with("/v1") {
        let prefix = raw_path.trim_end_matches("/v1");
        if prefix.is_empty() {
            "/api/show".to_string()
        } else {
            format!("{prefix}/api/show")
        }
    } else {
        format!("{raw_path}/api/show")
    };
    url.set_path(&path);
    url.set_query(None);
    Ok(url)
}

// ── Helpers ──

fn has_non_root_path(base: &str) -> bool {
    reqwest::Url::parse(base)
        .ok()
        .map(|url| {
            let pathname = url.path().trim_end_matches('/');
            !pathname.is_empty() && pathname != "/"
        })
        .unwrap_or(false)
}

fn is_ollama_provider(provider: &Provider) -> bool {
    provider
        .vendor
        .as_deref()
        .is_some_and(|v| v.eq_ignore_ascii_case("ollama"))
}

// ── Factory ──

pub fn get_adapter(provider: &Provider, egress: Protocol) -> Box<dyn ProviderAdapter> {
    if is_ollama_provider(provider) {
        return Box::new(OllamaAdapter);
    }
    match egress {
        Protocol::Anthropic => Box::new(AnthropicAdapter),
        Protocol::Gemini => Box::new(GeminiAdapter),
        _ => Box::new(OpenAICompatAdapter),
    }
}
