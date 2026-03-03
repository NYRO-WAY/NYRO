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
            _ => {
                let val = format!("Bearer {api_key}");
                if let Ok(v) = HeaderValue::from_str(&val) {
                    headers.insert("Authorization", v);
                }
            }
        }
        headers
    }

    pub async fn call_non_stream(
        &self,
        base_url: &str,
        path: &str,
        api_key: &str,
        protocol: Protocol,
        body: Value,
    ) -> Result<(Value, u16)> {
        let url = format!("{}{}", base_url.trim_end_matches('/'), path);
        let headers = Self::build_auth_headers(protocol, api_key);

        let resp = self
            .http
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;

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
    ) -> Result<(reqwest::Response, u16)> {
        let url = format!("{}{}", base_url.trim_end_matches('/'), path);
        let headers = Self::build_auth_headers(protocol, api_key);

        let resp = self
            .http
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;

        let status = resp.status().as_u16();
        Ok((resp, status))
    }
}
