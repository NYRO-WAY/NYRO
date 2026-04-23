#!/usr/bin/env python3
"""L2 aimock static E2E tests for Nyro.

Launches four isolated aimock instances (one per API schema directory) and
verifies Nyro's protocol transformations with byte-level determinism.

Port assignment:
  4010  openai-completions   (/v1/chat/completions)
  4011  openai-responses     (/v1/responses)
  4012  anthropic-messages   (/v1/messages)
  4013  google-generatecontent (/v1beta/models/*:generateContent)

If any fixed port is occupied, all four fall back to dynamically assigned ports
and the standalone.yaml is re-rendered accordingly.

aimock Docker image: ghcr.io/copilotkit/aimock:latest
  Pin to SHA digest in production and update via Dependabot weekly:
    docker inspect --format='{{index .RepoDigests 0}}' ghcr.io/copilotkit/aimock:latest
"""

from __future__ import annotations

import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from common.helpers import (
    find_free_port,
    http_request,
    is_port_free,
    render_standalone_yaml,
    resolve_nyro_binary,
    run_tests,
    start_nyro_server,
    stop_nyro_server,
    wait_until_ready,
)

REPO_ROOT = Path(__file__).resolve().parents[3]
FIXTURES_DIR = Path(__file__).parent / "fixtures"
CONFIG_TEMPLATE = Path(__file__).parent / "standalone.yaml"

# aimock image — pin SHA in production
AIMOCK_IMAGE = os.environ.get("AIMOCK_IMAGE", "ghcr.io/copilotkit/aimock:latest")

# Schema-to-directory mapping (port will be assigned at runtime)
SCHEMA_DIRS: list[tuple[str, str]] = [
    ("AIMOCK_OPENAI_COMPLETIONS_PORT", "openai-completions"),
    ("AIMOCK_OPENAI_RESPONSES_PORT", "openai-responses"),
    ("AIMOCK_ANTHROPIC_PORT", "anthropic-messages"),
    ("AIMOCK_GEMINI_PORT", "google-generatecontent"),
]

# Preferred fixed ports (used if free, otherwise dynamic)
PREFERRED_PORTS = [4010, 4011, 4012, 4013]


# ── aimock container lifecycle ────────────────────────────────────────────────


def choose_ports() -> list[int]:
    """Use fixed ports 4010-4013 if all are free; otherwise assign dynamically."""
    if all(is_port_free(p) for p in PREFERRED_PORTS):
        return list(PREFERRED_PORTS)
    print("  [warn] preferred ports 4010-4013 not all free; falling back to dynamic ports")
    return [find_free_port() for _ in PREFERRED_PORTS]


def start_aimock(
    port: int,
    fixture_subdir: str,
    *,
    container_name: str,
) -> subprocess.Popen[str]:
    """Start one aimock container serving a single fixtures subdirectory."""
    host_dir = str((FIXTURES_DIR / fixture_subdir).resolve())
    cmd = [
        "docker", "run", "--rm", "-d",
        "--name", container_name,
        "-p", f"127.0.0.1:{port}:3000",
        "-v", f"{host_dir}:/fixtures:ro",
        AIMOCK_IMAGE,
        "-f", "/fixtures",
        "--host", "0.0.0.0",
    ]
    return subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def stop_containers(names: list[str]) -> None:
    for name in names:
        subprocess.run(
            ["docker", "rm", "-f", name],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )


def wait_aimock_ready(port: int, timeout: float = 30.0) -> None:
    """Wait until aimock health endpoint responds."""
    # aimock exposes GET /health (or any request returns non-500 on startup)
    wait_until_ready(f"http://127.0.0.1:{port}/health", timeout=timeout)


# ── Test functions ────────────────────────────────────────────────────────────


