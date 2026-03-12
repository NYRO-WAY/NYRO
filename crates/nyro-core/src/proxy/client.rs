use anyhow::Result;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;

use crate::protocol::Protocol;

pub struct ProxyClient {
    pub http: reqwest::Client,
}

impl ProxyClient {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    fn build_auth_headers(protocol: Protocol, api_key: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        match protocol {
            Protocol::Anthropic => {
                headers.insert(
                    "x-api-key",
                    HeaderValue::from_str(api_key).unwrap_or_else(|_| HeaderValue::from_static("")),
                );
                headers.insert(
                    "anthropic-version",
                    HeaderValue::from_static("2023-06-01"),
                );
            }
            Protocol::Gemini => {
                // Gemini uses ?key= query param, handled in URL construction
            }
            _ => {
                let val = format!("Bearer {api_key}");
                if let Ok(v) = HeaderValue::from_str(&val) {
                    headers.insert("Authorization", v);
                }
            }
        }
        headers
    }

    fn build_url(base_url: &str, path: &str, protocol: Protocol, api_key: &str) -> String {
        let base = base_url.trim_end_matches('/');
        let adjusted_path = match protocol {
            Protocol::OpenAI => {
                let has_base_path = reqwest::Url::parse(base)
                    .ok()
                    .map(|url| {
                        let pathname = url.path().trim_end_matches('/');
                        !pathname.is_empty() && pathname != "/"
                    })
                    .unwrap_or(false);

                if has_base_path && path.starts_with("/v1/") {
                    &path[3..]
                } else {
                    path
                }
            }
            _ => path,
        };
        let url = format!("{base}{adjusted_path}");
        match protocol {
            Protocol::Gemini => {
                if url.contains('?') {
                    format!("{url}&key={api_key}")
                } else {
                    format!("{url}?key={api_key}")
                }
            }
            _ => url,
        }
    }

    pub async fn call_non_stream(
        &self,
        base_url: &str,
        path: &str,
        api_key: &str,
        protocol: Protocol,
        body: Value,
        extra_headers: HeaderMap,
    ) -> Result<(Value, u16)> {
        let url = Self::build_url(base_url, path, protocol, api_key);
        let mut headers = Self::build_auth_headers(protocol, api_key);
        headers.extend(extra_headers);

        let resp = self.http.post(&url).headers(headers).json(&body).send().await?;
        let status = resp.status().as_u16();
        let json: Value = resp.json().await?;
        Ok((json, status))
    }

    pub async fn call_stream(
        &self,
        base_url: &str,
        path: &str,
        api_key: &str,
        protocol: Protocol,
        body: Value,
        extra_headers: HeaderMap,
    ) -> Result<(reqwest::Response, u16)> {
        let url = Self::build_url(base_url, path, protocol, api_key);
        let mut headers = Self::build_auth_headers(protocol, api_key);
        headers.extend(extra_headers);

        let resp = self.http.post(&url).headers(headers).json(&body).send().await?;
        let status = resp.status().as_u16();
        Ok((resp, status))
    }
}
