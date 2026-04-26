//! `nyro-tools replay` — persistent stub upstream that serves recorded
//! fixtures by full-string `replay_model` HashMap lookup.
//!
//! Behaviour:
//! 1. Scan `--input-dir` recursively for `*.jsonl`, load each as a Fixture.
//! 2. Build `HashMap<replay_model, Fixture>`. Reject duplicates at startup.
//! 3. Serve the protocol's ingress path; on each request:
//!    - extract model name (body or path),
//!    - look up the Fixture, base64-decode `response.body_base64`,
//!    - write back as raw bytes with original status + headers.
//! 4. Unknown model → 404 JSON error listing all loaded keys.

use crate::fixture::{Fixture, scan_jsonl_files};
use crate::protocol::ProtocolKind;
use anyhow::{Context, Result, bail};
use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use clap::Args;
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Debug, Args)]
pub struct ReplayArgs {
    /// Listen port
    #[arg(short = 'P', long, default_value_t = 25208)]
    pub port: u16,

    /// Listen host
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    pub host: String,

    /// Protocol this replay instance serves (selects ingress path)
    #[arg(short = 'p', long, value_enum)]
    pub protocol: ProtocolKind,

    /// Directory holding recorded .jsonl fixtures (recursively scanned)
    #[arg(short = 'i', long)]
    pub input_dir: PathBuf,
}

#[derive(Debug)]
struct ReplayState {
    protocol: ProtocolKind,
    index: HashMap<String, Fixture>,
}

impl ReplayState {
    fn load(protocol: ProtocolKind, input_dir: &std::path::Path) -> Result<Self> {
        let files = scan_jsonl_files(input_dir)
            .with_context(|| format!("failed to scan {}", input_dir.display()))?;
        let mut index: HashMap<String, Fixture> = HashMap::new();
        for path in &files {
            let fx = Fixture::load(path)
                .with_context(|| format!("failed to load fixture {}", path.display()))?;
            fx.validate()?;
            // Filter by protocol so a single replay instance only serves
            // fixtures matching its declared protocol.
            if fx.protocol != protocol.as_short_name() {
                continue;
            }
            if let Some(prev) = index.get(&fx.replay_model) {
                bail!(
                    "duplicate replay_model `{}` found in two fixtures:\n  - {}\n  - (already loaded for vendor {})",
                    fx.replay_model,
                    path.display(),
                    prev.vendor
                );
            }
            index.insert(fx.replay_model.clone(), fx);
        }
        info!(
            protocol = %protocol,
            input_dir = %input_dir.display(),
            scanned = files.len(),
            loaded = index.len(),
            "replay state initialised"
        );
        Ok(Self { protocol, index })
    }

    fn loaded_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.index.keys().cloned().collect();
        keys.sort();
        keys
    }
}

pub async fn run(args: ReplayArgs) -> Result<()> {
    let state = Arc::new(ReplayState::load(args.protocol, &args.input_dir)?);
    let app = build_router(args.protocol, state.clone());

    let host: std::net::IpAddr = args
        .host
        .parse()
        .with_context(|| format!("invalid host `{}`", args.host))?;
    let addr = SocketAddr::from((host, args.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    info!(%addr, protocol = %args.protocol, "nyro-tools replay listening");
    axum::serve(listener, app).await?;
    Ok(())
}

fn build_router(protocol: ProtocolKind, state: Arc<ReplayState>) -> Router {
    let mut router = Router::new().route("/", axum::routing::get(health));
    router = match protocol {
        ProtocolKind::OpenAiChat => router.route("/v1/chat/completions", post(handle_body_model)),
        ProtocolKind::OpenAiResponses => router.route("/v1/responses", post(handle_body_model)),
        ProtocolKind::AnthropicMessages => router.route("/v1/messages", post(handle_body_model)),
        ProtocolKind::GoogleContent => router.route(
            "/v1beta/models/*tail",
            post(handle_google_path_model),
        ),
    };
    router.with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn handle_body_model(
    State(state): State<Arc<ReplayState>>,
    body: Json<Value>,
) -> Response {
    let path = state.protocol.ingress_path_template().to_string();
    let model = match state.protocol.extract_request_model(&path, &body) {
        Ok(m) => m,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e.to_string(), state.loaded_keys()),
    };
    serve_fixture(&state, &model)
}

async fn handle_google_path_model(
    State(state): State<Arc<ReplayState>>,
    AxumPath(tail): AxumPath<String>,
    _body: Json<Value>,
) -> Response {
    // axum strips the leading `/v1beta/models/`; rebuild the full path so
    // `parse_google_content_model` can locate the action segment.
    let full_path = format!("/v1beta/models/{tail}");
    let model = match state
        .protocol
        .extract_request_model(&full_path, &Value::Null)
    {
        Ok(m) => m,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e.to_string(), state.loaded_keys()),
    };
    serve_fixture(&state, &model)
}

fn serve_fixture(state: &ReplayState, model: &str) -> Response {
    let Some(fx) = state.index.get(model) else {
        warn!(%model, "replay miss");
        return error_response(
            StatusCode::NOT_FOUND,
            &format!("no fixture for replay_model `{model}`"),
            state.loaded_keys(),
        );
    };
    let body = match fx.response_body() {
        Ok(b) => b,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("failed to decode fixture body: {e}"),
                state.loaded_keys(),
            );
        }
    };
    let mut response = Response::builder()
        .status(StatusCode::from_u16(fx.response.status).unwrap_or(StatusCode::OK));
    let headers_map = response.headers_mut().expect("response builder valid");
    *headers_map = build_response_headers(&fx.response.headers);
    response.body(Body::from(body)).expect("valid body").into_response()
}

