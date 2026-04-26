//! `nyro-tools record` — scenario-driven recorder.
//!
//! For a given (vendor, upstream-protocol, upstream-endpoint, model[, reasoning-model]):
//!   1. iterate every scenario whose `bodies.<protocol>` is `Some`
//!   2. pick the real model name (reasoning-model when scenario.uses_reasoning_model is true)
//!   3. POST the protocol-specific body template to the upstream, collecting the full byte stream
//!   4. write `<output-dir>/<protocol>/<vendor>/<scenario>.jsonl` containing one Fixture line
//!
//! Existing fixture files are skipped (no overwrite). Sensitive headers are scrubbed before persistence.

use crate::fixture::{
    Fixture, FIXTURE_VERSION, RecordedRequest, RecordedResponse, scrub_sensitive_headers,
};
use crate::protocol::{ProtocolKind, validate_upstream_endpoint};
use crate::scenarios::{MODEL_PLACEHOLDER, SCENARIOS, Scenario};
use anyhow::{Context, Result, bail};
use base64::Engine;
use clap::Args;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{info, warn};

#[derive(Debug, Args)]
pub struct RecordArgs {
    /// Vendor identifier (kebab-case, no `--` substring)
    #[arg(long)]
    pub vendor: String,

    /// Upstream protocol short name
    #[arg(short = 'p', long, value_enum)]
    pub upstream_protocol: ProtocolKind,

    /// Upstream endpoint base URL
    #[arg(short = 'e', long)]
    pub upstream_endpoint: url::Url,

    /// Output root directory; subdirs `<protocol>/<vendor>/` are auto-derived
    #[arg(short = 'o', long)]
    pub output_dir: PathBuf,

    /// Real LLM model name for non-reasoning scenarios
    #[arg(long)]
    pub model: String,

    /// Real LLM model name for reasoning-* scenarios; falls back to --model when omitted
    #[arg(long)]
    pub reasoning_model: Option<String>,

    /// Environment variable that holds the API key (e.g. DEEPSEEK_API_KEY)
    #[arg(long)]
    pub api_key_env: String,
}

