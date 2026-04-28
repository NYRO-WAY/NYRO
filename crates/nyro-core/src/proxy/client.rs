//! Thin wrapper around `reqwest::Client` for upstream calls.
//!
//! PR3 split out the old `ProviderAdapter` plumbing — URL building and
//! auth header construction now happen at the call site (via
//! `VendorRegistry::resolve` + `VendorExtension::{auth_headers,
//! build_url}`). `ProxyClient` is intentionally adapter-agnostic: it
//! takes a fully-built URL and a ready-to-send header map and just
//! issues the HTTP call.

use anyhow::Result;
use reqwest::header::HeaderMap;
use serde_json::Value;

pub struct ProxyClient {
    pub http: reqwest::Client,
}

impl ProxyClient {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    pub async fn call_non_stream(
        &self,
        url: &str,
        headers: HeaderMap,
        body: Value,
    ) -> Result<(Value, u16)> {
        let resp = self.http.post(url).headers(headers).json(&body).send().await?;
        let status = resp.status().as_u16();
        let json: Value = resp.json().await?;
        Ok((json, status))
    }

    pub async fn call_stream(
        &self,
        url: &str,
        headers: HeaderMap,
        body: Value,
    ) -> Result<(reqwest::Response, u16)> {
        let resp = self.http.post(url).headers(headers).json(&body).send().await?;
        let status = resp.status().as_u16();
        Ok((resp, status))
    }
}