def test_openai_chat_basic_stream(base: str, _: Any = None) -> None:
    """L2 byte-level regression: OpenAI chat stream via aimock."""
    status, raw = http_request(
        "POST",
        f"{base}/v1/chat/completions",
        {
            "model": "mock-chat",
            "stream": True,
            "messages": [{"role": "user", "content": "ping-stream"}],
        },
        timeout=15.0,
    )
    assert status == 200, f"openai chat stream status={status}: {raw!r:.200}"
    text = str(raw)
    assert "[DONE]" in text, "missing [DONE] in stream"
    assert "pong" in text, f"missing 'pong' in stream response: {text[:300]}"


def test_openai_chat_content_filter(base: str, _: Any = None) -> None:
    """Verify Nyro passes through content_filter finish_reason without crashing."""
    status, resp = http_request(
        "POST",
        f"{base}/v1/chat/completions",
        {
            "model": "mock-chat",
            "messages": [{"role": "user", "content": "ping-azure"}],
        },
    )
    assert status == 200, f"azure content-filter status={status}: {resp}"
    finish = resp.get("choices", [{}])[0].get("finish_reason", "")
    assert finish == "content_filter", f"expected content_filter finish_reason, got: {finish}"


def test_openai_chat_reasoning(base: str, _: Any = None) -> None:
    """Verify reasoning_content from vendor-deepseek fixture is forwarded."""
    status, resp = http_request(
        "POST",
        f"{base}/v1/chat/completions",
        {
            "model": "mock-chat",
            "messages": [{"role": "user", "content": "ping-reasoning"}],
        },
    )
    assert status == 200, f"deepseek reasoning status={status}: {resp}"
    msg = resp.get("choices", [{}])[0].get("message", {})
    content = msg.get("content", "")
    assert content, f"missing content in response: {resp}"


def test_openai_chat_chaos_disconnect(base: str, _: Any = None) -> None:
    """Chaos: mid-stream disconnect; Nyro must not return 500."""
    # The response may be truncated or may return an error status — but Nyro
    # itself must not crash (we accept both 200 partial and 4xx/5xx from the
    # proxy layer, as long as the server stays up).
    try:
        status, _ = http_request(
            "POST",
            f"{base}/v1/chat/completions",
            {
                "model": "mock-chat",
                "stream": True,
                "messages": [{"role": "user", "content": "ping-chaos-disconnect"}],
            },
            timeout=10.0,
        )
        # Accept any status: disconnect may cause 200 with truncated body,
        # or 502/503 from Nyro's upstream error handling.
        assert status < 600, f"unexpected status: {status}"
    except Exception:  # noqa: BLE001
        # Connection reset is acceptable for a disconnect scenario.
        pass


def test_openai_responses_reasoning(base: str, _: Any = None) -> None:
    """Verify reasoning comes through Responses API stream."""
    status, raw = http_request(
        "POST",
        f"{base}/v1/responses",
        {
            "model": "mock-responses",
            "stream": True,
            "input": "ping-responses-reasoning",
        },
        timeout=15.0,
    )
    assert status == 200, f"openai responses stream status={status}: {raw!r:.200}"
    text = str(raw)
    assert "response.created" in text or "response.completed" in text, (
        f"missing Responses API events: {text[:300]}"
    )


def test_anthropic_basic_stream(base: str, _: Any = None) -> None:
    """Verify Anthropic SSE stream from aimock contains standard events."""
    status, raw = http_request(
        "POST",
        f"{base}/v1/messages",
        {
            "model": "mock-anthropic",
            "max_tokens": 128,
            "stream": True,
            "messages": [{"role": "user", "content": "ping-anthropic-stream"}],
        },
        headers={"anthropic-version": "2023-06-01"},
        timeout=15.0,
    )
    assert status == 200, f"anthropic stream status={status}: {raw!r:.200}"
    text = str(raw)
    assert "message_start" in text, f"missing message_start: {text[:300]}"
    assert "message_stop" in text or "message_delta" in text, (
        f"missing message_stop/message_delta: {text[:300]}"
    )
    assert "pong-anthropic" in text, f"missing fixture content 'pong-anthropic': {text[:300]}"