pub async fn run(args: RecordArgs) -> Result<()> {
    if args.vendor.contains("--") {
        bail!("--vendor `{}` must not contain `--`", args.vendor);
    }
    if args.vendor.is_empty() || args.model.is_empty() {
        bail!("--vendor and --model must be non-empty");
    }
    validate_upstream_endpoint(&args.upstream_endpoint)?;
    let api_key = std::env::var(&args.api_key_env).with_context(|| {
        format!(
            "API key env var `{}` is unset; export it before running `record`",
            args.api_key_env
        )
    })?;
    if api_key.is_empty() {
        bail!("API key env var `{}` is empty", args.api_key_env);
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?;

    let mut summary = RecordSummary::default();
    for scenario in SCENARIOS {
        let result = record_one(&args, &api_key, &client, scenario).await;
        match result {
            Ok(RecordOutcome::Skipped(path)) => {
                info!(scenario = scenario.name, path = %path.display(), "SKIP (fixture exists)");
                summary.skipped += 1;
            }
            Ok(RecordOutcome::Recorded(path)) => {
                info!(scenario = scenario.name, path = %path.display(), "OK");
                summary.recorded += 1;
            }
            Ok(RecordOutcome::NotApplicable) => {
                info!(scenario = scenario.name, "n/a (no body for this protocol)");
                summary.skipped += 1;
            }
            Err(e) => {
                warn!(scenario = scenario.name, error = %e, "FAIL");
                summary.failed.push((scenario.name, e.to_string()));
            }
        }
    }

    summary.print(&args);
    if !summary.failed.is_empty() {
        bail!("{} scenario(s) failed; see log above", summary.failed.len());
    }
    Ok(())
}

#[derive(Default)]
struct RecordSummary {
    recorded: usize,
    skipped: usize,
    failed: Vec<(&'static str, String)>,
}

impl RecordSummary {
    fn print(&self, args: &RecordArgs) {
        println!(
            "\n=== nyro-tools record summary ===\n  vendor:   {}\n  protocol: {}\n  endpoint: {}\n  recorded: {}\n  skipped:  {}\n  failed:   {}",
            args.vendor,
            args.upstream_protocol,
            args.upstream_endpoint,
            self.recorded,
            self.skipped,
            self.failed.len()
        );
        for (name, err) in &self.failed {
            println!("    - {name}: {err}");
        }
    }
}

enum RecordOutcome {
    NotApplicable,
    Skipped(PathBuf),
    Recorded(PathBuf),
}

async fn record_one(
    args: &RecordArgs,
    api_key: &str,
    client: &reqwest::Client,
    scenario: &Scenario,
) -> Result<RecordOutcome> {
    let Some(body_template) = scenario.body_for(args.upstream_protocol) else {
        return Ok(RecordOutcome::NotApplicable);
    };

    let real_model = if scenario.uses_reasoning_model {
        args.reasoning_model.as_deref().unwrap_or(&args.model)
    } else {
        &args.model
    };

    let fixture_path = args
        .output_dir
        .join(args.upstream_protocol.as_short_name())
        .join(&args.vendor)
        .join(format!("{}.jsonl", scenario.name));

    if fixture_path.exists() {
        return Ok(RecordOutcome::Skipped(fixture_path));
    }

    let body_json: Value = {
        let rendered = body_template.replace(MODEL_PLACEHOLDER, real_model);
        serde_json::from_str(&rendered).with_context(|| {
            format!(
                "scenario `{}` rendered body is not valid JSON",
                scenario.name
            )
        })?
    };

    let (suffix, query) = args
        .upstream_protocol
        .record_path_suffix(scenario.stream, real_model);

    let mut url = args.upstream_endpoint.clone();
    {
        let base = url.path().trim_end_matches('/').to_string();
        url.set_path(&format!("{base}{suffix}"));
    }
    if let Some(q) = query.as_deref() {
        url.set_query(Some(q));
    }
    // Full upstream URL path actually requested, e.g.
    // `/v1/chat/completions` for OpenAI-style or `/api/coding/paas/v4/chat/completions`
    // for Zhipu's non-standard mount. Stored verbatim in the fixture for diagnostics.
    let recorded_request_path = match url.query() {
        Some(q) => format!("{}?{q}", url.path()),
        None => url.path().to_string(),
    };

    let mut req = client.post(url.clone());
    req = apply_auth_headers(req, args.upstream_protocol, api_key);
    let req = req
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::ACCEPT, accept_header(scenario))
        .json(&body_json);

    let resp = req.send().await.with_context(|| {
        format!(
            "scenario `{}`: request to {} {} failed",
            scenario.name, "POST", url
        )
    })?;
    let status = resp.status().as_u16();
    let mut response_headers = BTreeMap::new();
    for (name, value) in resp.headers().iter() {
        if let Ok(v) = value.to_str() {
            response_headers.insert(name.as_str().to_lowercase(), v.to_string());
        }
    }
    let body_bytes = resp.bytes().await.with_context(|| {
        format!("scenario `{}`: failed to read full response body", scenario.name)
    })?;
    if status >= 400 {
        let snippet = String::from_utf8_lossy(&body_bytes[..body_bytes.len().min(512)]);
        bail!(
            "scenario `{}`: upstream returned HTTP {} — body: {snippet}",
            scenario.name,
            status
        );
    }
    scrub_sensitive_headers(&mut response_headers);

    let request_headers_recorded = recorded_request_headers(args.upstream_protocol);

    let replay_model = format!(
        "{}--{}--{}",
        args.vendor,
        args.upstream_protocol.as_short_name(),
        scenario.name
    );

    let fx = Fixture {
        version: FIXTURE_VERSION,
        replay_model: replay_model.clone(),
        scenario: scenario.name.to_string(),
        vendor: args.vendor.clone(),
        protocol: args.upstream_protocol.as_short_name().to_string(),
        recorded_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        request: RecordedRequest {
            method: "POST".to_string(),
            path: recorded_request_path,
            headers: request_headers_recorded,
            body_json,
        },
        response: RecordedResponse {
            status,
            headers: response_headers,
            body_base64: base64::engine::general_purpose::STANDARD.encode(&body_bytes),
        },
    };
    fx.validate()?;
    fx.save(&fixture_path)?;
    Ok(RecordOutcome::Recorded(fixture_path))
}

fn apply_auth_headers(
    req: reqwest::RequestBuilder,
    protocol: ProtocolKind,
    api_key: &str,
) -> reqwest::RequestBuilder {
    match protocol {
        ProtocolKind::OpenAiChat | ProtocolKind::OpenAiResponses => {
            req.header(reqwest::header::AUTHORIZATION, format!("Bearer {api_key}"))
        }
        ProtocolKind::AnthropicMessages => req
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
        ProtocolKind::GoogleContent => req.header("x-goog-api-key", api_key),
    }
}

fn accept_header(scenario: &Scenario) -> &'static str {
    if scenario.stream {
        "text/event-stream"
    } else {
        "application/json"
    }
}

