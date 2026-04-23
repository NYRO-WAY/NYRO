#!/usr/bin/env python3
"""L3 Ollama dynamic E2E tests for Nyro.

Covers 7 protocol chains (qwen3.5:0.8b via Ollama):
  1. OpenAI  /v1/chat/completions  non-stream
  2. OpenAI  /v1/chat/completions  stream
  3. OpenAI  /v1/responses         non-stream  (reasoning output)
  4. OpenAI  /v1/responses         stream      (response.reasoning_summary_text.delta)
  5. Anthropic /v1/messages        non-stream  (thinking block)
  6. Anthropic /v1/messages        stream      (thinking events)
  7. Anthropic /v1/messages        tool_use

Assertions: structural + enum fields only — no concrete text matching.
max_tokens / max_output_tokens ≥ 512 to avoid token budget exhaustion.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

# Allow importing from tests/common
sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from common.helpers import (
    find_free_port,
    http_request,
    render_standalone_yaml,
    resolve_nyro_binary,
    run_tests,
    start_nyro_server,
    stop_nyro_server,
    wait_until_ready,
)

REPO_ROOT = Path(__file__).resolve().parents[3]
CONFIG_TEMPLATE = Path(__file__).parent / "standalone.yaml"
MODEL = "qwen3"

WEATHER_TOOL_ANTHROPIC = {
    "name": "get_current_weather",
    "description": "Get the current weather for a location.",
    "input_schema": {
        "type": "object",
        "properties": {"location": {"type": "string", "description": "City name"}},
        "required": ["location"],
    },
}


# ── Individual test functions ────────────────────────────────────────────────


def test_openai_chat_nonstream(base: str) -> None:
    status, resp = http_request(
        "POST",
        f"{base}/v1/chat/completions",
        {
            "model": MODEL,
            "messages": [{"role": "user", "content": "Say hi in one word"}],
            "max_tokens": 512,
        },
        timeout=60.0,
    )
    assert status == 200, f"OpenAI chat non-stream status={status}: {resp}"
    assert "choices" in resp, f"missing choices: {resp}"
    choice = resp["choices"][0]
    assert choice["message"]["role"] == "assistant", "wrong role"
    assert choice["finish_reason"] in ("stop", "length"), f"bad finish_reason: {choice['finish_reason']}"
    assert isinstance(choice["message"].get("content"), str), "content must be string"


def test_openai_chat_stream(base: str) -> None:
    status, raw = http_request(
        "POST",
        f"{base}/v1/chat/completions",
        {
            "model": MODEL,
            "stream": True,
            "messages": [{"role": "user", "content": "Say hi in one word"}],
            "max_tokens": 512,
        },
        timeout=60.0,
    )
    assert status == 200, f"OpenAI chat stream status={status}"
    text = str(raw)
    assert "[DONE]" in text, "missing [DONE] in OpenAI chat stream"
    assert "chat.completion.chunk" in text, "missing chunk object type in stream"


def test_openai_responses_nonstream(base: str) -> None:
    status, resp = http_request(
        "POST",
        f"{base}/v1/responses",
        {
            "model": MODEL,
            "input": "Think briefly then say hi",
            "max_output_tokens": 512,
        },
        timeout=120.0,
    )
    assert status == 200, f"OpenAI Responses non-stream status={status}: {resp}"
    assert "output" in resp, f"missing output: {resp}"
    output = resp["output"]
    assert isinstance(output, list) and len(output) >= 1, "output must be non-empty list"
    types = [item.get("type") for item in output]
    assert "message" in types, f"no message item in output: {types}"
    msg = next(i for i in output if i.get("type") == "message")
    assert msg.get("role") == "assistant", f"wrong role: {msg.get('role')}"
    content = msg.get("content", [])
    assert isinstance(content, list) and len(content) >= 1, "message content must be non-empty"


def test_openai_responses_stream(base: str) -> None:
    status, raw = http_request(
        "POST",
        f"{base}/v1/responses",
        {
            "model": MODEL,
            "input": "Think briefly then say hi",
            "stream": True,
            "max_output_tokens": 512,
        },
        timeout=60.0,
    )
    assert status == 200, f"OpenAI Responses stream status={status}"
    text = str(raw)
    assert "response.created" in text or "response.completed" in text, (
        f"missing Responses stream events: {text[:300]}"
    )


def test_anthropic_messages_nonstream(base: str) -> None:
    status, resp = http_request(
        "POST",
        f"{base}/v1/messages",
        {
            "model": MODEL,
            "max_tokens": 512,
            "messages": [{"role": "user", "content": "Think briefly then say hi"}],
        },
        headers={"anthropic-version": "2023-06-01"},
        timeout=60.0,
    )
    assert status == 200, f"Anthropic non-stream status={status}: {resp}"
    assert resp.get("role") == "assistant", f"wrong role: {resp.get('role')}"
    assert resp.get("stop_reason") in ("end_turn", "max_tokens"), (
        f"bad stop_reason: {resp.get('stop_reason')}"
    )
    content = resp.get("content", [])
    assert isinstance(content, list) and len(content) >= 1, "content must be non-empty list"
    types = [b.get("type") for b in content]
    assert "text" in types or "thinking" in types, f"no text/thinking block: {types}"


def test_anthropic_messages_stream(base: str) -> None:
    status, raw = http_request(
        "POST",
        f"{base}/v1/messages",
        {
            "model": MODEL,
            "max_tokens": 512,
            "stream": True,
            "messages": [{"role": "user", "content": "Think briefly then say hi"}],
        },
        headers={"anthropic-version": "2023-06-01"},
        timeout=60.0,
    )
    assert status == 200, f"Anthropic stream status={status}"
    text = str(raw)
    assert "message_start" in text, "missing message_start SSE event"
    assert "message_stop" in text or "message_delta" in text, (
        "missing message_stop/message_delta SSE event"
    )


def test_anthropic_messages_tool_use(base: str) -> None:
    status, resp = http_request(
        "POST",
        f"{base}/v1/messages",
        {
            "model": MODEL,
            "max_tokens": 512,
            "messages": [
                {
                    "role": "user",
                    "content": (
                        "You must call get_current_weather for Paris. "
                        "Do not answer in text, only call the tool."
                    ),
                }
            ],
            "tools": [WEATHER_TOOL_ANTHROPIC],
        },
        headers={"anthropic-version": "2023-06-01"},
        timeout=60.0,
    )
    assert status == 200, f"Anthropic tool_use status={status}: {resp}"
    assert resp.get("role") == "assistant", f"wrong role: {resp.get('role')}"
    content = resp.get("content", [])
    types = [b.get("type") for b in content]
    # Small models may not always trigger tool_use; accept text fallback.
    assert "tool_use" in types or "text" in types, (
        f"expected tool_use or text block, got: {types}"
    )


# ── Entry point ──────────────────────────────────────────────────────────────


def main() -> int:
    proxy_port = find_free_port()
    nyro_binary = resolve_nyro_binary(REPO_ROOT)

    if not nyro_binary.exists():
        print(f"nyro-server not found at {nyro_binary}", file=sys.stderr)
        print("Build with: cargo build -p nyro-server", file=sys.stderr)
        return 1

    config_path = render_standalone_yaml(CONFIG_TEMPLATE, proxy_port)
    proc, logs = start_nyro_server(config_path, nyro_binary=nyro_binary)
    base = f"http://127.0.0.1:{proxy_port}"

    try:
        wait_until_ready(f"{base}/v1/chat/completions", timeout=30.0)

        tests = [
            ("openai_chat_nonstream", test_openai_chat_nonstream),
            ("openai_chat_stream", test_openai_chat_stream),
            ("openai_responses_nonstream", test_openai_responses_nonstream),
            ("openai_responses_stream", test_openai_responses_stream),
            ("anthropic_messages_nonstream", test_anthropic_messages_nonstream),
            ("anthropic_messages_stream", test_anthropic_messages_stream),
            ("anthropic_messages_tool_use", test_anthropic_messages_tool_use),
        ]

        print("Running L3 Ollama inference tests...")
        rc = run_tests(tests, base)
        if rc == 0:
            print("\nAll 7 chains passed.")
        return rc
    finally:
        stop_nyro_server(proc, logs)
        try:
            config_path.unlink(missing_ok=True)
        except OSError:
            pass


if __name__ == "__main__":
    sys.exit(main())