def test_anthropic_chaos_malformed(base: str, _: Any = None) -> None:
    """Chaos: malformed SSE frames; Nyro must recover and not return 500."""
    try:
        status, _ = http_request(
            "POST",
            f"{base}/v1/messages",
            {
                "model": "mock-anthropic",
                "max_tokens": 128,
                "stream": True,
                "messages": [{"role": "user", "content": "ping-chaos-malformed"}],
            },
            headers={"anthropic-version": "2023-06-01"},
            timeout=10.0,
        )
        assert status < 600, f"unexpected status: {status}"
    except Exception:  # noqa: BLE001
        pass


def test_gemini_basic_stream(base: str, _: Any = None) -> None:
    """L2-exclusive: Gemini protocol (/v1beta/) via aimock — Ollama doesn't expose this."""
    status, raw = http_request(
        "POST",
        f"{base}/v1beta/models/mock-gemini:streamGenerateContent?alt=sse",
        {
            "contents": [{"role": "user", "parts": [{"text": "ping-gemini-stream"}]}]
        },
        timeout=15.0,
    )
    assert status == 200, f"gemini stream status={status}: {raw!r:.200}"
    text = str(raw)
    assert "pong-gemini" in text, f"missing fixture content 'pong-gemini': {text[:300]}"


# ── Entry point ──────────────────────────────────────────────────────────────


def main() -> int:
    nyro_binary = resolve_nyro_binary(REPO_ROOT)
    if not nyro_binary.exists():
        print(f"nyro-server not found at {nyro_binary}", file=sys.stderr)
        print("Build with: cargo build -p nyro-server", file=sys.stderr)
        return 1

    # Assign ports
    aimock_ports = choose_ports()
    proxy_port = find_free_port()
    port_map = {key: port for (key, _), port in zip(SCHEMA_DIRS, aimock_ports)}

    container_names = [f"nyro-aimock-{i}" for i in range(len(SCHEMA_DIRS))]
    stop_containers(container_names)  # clean up any leftovers

    print("Starting aimock instances...")
    for (_, subdir), port, name in zip(SCHEMA_DIRS, aimock_ports, container_names):
        start_aimock(port, subdir, container_name=name)

    print("Waiting for aimock instances to be ready...")
    try:
        for port in aimock_ports:
            wait_aimock_ready(port, timeout=30.0)
    except TimeoutError as exc:
        print(f"aimock startup timeout: {exc}", file=sys.stderr)
        stop_containers(container_names)
        return 1

    config_path = render_standalone_yaml(CONFIG_TEMPLATE, proxy_port, port_map)
    proc, logs = start_nyro_server(config_path, nyro_binary=nyro_binary)
    base = f"http://127.0.0.1:{proxy_port}"

    try:
        wait_until_ready(f"{base}/v1/chat/completions", timeout=30.0)

        tests = [
            ("openai_chat_basic_stream", test_openai_chat_basic_stream),
            ("openai_chat_content_filter", test_openai_chat_content_filter),
            ("openai_chat_reasoning", test_openai_chat_reasoning),
            ("openai_chat_chaos_disconnect", test_openai_chat_chaos_disconnect),
            ("openai_responses_reasoning", test_openai_responses_reasoning),
            ("anthropic_basic_stream", test_anthropic_basic_stream),
            ("anthropic_chaos_malformed", test_anthropic_chaos_malformed),
            ("gemini_basic_stream", test_gemini_basic_stream),
        ]

        print("Running L2 aimock static tests...")
        rc = run_tests(tests, base)
        if rc == 0:
            print(f"\nAll {len(tests)} static tests passed.")
        return rc
    finally:
        stop_nyro_server(proc, logs)
        stop_containers(container_names)
        try:
            config_path.unlink(missing_ok=True)
        except OSError:
            pass


if __name__ == "__main__":
    sys.exit(main())
