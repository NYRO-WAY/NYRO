#!/usr/bin/env python3
"""Smoke test storage backends for Nyro.

Covers, per backend:
- provider / route / api key admin writes
- list / export admin reads
- proxy auth: missing key => 401
- proxy pass-through with valid key => 200
- log persistence + stats after proxy request

Backends:
- sqlite
- postgres (requires DB_URL, falls back to /workspace/.env)
"""

from __future__ import annotations

import argparse
import json
import os
import secrets
import socket
import subprocess
import sys
import tempfile
import textwrap
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any


REPO_ROOT = Path("/workspace/reference-projects/nyro")
WORKSPACE_ENV = Path("/workspace/.env")


def assert_true(cond: bool, msg: str) -> None:
    if not cond:
        raise AssertionError(msg)


def find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def load_workspace_env() -> dict[str, str]:
    env: dict[str, str] = {}
    if not WORKSPACE_ENV.exists():
        return env

    for line in WORKSPACE_ENV.read_text().splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#") or "=" not in stripped:
            continue
        key, value = stripped.split("=", 1)
        env[key.strip()] = value.strip().strip("'").strip('"')
    return env


def build_env() -> dict[str, str]:
    env = os.environ.copy()
    for key, value in load_workspace_env().items():
        env.setdefault(key, value)
    return env


def make_isolated_name(prefix: str, fallback_prefix: str, *, max_len: int = 63) -> str:
    normalized = "".join(ch if ch.isalnum() else "_" for ch in prefix.strip().lower()).strip("_")
    if not normalized:
        normalized = fallback_prefix
    suffix = f"{int(time.time())}_{secrets.token_hex(3)}"
    keep = max(1, max_len - len(suffix) - 1)
    return f"{normalized[:keep]}_{suffix}"


class MockProviderHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt: str, *args: Any) -> None:
        return

    def _read_json_body(self) -> dict[str, Any]:
        length = int(self.headers.get("content-length", "0"))
        raw = self.rfile.read(length) if length else b"{}"
        return json.loads(raw.decode("utf-8")) if raw else {}

    def _write_json(self, status: int, payload: dict[str, Any]) -> None:
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.send_header("connection", "close")
        self.end_headers()
        self.wfile.write(body)
        self.wfile.flush()

    def do_POST(self) -> None:  # noqa: N802
        path = self.path.split("?", 1)[0]
        body = self._read_json_body()
        if path != "/v1/chat/completions":
            self._write_json(404, {"error": "not found"})
            return

        model = str(body.get("model", "mock-model"))
        self._write_json(
            200,
            {
                "id": "chatcmpl-storage-smoke",
                "object": "chat.completion",
                "model": model,
                "choices": [
                    {
                        "index": 0,
                        "message": {"role": "assistant", "content": "smoke-ok"},
                        "finish_reason": "stop",
                    }
                ],
                "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5},
            },
        )


