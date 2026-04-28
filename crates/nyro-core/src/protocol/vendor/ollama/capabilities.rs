//! Ollama `/api/show` capability probing with TTL cache. Migrated
//! verbatim from `proxy/adapter.rs`.

use std::time::Duration;

use reqwest::Url;
use serde_json::Value;

use crate::Gateway;
use crate::db::models::Provider;

pub(super) const OLLAMA_CAPABILITY_CACHE_TTL_SECS: u64 = 3600;

pub(super) async fn get_ollama_capabilities(
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
