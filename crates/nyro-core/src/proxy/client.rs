use anyhow::Result;
use reqwest::header::HeaderMap;
use serde_json::Value;

use super::adapter::ProviderAdapter;

pub struct ProxyClient {
    pub http: reqwest::Client,
}

impl ProxyClient {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    pub async fn call_non_stream(
        &self,
        adapter: &dyn ProviderAdapter,
        base_url: &str,
        path: &str,
        api_key: &str,
        body: Value,
        extra_headers: HeaderMap,
        skip_default_auth: bool,
    ) -> Result<(Value, u16)> {
        let url = adapter.build_url(base_url, path, api_key);
        let mut headers = if skip_default_auth {
            HeaderMap::new()
        } else {
            adapter.auth_headers(api_key)
        };
        headers.extend(extra_headers);

        let resp = self.http.post(&url).headers(headers).json(&body).send().await?;
        let status = resp.status().as_u16();
        let json: Value = resp.json().await?;
        Ok((json, status))
    }

    pub async fn call_stream(
        &self,
        adapter: &dyn ProviderAdapter,
        base_url: &str,
        path: &str,
        api_key: &str,
        body: Value,
        extra_headers: HeaderMap,
        skip_default_auth: bool,
    ) -> Result<(reqwest::Response, u16)> {
        let url = adapter.build_url(base_url, path, api_key);
        let mut headers = if skip_default_auth {
            HeaderMap::new()
        } else {
            adapter.auth_headers(api_key)
        };
        headers.extend(extra_headers);

        let resp = self.http.post(&url).headers(headers).json(&body).send().await?;
        let status = resp.status().as_u16();
        Ok((resp, status))
    }
}
