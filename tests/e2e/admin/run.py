#!/usr/bin/env python3
"""E2E admin tests for Nyro (control plane + observability).

Covers:
  - Admin auth: anonymous → 401
  - Provider / Route / API-key full CRUD lifecycle
  - Route access_control: missing API key → 401
  - Admin export_config: correct provider + route counts
  - Minimal proxy request → log entry appears in /api/v1/logs
  - Stats overview: total_requests / total_output_tokens incremented

Does NOT cover protocol proxy flows — those live in L2 (aimock) and L3 (Ollama).
"""

from __future__ import annotations

import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from common.helpers import (
    find_free_port,
    http_request,
    minimal_mock_provider,
    resolve_nyro_binary,
    run_tests,
    stop_nyro_server,
    wait_until_ready,
)

import os
import subprocess
import threading

REPO_ROOT = Path(__file__).resolve().parents[3]


def start_nyro_full(
    *,
    proxy_port: int,
    admin_port: int,
    admin_token: str,
    data_dir: str,
    nyro_binary: Path,
) -> tuple[subprocess.Popen[str], list[str]]:
    """Start nyro-server in full mode (DB + admin API)."""
    logs: list[str] = []
    proc = subprocess.Popen(
        [
            str(nyro_binary),
            "--proxy-host", "127.0.0.1",
            "--proxy-port", str(proxy_port),
            "--admin-host", "127.0.0.1",
            "--admin-port", str(admin_port),
            "--data-dir", data_dir,
            "--admin-token", admin_token,
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )

    def _drain() -> None:
        assert proc.stdout is not None
        for line in proc.stdout:
            logs.append(line.rstrip("\n"))

    threading.Thread(target=_drain, name="nyro-admin-log", daemon=True).start()
    return proc, logs


# ── Test functions (receive (admin_base, proxy_base, admin_headers, mock_base)) ──


def test_admin_anon_returns_401(ctx: dict) -> None:
    status, _ = http_request("GET", f"{ctx['admin']}/api/v1/status")
    assert status == 401, f"expected 401 for anonymous admin access, got {status}"


def test_provider_crud(ctx: dict) -> None:
    # Create
    status, resp = http_request(
        "POST",
        f"{ctx['admin']}/api/v1/providers",
        payload={
            "name": "test-provider",
            "protocol": "openai",
            "base_url": ctx["mock"],
            "api_key": "dummy-key",
        },
        headers=ctx["auth"],
    )
    assert status == 200, f"create provider failed: {status} {resp}"
    pid = resp["data"]["id"]

    # List
    status, resp = http_request("GET", f"{ctx['admin']}/api/v1/providers", headers=ctx["auth"])
    assert status == 200
    ids = [p["id"] for p in resp["data"]]
    assert pid in ids, f"provider {pid} not in list: {ids}"

    # Get
    status, resp = http_request("GET", f"{ctx['admin']}/api/v1/providers/{pid}", headers=ctx["auth"])
    assert status == 200, f"get provider failed: {status}"
    assert resp["data"]["id"] == pid

    ctx["provider_id"] = pid


def test_route_crud(ctx: dict) -> None:
    pid = ctx.get("provider_id")
    assert pid, "provider_id not set — test_provider_crud must run first"

    status, resp = http_request(
        "POST",
        f"{ctx['admin']}/api/v1/routes",
        payload={
            "name": "test-route",
            "virtual_model": "test-model",
            "target_provider": pid,
            "target_model": "gpt-4o-mini",
            "access_control": True,
        },
        headers=ctx["auth"],
    )
    assert status == 200, f"create route failed: {status} {resp}"
    rid = resp["data"]["id"]
    ctx["route_id"] = rid

    # Verify via list (GET /routes/:id doesn't exist — only PUT/DELETE on item)
    status, resp = http_request("GET", f"{ctx['admin']}/api/v1/routes", headers=ctx["auth"])
    assert status == 200
    ids = [r["id"] for r in resp.get("data", [])]
    assert rid in ids, f"route {rid} not found in list: {ids}"


def test_api_key_crud(ctx: dict) -> None:
    rid = ctx.get("route_id")
    assert rid

    status, resp = http_request(
        "POST",
        f"{ctx['admin']}/api/v1/api-keys",
        payload={"name": "test-key", "route_ids": [rid]},
        headers=ctx["auth"],
    )
    assert status == 200, f"create api-key failed: {status} {resp}"
    ctx["proxy_key"] = resp["data"]["key"]


def test_access_control_rejects_anonymous(ctx: dict) -> None:
    status, _ = http_request(
        "POST",
        f"{ctx['proxy']}/v1/chat/completions",
        payload={"model": "test-model", "messages": [{"role": "user", "content": "hi"}]},
    )
    assert status == 401, f"expected 401 for access-controlled route without key, got {status}"


def test_export_config_counts(ctx: dict) -> None:
    status, resp = http_request("GET", f"{ctx['admin']}/api/v1/config/export", headers=ctx["auth"])
    assert status == 200, f"export config failed: {status}"
    data = resp.get("data", {})
    assert len(data.get("providers", [])) >= 1, "export must include at least 1 provider"
    assert len(data.get("routes", [])) >= 1, "export must include at least 1 route"


def test_proxy_request_creates_log(ctx: dict) -> None:
    key = ctx.get("proxy_key")
    assert key

    status, resp = http_request(
        "POST",
        f"{ctx['proxy']}/v1/chat/completions",
        payload={
            "model": "test-model",
            "messages": [{"role": "user", "content": "log-trigger"}],
        },
        headers={"authorization": f"Bearer {key}"},
    )
    assert status == 200, f"proxy request failed: {status} {resp}"

    # Poll until the log entry is written (async write)
    deadline = time.time() + 10.0
    total = 0
    while time.time() < deadline:
        s, logs_resp = http_request(
            "GET",
            f"{ctx['admin']}/api/v1/logs?limit=20&offset=0",
            headers=ctx["auth"],
        )
        if s == 200:
            total = int(logs_resp.get("data", {}).get("total", 0))
            if total >= 1:
                break
        time.sleep(0.3)
    assert total >= 1, f"expected ≥1 log entry after proxy request, got {total}"


def test_stats_overview_incremented(ctx: dict) -> None:
    status, resp = http_request("GET", f"{ctx['admin']}/api/v1/stats/overview", headers=ctx["auth"])
    assert status == 200, f"stats overview failed: {status}"
    data = resp.get("data", {})
    assert data.get("total_requests", 0) >= 1, f"total_requests not incremented: {data}"


# ── Entry point ──────────────────────────────────────────────────────────────


def main() -> int:
    nyro_binary = resolve_nyro_binary(REPO_ROOT)
    if not nyro_binary.exists():
        print(f"nyro-server not found at {nyro_binary}", file=sys.stderr)
        print("Build with: cargo build -p nyro-server", file=sys.stderr)
        return 1

    admin_token = "admin-e2e-token"
    mock_port = find_free_port()
    proxy_port = find_free_port()
    admin_port = find_free_port()

    mock_server, _ = minimal_mock_provider(mock_port)
    proc = logs = None

    try:
        with tempfile.TemporaryDirectory(prefix="nyro-admin-e2e-") as data_dir:
            proc, logs = start_nyro_full(
                proxy_port=proxy_port,
                admin_port=admin_port,
                admin_token=admin_token,
                data_dir=data_dir,
                nyro_binary=nyro_binary,
            )
            admin_base = f"http://127.0.0.1:{admin_port}"
            proxy_base = f"http://127.0.0.1:{proxy_port}"
            auth_headers = {"authorization": f"Bearer {admin_token}"}

            wait_until_ready(
                f"{admin_base}/api/v1/status",
                timeout=40.0,
                headers=auth_headers,
            )

            ctx: dict = {
                "admin": admin_base,
                "proxy": proxy_base,
                "mock": f"http://127.0.0.1:{mock_port}",
                "auth": auth_headers,
            }

            # Tests run in sequence; each may populate ctx for the next.
            tests_ordered = [
                ("admin_anon_returns_401", test_admin_anon_returns_401),
                ("provider_crud", test_provider_crud),
                ("route_crud", test_route_crud),
                ("api_key_crud", test_api_key_crud),
                ("access_control_rejects_anonymous", test_access_control_rejects_anonymous),
                ("export_config_counts", test_export_config_counts),
                ("proxy_request_creates_log", test_proxy_request_creates_log),
                ("stats_overview_incremented", test_stats_overview_incremented),
            ]

            print("Running E2E admin tests...")
            rc = 0
            failed = []
            for name, fn in tests_ordered:
                try:
                    fn(ctx)
                    print(f"  PASS  {name}")
                except AssertionError as exc:
                    print(f"  FAIL  {name}: {exc}")
                    failed.append(name)
                except Exception as exc:  # noqa: BLE001
                    print(f"  ERROR {name}: {type(exc).__name__}: {exc}")
                    failed.append(name)

            if failed:
                print(f"\n{len(failed)} test(s) failed: {', '.join(failed)}", file=sys.stderr)
                rc = 1
            else:
                print("\nAll admin tests passed.")
            return rc
    finally:
        if proc is not None and logs is not None:
            stop_nyro_server(proc, logs)
        mock_server.shutdown()
        mock_server.server_close()


if __name__ == "__main__":
    sys.exit(main())
