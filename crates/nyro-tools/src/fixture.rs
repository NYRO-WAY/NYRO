//! .jsonl fixture format. One file = one line = one full interaction.
//!
//! Fixture schema:
//! - `replay_model` is the canonical HashMap key used by `replay`; it is
//!   `<vendor>--<protocol>--<scenario>` and never split.
//! - `request` is captured purely for audit (not consumed during replay).
//! - `response.body_base64` is the raw upstream byte stream (incl. SSE / chunked).

use anyhow::{Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const FIXTURE_VERSION: u32 = 1;

/// Headers that must NEVER be persisted to a fixture file.
/// Hardcoded blacklist — no toggle, no opt-out.
pub const SENSITIVE_HEADER_BLACKLIST: &[&str] = &[
    "authorization",
    "x-api-key",
    "x-goog-api-key",
    "cookie",
    "set-cookie",
    "proxy-authorization",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fixture {
    pub version: u32,
    pub replay_model: String,
    pub scenario: String,
    pub vendor: String,
    pub protocol: String,
    pub recorded_at: String,
    pub request: RecordedRequest,
    pub response: RecordedResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedRequest {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body_json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    /// Full response body (incl. SSE/chunked payload) base64-encoded.
    pub body_base64: String,
}

impl Fixture {
    /// Decode `response.body_base64` into raw bytes for replay.
    pub fn response_body(&self) -> Result<Vec<u8>> {
        base64::engine::general_purpose::STANDARD
            .decode(&self.response.body_base64)
            .with_context(|| format!("invalid base64 body in {}", self.replay_model))
    }

    /// Load a single-line .jsonl fixture from disk.
    pub fn load(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read fixture file {}", path.display()))?;
        let line = raw
            .lines()
            .next()
            .with_context(|| format!("fixture file {} is empty", path.display()))?;
        serde_json::from_str(line)
            .with_context(|| format!("invalid fixture JSON in {}", path.display()))
    }

    /// Persist as single-line .jsonl with trailing newline.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let line = serde_json::to_string(self)?;
        fs::write(path, format!("{line}\n"))
            .with_context(|| format!("failed to write fixture {}", path.display()))
    }

    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.version == FIXTURE_VERSION,
            "unsupported fixture version: {} (expected {FIXTURE_VERSION})",
            self.version
        );
        anyhow::ensure!(
            !self.replay_model.is_empty(),
            "fixture missing replay_model"
        );
        anyhow::ensure!(
            self.replay_model.matches("--").count() >= 2,
            "replay_model must be `<vendor>--<protocol>--<scenario>`: got `{}`",
            self.replay_model
        );
        Ok(())
    }
}

/// Strip sensitive headers in place. Header names are matched
/// case-insensitively against `SENSITIVE_HEADER_BLACKLIST`.
pub fn scrub_sensitive_headers(headers: &mut BTreeMap<String, String>) {
    headers.retain(|name, _| {
        let lower = name.to_ascii_lowercase();
        !SENSITIVE_HEADER_BLACKLIST.contains(&lower.as_str())
    });
}

/// Recursively collect `*.jsonl` files under `root`.
pub fn scan_jsonl_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
            out.push(path.to_path_buf());
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn sample() -> Fixture {
        Fixture {
            version: FIXTURE_VERSION,
            replay_model: "deepseek--openai-chat--basic-stream".into(),
            scenario: "basic-stream".into(),
            vendor: "deepseek".into(),
            protocol: "openai-chat".into(),
            recorded_at: "2026-04-25T22:13:45Z".into(),
            request: RecordedRequest {
                method: "POST".into(),
                path: "/v1/chat/completions".into(),
                headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
                body_json: serde_json::json!({"model": "deepseek-chat"}),
            },
            response: RecordedResponse {
                status: 200,
                headers: BTreeMap::from([(
                    "content-type".into(),
                    "text/event-stream".into(),
                )]),
                body_base64: base64::engine::general_purpose::STANDARD
                    .encode(b"data: hello\n\ndata: [DONE]\n\n"),
            },
        }
    }

    #[test]
    fn round_trip_jsonl() {
        let fx = sample();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("basic-stream.jsonl");
        fx.save(&path).unwrap();
        let loaded = Fixture::load(&path).unwrap();
        loaded.validate().unwrap();
        assert_eq!(loaded.replay_model, fx.replay_model);
        assert_eq!(loaded.response_body().unwrap(), b"data: hello\n\ndata: [DONE]\n\n");
    }

    #[test]
    fn scrub_blacklisted_headers() {
        let mut h: BTreeMap<String, String> = [
            ("Authorization", "Bearer sk-xxx"),
            ("X-Api-Key", "secret"),
            ("Content-Type", "application/json"),
            ("x-goog-api-key", "another"),
            ("X-Custom-Safe", "value"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        scrub_sensitive_headers(&mut h);
        assert!(h.contains_key("Content-Type"));
        assert!(h.contains_key("X-Custom-Safe"));
        assert!(!h.keys().any(|k| k.eq_ignore_ascii_case("authorization")));
        assert!(!h.keys().any(|k| k.eq_ignore_ascii_case("x-api-key")));
        assert!(!h.keys().any(|k| k.eq_ignore_ascii_case("x-goog-api-key")));
    }

    #[test]
    fn rejects_invalid_replay_model() {
        let mut fx = sample();
        fx.replay_model = "wrong".into();
        assert!(fx.validate().is_err());
    }

    #[test]
    fn scan_finds_only_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a/b")).unwrap();
        std::fs::write(dir.path().join("a/x.jsonl"), "{}\n").unwrap();
        std::fs::write(dir.path().join("a/b/y.jsonl"), "{}\n").unwrap();
        std::fs::write(dir.path().join("a/notes.txt"), "ignore").unwrap();
        let files = scan_jsonl_files(dir.path()).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|p| p.extension().unwrap() == "jsonl"));
    }
}
