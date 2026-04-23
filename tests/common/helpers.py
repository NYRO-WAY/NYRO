"""Shared test utilities for Nyro E2E test suites."""

from __future__ import annotations

import json
import os
import re
import socket
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


# ── Port utilities ──────────────────────────────────────────────────────────


def find_free_port() -> int:
    """Bind to port 0 and return the OS-assigned ephemeral port."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def is_port_free(port: int) -> bool:
    """Return True if nothing is currently listening on 127.0.0.1:<port>."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.settimeout(0.3)
        try:
            s.connect(("127.0.0.1", port))
            return False
        except (ConnectionRefusedError, OSError):
            return True


# ── HTTP helpers ─────────────────────────────────────────────────────────────


def _decode_body(raw: bytes) -> Any:
    text = raw.decode("utf-8", errors="replace")
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return text


def http_request(
    method: str,
    url: str,
    payload: Any | None = None,
    headers: dict[str, str] | None = None,
    timeout: float = 15.0,
) -> tuple[int, Any]:
    """Make an HTTP request and return (status_code, decoded_body)."""
    hdrs: dict[str, str] = dict(headers or {})
    data: bytes | None = None
    if payload is not None:
        hdrs.setdefault("content-type", "application/json")
        data = json.dumps(payload).encode("utf-8")

    req = Request(url=url, method=method, data=data, headers=hdrs)
    try:
        with urlopen(req, timeout=timeout) as resp:
            return int(resp.status), _decode_body(resp.read())
    except HTTPError as e:
        return int(e.code), _decode_body(e.read())


def wait_until_ready(
    url: str,
    timeout: float = 30.0,
    headers: dict[str, str] | None = None,
) -> None:
    """Poll <url> until any non-connection-error response arrives (< 500)."""
    deadline = time.time() + timeout
    last_err: str = ""
    while time.time() < deadline:
        try:
            status, _ = http_request("GET", url, headers=headers, timeout=2.0)
            if status < 500:
                return
            last_err = f"status={status}"
        except (URLError, TimeoutError, OSError) as exc:
            last_err = str(exc)
        time.sleep(0.3)
    raise TimeoutError(f"server not ready at {url!r}: {last_err}")


# ── Nyro standalone.yaml template rendering ──────────────────────────────────


def render_standalone_yaml(
    template_path: Path,
    proxy_port: int,
    port_map: dict[str, int] | None = None,
) -> Path:
    """Render a standalone.yaml template to a temp file.

    Replaces the ``proxy_port`` numeric value and any ``{KEY}`` placeholders
    that appear in *port_map*.  Returns the path to the temporary file (the
    caller is responsible for deletion).
    """
    text = template_path.read_text(encoding="utf-8")
    # Replace proxy_port value
    text = re.sub(r"(proxy_port\s*:\s*)\d+", rf"\g<1>{proxy_port}", text)
    # Replace {KEY} placeholders (e.g. {AIMOCK_OPENAI_PORT})
    for key, val in (port_map or {}).items():
        text = text.replace(f"{{{key}}}", str(val))

    tmp = tempfile.NamedTemporaryFile(
        suffix=".yaml", mode="w", delete=False, encoding="utf-8"
    )
    tmp.write(text)
    tmp.close()
    return Path(tmp.name)


# ── Nyro server process helpers ───────────────────────────────────────────────


def resolve_nyro_binary(repo_root: Path) -> Path:
    """Find the nyro-server binary: $NYRO_BINARY env or debug build fallback."""
    env_bin = os.environ.get("NYRO_BINARY")
    if env_bin:
        candidate = Path(env_bin)
        if not candidate.is_absolute():
            candidate = repo_root / candidate
        return candidate
    return repo_root / "target" / "debug" / "nyro-server"


def start_nyro_server(
    config_path: Path,
    *,
    nyro_binary: Path,
) -> tuple[subprocess.Popen[str], list[str]]:
    """Start nyro-server with --config <config_path>; return (proc, log_lines)."""
    logs: list[str] = []
    proc = subprocess.Popen(
        [str(nyro_binary), "--config", str(config_path)],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )

    def _drain() -> None:
        assert proc.stdout is not None
        for line in proc.stdout:
            logs.append(line.rstrip("\n"))

    threading.Thread(target=_drain, name="nyro-server-log", daemon=True).start()
    return proc, logs


def stop_nyro_server(
    proc: subprocess.Popen[str],
    logs: list[str],
    *,
    print_tail: int = 80,
) -> None:
    """Gracefully terminate nyro-server and print tail logs on non-zero exit."""
    if proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=3)
    if proc.returncode not in (0, None, -15):
        tail = "\n".join(logs[-print_tail:])
        print("\n--- nyro-server logs (tail) ---", file=sys.stderr)
        print(tail, file=sys.stderr)


# ── Minimal mock provider ─────────────────────────────────────────────────────


class _MinimalMockHandler(BaseHTTPRequestHandler):
    """Single-endpoint OpenAI /v1/chat/completions happy-path mock."""

    protocol_version = "HTTP/1.1"

    def log_message(self, fmt: str, *args: Any) -> None:  # noqa: D401
        return

    def _read_body(self) -> dict[str, Any]:
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
        body = self._read_body()
        path = self.path.split("?", 1)[0]
        if path != "/v1/chat/completions":
            self._write_json(404, {"error": f"unknown path: {path}"})
            return
        model = str(body.get("model", "mock"))
        self._write_json(
            200,
            {
                "id": "chatcmpl-mock",
                "object": "chat.completion",
                "model": model,
                "choices": [
                    {
                        "index": 0,
                        "message": {"role": "assistant", "content": "mock-ok"},
                        "finish_reason": "stop",
                    }
                ],
                "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5},
            },
        )


def minimal_mock_provider(port: int) -> tuple[ThreadingHTTPServer, threading.Thread]:
    """Start a minimal OpenAI-compatible mock on *port*; return (server, thread)."""
    server = ThreadingHTTPServer(("127.0.0.1", port), _MinimalMockHandler)
    t = threading.Thread(target=server.serve_forever, name="mock-provider", daemon=True)
    t.start()
    return server, t


# ── Simple test runner ────────────────────────────────────────────────────────


def run_tests(tests: list[tuple[str, Any]], *args: Any) -> int:
    """Run a list of (name, fn) pairs; return 0 if all pass, 1 otherwise."""
    failed: list[str] = []
    for name, fn in tests:
        try:
            fn(*args)
            print(f"  PASS  {name}")
        except AssertionError as exc:
            print(f"  FAIL  {name}: {exc}")
            failed.append(name)
        except Exception as exc:  # noqa: BLE001
            print(f"  ERROR {name}: {type(exc).__name__}: {exc}")
            failed.append(name)
    if failed:
        print(f"\n{len(failed)} test(s) failed: {', '.join(failed)}", file=sys.stderr)
        return 1
    return 0