def start_mock_provider() -> tuple[ThreadingHTTPServer, int]:
    port = find_free_port()
    server = ThreadingHTTPServer(("127.0.0.1", port), MockProviderHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, port


def write_harness(project_dir: Path) -> None:
    cargo_toml = textwrap.dedent(
        f"""
        [package]
        name = "nyro-storage-smoke-harness"
        version = "0.1.0"
        edition = "2024"

        [dependencies]
        anyhow = "1"
        axum = "0.7"
        nyro-core = {{ path = "{REPO_ROOT / 'crates/nyro-core'}" }}
        reqwest = {{ version = "0.12", features = ["json"] }}
        serde_json = "1"
        sqlx = {{ version = "0.8", features = ["runtime-tokio", "postgres"] }}
        tokio = {{ version = "1", features = ["macros", "rt-multi-thread", "time"] }}
        """
    ).strip() + "\n"

    main_rs = textwrap.dedent(
        r"""
        use std::env;
        use std::path::PathBuf;
        use std::time::Duration;

        use anyhow::{Context, ensure};
        use nyro_core::config::{GatewayConfig, SqlStorageConfig, StorageBackendKind};
        use nyro_core::db::models::{CreateApiKey, CreateProvider, CreateRoute, LogQuery};
        use nyro_core::{logging, Gateway};
        use reqwest::StatusCode;
        use sqlx::postgres::PgPoolOptions;

        #[tokio::main]
        async fn main() -> anyhow::Result<()> {
            let backend = env::var("NYRO_SMOKE_BACKEND").context("missing NYRO_SMOKE_BACKEND")?;
            let upstream_base_url =
                env::var("NYRO_SMOKE_UPSTREAM_BASE_URL").context("missing NYRO_SMOKE_UPSTREAM_BASE_URL")?;
            let data_dir = PathBuf::from(env::var("NYRO_SMOKE_DATA_DIR").context("missing NYRO_SMOKE_DATA_DIR")?);
            let proxy_port: u16 = env::var("NYRO_SMOKE_PROXY_PORT")
                .context("missing NYRO_SMOKE_PROXY_PORT")?
                .parse()
                .context("invalid NYRO_SMOKE_PROXY_PORT")?;

            let mut config = GatewayConfig {
                proxy_host: "127.0.0.1".to_string(),
                proxy_port,
                data_dir,
                ..Default::default()
            };

            match backend.as_str() {
                "sqlite" => {
                    config.storage.backend = StorageBackendKind::Sqlite;
                    config.storage.sqlite.migrate_on_start = true;
                }
                "postgres" => {
                    let base_url = env::var("NYRO_SMOKE_PG_BASE_URL").context("missing NYRO_SMOKE_PG_BASE_URL")?;
                    let schema = env::var("NYRO_SMOKE_PG_SCHEMA").context("missing NYRO_SMOKE_PG_SCHEMA")?;
                    ensure!(
                        schema
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '_'),
                        "invalid schema name"
                    );

                    let pool = PgPoolOptions::new().max_connections(1).connect(&base_url).await?;
                    let create_sql = format!("CREATE SCHEMA IF NOT EXISTS {schema}");
                    sqlx::query(&create_sql).execute(&pool).await?;
                    pool.close().await;

                    config.storage.backend = StorageBackendKind::Postgres;
                    config.storage.postgres = SqlStorageConfig {
                        url: Some(with_search_path(&base_url, &schema)),
                        ..Default::default()
                    };
                }
                other => anyhow::bail!("unsupported backend: {other}"),
            }

            let (gw, log_rx) = Gateway::new(config).await?;
            let storage = gw.storage.clone();
            tokio::spawn(async move {
                logging::run_collector(log_rx, storage).await;
            });

            let admin = gw.admin();
            let provider = admin
                .create_provider(CreateProvider {
                    name: format!("{backend}-smoke-provider"),
                    vendor: None,
                    protocol: "openai".to_string(),
                    base_url: format!("{upstream_base_url}/v1"),
                    preset_key: None,
                    channel: None,
                    models_source: None,
                    capabilities_source: None,
                    static_models: None,
                    api_key: "dummy-key".to_string(),
                })
                .await?;

            let route = admin
                .create_route(CreateRoute {
                    name: format!("{backend}-smoke-route"),
                    virtual_model: format!("{backend}-smoke-model"),
                    target_provider: provider.id.clone(),
                    target_model: "gpt-4o-mini".to_string(),
                    access_control: Some(true),
                })
                .await?;

            let api_key = admin
                .create_api_key(CreateApiKey {
                    name: format!("{backend}-smoke-key"),
                    rpm: Some(10),
                    rpd: None,
                    tpm: None,
                    tpd: None,
                    expires_at: None,
                    route_ids: vec![route.id.clone()],
                })
                .await?;

            ensure!(admin.list_providers().await?.len() == 1, "provider list count mismatch");
            ensure!(admin.list_routes().await?.len() == 1, "route list count mismatch");
            ensure!(admin.list_api_keys().await?.len() == 1, "api key list count mismatch");

            let export = admin.export_config().await?;
            ensure!(export.providers.len() == 1, "export provider count mismatch");
            ensure!(export.routes.len() == 1, "export route count mismatch");

            let proxy = gw.clone();
            tokio::spawn(async move {
                let _ = proxy.start_proxy().await;
            });
            tokio::time::sleep(Duration::from_millis(250)).await;

            let client = reqwest::Client::new();
            let url = format!("http://127.0.0.1:{proxy_port}/v1/chat/completions");
            let payload = serde_json::json!({
                "model": format!("{backend}-smoke-model"),
                "messages": [{"role": "user", "content": "hello"}]
            });

            let no_key = client.post(&url).json(&payload).send().await?;
            ensure!(no_key.status() == StatusCode::UNAUTHORIZED, "missing key should return 401");

            let with_key = client.post(&url).bearer_auth(&api_key.key).json(&payload).send().await?;
            ensure!(with_key.status() == StatusCode::OK, "valid key should return 200");
            let with_key_body: serde_json::Value = with_key.json().await?;
            let content = with_key_body["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or_default();
            ensure!(content == "smoke-ok", "unexpected proxy body");

            let mut logs_total = 0;
            let mut stats_total_requests = 0;
            for _ in 0..20 {
                let logs = admin
                    .query_logs(LogQuery {
                        limit: Some(10),
                        offset: Some(0),
                        ..Default::default()
                    })
                    .await?;
                let overview = admin.get_stats_overview(None).await?;
                logs_total = logs.total;
                stats_total_requests = overview.total_requests;
                if logs_total >= 1 && stats_total_requests >= 1 && overview.total_output_tokens >= 1 {
                    println!("backend={backend}");
                    println!("provider_id={}", provider.id);
                    println!("route_id={}", route.id);
                    println!("api_key_id={}", api_key.id);
                    println!("logs_total={}", logs_total);
                    println!("stats_total_requests={}", stats_total_requests);
                    println!("proxy_status_valid=200");
                    println!("proxy_status_missing_key=401");
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }

            anyhow::bail!(
                "log/stat propagation timeout: logs_total={logs_total}, stats_total_requests={stats_total_requests}"
            );
        }

        fn with_search_path(base_url: &str, schema: &str) -> String {
            let opt = format!("options=-csearch_path%3D{schema}");
            if base_url.contains('?') {
                format!("{base_url}&{opt}")
            } else {
                format!("{base_url}?{opt}")
            }
        }
        """
    ).strip() + "\n"

    (project_dir / "Cargo.toml").write_text(cargo_toml)
    src_dir = project_dir / "src"
    src_dir.mkdir(parents=True, exist_ok=True)
    (src_dir / "main.rs").write_text(main_rs)


def run_cmd(cmd: list[str], *, env: dict[str, str], cwd: Path) -> str:
    proc = subprocess.run(
        cmd,
        cwd=cwd,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"command failed: {' '.join(cmd)}\nstdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )
    return proc.stdout


def run_backend(backend: str, *, env: dict[str, str], upstream_port: int, work_dir: Path) -> str:
    backend_env = env.copy()
    backend_env["NYRO_SMOKE_BACKEND"] = backend
    backend_env["NYRO_SMOKE_UPSTREAM_BASE_URL"] = f"http://127.0.0.1:{upstream_port}"
    backend_env["NYRO_SMOKE_PROXY_PORT"] = str(find_free_port())
    backend_env["NYRO_SMOKE_DATA_DIR"] = str(work_dir / f"{backend}-data")

    if backend == "postgres":
        db_url = backend_env.get("DB_URL") or backend_env.get("DATABASE_URL")
        if not db_url:
            raise RuntimeError("postgres smoke requires DB_URL or DATABASE_URL")
        backend_env["NYRO_SMOKE_PG_BASE_URL"] = db_url
        backend_env["NYRO_SMOKE_PG_SCHEMA"] = make_isolated_name("nyro_pg_smoke", "nyro_pg_smoke")
    return run_cmd(
        ["cargo", "run", "--quiet", "--manifest-path", str(work_dir / "Cargo.toml")],
        env=backend_env,
        cwd=REPO_ROOT,
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Smoke test Nyro storage backends")
    parser.add_argument(
        "--backend",
        action="append",
        choices=["sqlite", "postgres"],
        help="Backend(s) to test. Defaults to sqlite + postgres.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    backends = args.backend or ["sqlite", "postgres"]
    env = build_env()

    server, upstream_port = start_mock_provider()
    try:
        with tempfile.TemporaryDirectory(prefix="nyro-storage-smoke-") as tmp:
            tmpdir = Path(tmp)
            write_harness(tmpdir)

            for backend in backends:
                started = time.time()
                output = run_backend(backend, env=env, upstream_port=upstream_port, work_dir=tmpdir)
                elapsed = time.time() - started
                print(f"[{backend}] ok ({elapsed:.1f}s)")
                print(output.strip())
    finally:
        server.shutdown()
        server.server_close()

    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:  # noqa: BLE001
        print(f"[storage-smoke] FAILED: {exc}", file=sys.stderr)
        raise SystemExit(1)