/// Records only the non-sensitive headers we actually sent — never the API key.
fn recorded_request_headers(protocol: ProtocolKind) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    map.insert("content-type".to_string(), "application/json".to_string());
    if matches!(protocol, ProtocolKind::AnthropicMessages) {
        map.insert(
            "anthropic-version".to_string(),
            "2023-06-01".to_string(),
        );
    }
    scrub_sensitive_headers(&mut map);
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_path_layout() {
        let args = RecordArgs {
            vendor: "deepseek".into(),
            upstream_protocol: ProtocolKind::OpenAiChat,
            upstream_endpoint: url::Url::parse("https://api.deepseek.com/v1").unwrap(),
            output_dir: PathBuf::from("/tmp/out"),
            model: "deepseek-chat".into(),
            reasoning_model: None,
            api_key_env: "X".into(),
        };
        let p = args
            .output_dir
            .join(args.upstream_protocol.as_short_name())
            .join(&args.vendor)
            .join("basic-stream.jsonl");
        assert_eq!(
            p,
            std::path::Path::new("/tmp/out/openai-chat/deepseek/basic-stream.jsonl")
        );
    }

    #[test]
    fn endpoint_with_version_path_required() {
        let bare = url::Url::parse("https://api.deepseek.com").unwrap();
        assert!(crate::protocol::validate_upstream_endpoint(&bare).is_err());
        let versioned = url::Url::parse("https://api.deepseek.com/v1").unwrap();
        assert!(crate::protocol::validate_upstream_endpoint(&versioned).is_ok());
        let zhipu = url::Url::parse("https://open.bigmodel.cn/api/coding/paas/v4").unwrap();
        assert!(crate::protocol::validate_upstream_endpoint(&zhipu).is_ok());
    }

    #[test]
    fn record_path_suffix_is_protocol_version_agnostic() {
        // Standard /v1 vendor: full URL = endpoint + suffix.
        let suffix = ProtocolKind::OpenAiChat
            .record_path_suffix(true, "deepseek-chat")
            .0;
        assert_eq!(suffix, "/chat/completions");
        let mut url = url::Url::parse("https://api.deepseek.com/v1").unwrap();
        let base = url.path().trim_end_matches('/').to_string();
        url.set_path(&format!("{base}{suffix}"));
        assert_eq!(url.as_str(), "https://api.deepseek.com/v1/chat/completions");

        // Non-standard /api/coding/paas/v4 vendor: same suffix, different base.
        let mut url = url::Url::parse("https://open.bigmodel.cn/api/coding/paas/v4").unwrap();
        let base = url.path().trim_end_matches('/').to_string();
        url.set_path(&format!("{base}{suffix}"));
        assert_eq!(
            url.as_str(),
            "https://open.bigmodel.cn/api/coding/paas/v4/chat/completions"
        );
    }

    #[test]
    fn replay_model_format() {
        let composed = format!(
            "{}--{}--{}",
            "deepseek",
            ProtocolKind::OpenAiChat.as_short_name(),
            "basic-stream"
        );
        assert_eq!(composed, "deepseek--openai-chat--basic-stream");
    }

    #[test]
    fn body_template_substitutes_model() {
        let scenario = &SCENARIOS[0];
        let template = scenario.body_for(ProtocolKind::OpenAiChat).unwrap();
        let rendered = template.replace(MODEL_PLACEHOLDER, "real-model-1");
        let v: Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(v["model"], "real-model-1");
    }
}