fn build_response_headers(stored: &std::collections::BTreeMap<String, String>) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (name, value) in stored {
        let Ok(name) = HeaderName::try_from(name.as_str()) else {
            continue;
        };
        // axum will set transfer-encoding/content-length for us; skip them
        // to avoid double-encoding.
        if matches!(
            name.as_str(),
            "transfer-encoding" | "content-length" | "connection"
        ) {
            continue;
        }
        if let Ok(v) = HeaderValue::from_str(value) {
            out.insert(name, v);
        }
    }
    out
}

fn error_response(status: StatusCode, message: &str, available: Vec<String>) -> Response {
    let body = serde_json::json!({
        "error": {
            "code": status.as_u16(),
            "message": message,
            "available_replay_models": available,
        }
    });
    (status, Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::{FIXTURE_VERSION, RecordedRequest, RecordedResponse};
    use base64::Engine;
    use std::collections::BTreeMap;

    fn write_fx(dir: &std::path::Path, replay_model: &str, body: &[u8]) {
        let parts: Vec<&str> = replay_model.split("--").collect();
        let fx = Fixture {
            version: FIXTURE_VERSION,
            replay_model: replay_model.to_string(),
            scenario: parts[2].to_string(),
            vendor: parts[0].to_string(),
            protocol: parts[1].to_string(),
            recorded_at: "2026-01-01T00:00:00Z".to_string(),
            request: RecordedRequest {
                method: "POST".into(),
                path: "/v1/chat/completions".into(),
                headers: BTreeMap::new(),
                body_json: serde_json::json!({"model": replay_model}),
            },
            response: RecordedResponse {
                status: 200,
                headers: BTreeMap::from([(
                    "content-type".into(),
                    "text/event-stream".into(),
                )]),
                body_base64: base64::engine::general_purpose::STANDARD.encode(body),
            },
        };
        let path = dir.join(format!("{}.jsonl", parts[2]));
        fx.save(&path).unwrap();
    }

    #[test]
    fn loads_and_indexes_fixtures() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("openai-chat/deepseek");
        std::fs::create_dir_all(&dir).unwrap();
        write_fx(&dir, "deepseek--openai-chat--basic-stream", b"hello stream");
        write_fx(&dir, "deepseek--openai-chat--reasoning-stream", b"hi reason");

        let state = ReplayState::load(ProtocolKind::OpenAiChat, tmp.path()).unwrap();
        assert_eq!(state.index.len(), 2);
        assert!(state.index.contains_key("deepseek--openai-chat--basic-stream"));
        assert!(state.index.contains_key("deepseek--openai-chat--reasoning-stream"));
    }

    #[test]
    fn rejects_duplicate_replay_model() {
        let tmp = tempfile::tempdir().unwrap();
        let dir1 = tmp.path().join("openai-chat/deepseek");
        let dir2 = tmp.path().join("openai-chat/azure");
        std::fs::create_dir_all(&dir1).unwrap();
        std::fs::create_dir_all(&dir2).unwrap();
        write_fx(&dir1, "deepseek--openai-chat--basic-stream", b"a");
        // craft a second fixture with the same replay_model but in another vendor dir
        let parts: Vec<&str> = "deepseek--openai-chat--basic-stream".split("--").collect();
        let fx = Fixture {
            version: FIXTURE_VERSION,
            replay_model: "deepseek--openai-chat--basic-stream".into(),
            scenario: parts[2].to_string(),
            vendor: "azure".into(),
            protocol: parts[1].to_string(),
            recorded_at: "x".into(),
            request: RecordedRequest {
                method: "POST".into(),
                path: "/v1/chat/completions".into(),
                headers: BTreeMap::new(),
                body_json: serde_json::Value::Null,
            },
            response: RecordedResponse {
                status: 200,
                headers: BTreeMap::new(),
                body_base64: String::new(),
            },
        };
        fx.save(&dir2.join("basic-stream.jsonl")).unwrap();

        let err = ReplayState::load(ProtocolKind::OpenAiChat, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("duplicate replay_model"));
    }

    #[test]
    fn ignores_other_protocol_fixtures() {
        let tmp = tempfile::tempdir().unwrap();
        let openai = tmp.path().join("openai-chat/deepseek");
        let google = tmp.path().join("google-content/google-aistudio");
        std::fs::create_dir_all(&openai).unwrap();
        std::fs::create_dir_all(&google).unwrap();
        write_fx(&openai, "deepseek--openai-chat--basic-stream", b"x");
        write_fx(
            &google,
            "google-aistudio--google-content--basic-stream",
            b"y",
        );

        let state = ReplayState::load(ProtocolKind::OpenAiChat, tmp.path()).unwrap();
        assert_eq!(state.index.len(), 1);
        assert!(state.index.contains_key("deepseek--openai-chat--basic-stream"));
    }

    async fn spawn_replay(
        protocol: ProtocolKind,
        state: Arc<ReplayState>,
    ) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let app = build_router(protocol, state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });
        // give the server a moment to start accepting
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        (addr, handle)
    }

    #[tokio::test]
    async fn end_to_end_openai_chat_byte_replay() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("openai-chat/deepseek");
        std::fs::create_dir_all(&dir).unwrap();
        let body = b"data: {\"choices\":[{\"delta\":{\"content\":\"NYRO_PROBE_BASIC_STREAM\"}}]}\n\ndata: [DONE]\n\n";
        write_fx(&dir, "deepseek--openai-chat--basic-stream", body);

        let state = Arc::new(ReplayState::load(ProtocolKind::OpenAiChat, tmp.path()).unwrap());
        let (addr, handle) = spawn_replay(ProtocolKind::OpenAiChat, state).await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://{addr}/v1/chat/completions"))
            .json(&serde_json::json!({"model": "deepseek--openai-chat--basic-stream", "stream": true}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.headers()["content-type"], "text/event-stream");
        let bytes = resp.bytes().await.unwrap();
        assert_eq!(&bytes[..], &body[..]);

        handle.abort();
    }

    #[tokio::test]
    async fn end_to_end_google_content_path_model() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("google-content/google-aistudio");
        std::fs::create_dir_all(&dir).unwrap();
        let replay_model = "google-aistudio--google-content--basic-stream";
        let body = b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"NYRO_PROBE_BASIC_STREAM\"}]}}]}\n\n";
        // adapt write_fx to set protocol = google-content
        let parts: Vec<&str> = replay_model.split("--").collect();
        let fx = Fixture {
            version: FIXTURE_VERSION,
            replay_model: replay_model.to_string(),
            scenario: parts[2].to_string(),
            vendor: parts[0].to_string(),
            protocol: parts[1].to_string(),
            recorded_at: "2026-01-01T00:00:00Z".to_string(),
            request: RecordedRequest {
                method: "POST".into(),
                path: format!("/v1beta/models/{replay_model}:streamGenerateContent"),
                headers: BTreeMap::new(),
                body_json: serde_json::Value::Null,
            },
            response: RecordedResponse {
                status: 200,
                headers: BTreeMap::from([(
                    "content-type".into(),
                    "text/event-stream".into(),
                )]),
                body_base64: base64::engine::general_purpose::STANDARD.encode(body),
            },
        };
        fx.save(&dir.join("basic-stream.jsonl")).unwrap();

        let state = Arc::new(ReplayState::load(ProtocolKind::GoogleContent, tmp.path()).unwrap());
        let (addr, handle) = spawn_replay(ProtocolKind::GoogleContent, state).await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!(
                "http://{addr}/v1beta/models/{replay_model}:streamGenerateContent"
            ))
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let bytes = resp.bytes().await.unwrap();
        assert_eq!(&bytes[..], &body[..]);

        handle.abort();
    }

    #[tokio::test]
    async fn replay_miss_returns_404_with_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("openai-chat/deepseek");
        std::fs::create_dir_all(&dir).unwrap();
        write_fx(&dir, "deepseek--openai-chat--basic-stream", b"x");

        let state = Arc::new(ReplayState::load(ProtocolKind::OpenAiChat, tmp.path()).unwrap());
        let (addr, handle) = spawn_replay(ProtocolKind::OpenAiChat, state).await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://{addr}/v1/chat/completions"))
            .json(&serde_json::json!({"model": "no-such-model"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
        let body: serde_json::Value = resp.json().await.unwrap();
        let available = body["error"]["available_replay_models"].as_array().unwrap();
        assert!(available.iter().any(|v| v == "deepseek--openai-chat--basic-stream"));

        handle.abort();
    }
}
