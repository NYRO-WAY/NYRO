from __future__ import annotations

import pytest

from tests.common.helpers import http_request

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


@pytest.mark.e2e
@pytest.mark.ollama
@pytest.mark.slow
def test_openai_chat_nonstream(ollama_base_url: str) -> None:
    status, resp = http_request(
        "POST",
        f"{ollama_base_url}/v1/chat/completions",
        {
            "model": MODEL,
            "messages": [{"role": "user", "content": "Say hi in one word"}],
            "max_tokens": 512,
        },
        timeout=60.0,
    )
    assert status == 200
    assert "choices" in resp
    choice = resp["choices"][0]
    assert choice["message"]["role"] == "assistant"
    assert choice["finish_reason"] in ("stop", "length")
    assert isinstance(choice["message"].get("content"), str)


@pytest.mark.e2e
@pytest.mark.ollama
@pytest.mark.slow
def test_openai_chat_stream(ollama_base_url: str) -> None:
    status, raw = http_request(
        "POST",
        f"{ollama_base_url}/v1/chat/completions",
        {
            "model": MODEL,
            "stream": True,
            "messages": [{"role": "user", "content": "Say hi in one word"}],
            "max_tokens": 512,
        },
        timeout=60.0,
    )
    assert status == 200
    text = str(raw)
    assert "[DONE]" in text
    assert "chat.completion.chunk" in text


@pytest.mark.e2e
@pytest.mark.ollama
@pytest.mark.slow
@pytest.mark.flaky(reruns=2, reruns_delay=3)
def test_openai_responses_nonstream(ollama_base_url: str) -> None:
    status, resp = http_request(
        "POST",
        f"{ollama_base_url}/v1/responses",
        {
            "model": MODEL,
            "input": "Think briefly then say hi",
            "max_output_tokens": 512,
        },
        timeout=120.0,
    )
    assert status == 200
    assert "output" in resp
    output = resp["output"]
    assert isinstance(output, list) and len(output) >= 1
    types = [item.get("type") for item in output]
    assert "message" in types
    msg = next(item for item in output if item.get("type") == "message")
    assert msg.get("role") == "assistant"
    assert isinstance(msg.get("content", []), list) and len(msg.get("content", [])) >= 1


@pytest.mark.e2e
@pytest.mark.ollama
@pytest.mark.slow
def test_openai_responses_stream(ollama_base_url: str) -> None:
    status, raw = http_request(
        "POST",
        f"{ollama_base_url}/v1/responses",
        {
            "model": MODEL,
            "input": "Think briefly then say hi",
            "stream": True,
            "max_output_tokens": 512,
        },
        timeout=60.0,
    )
    assert status == 200
    text = str(raw)
    assert "response.created" in text or "response.completed" in text


@pytest.mark.e2e
@pytest.mark.ollama
@pytest.mark.slow
def test_anthropic_messages_nonstream(ollama_base_url: str) -> None:
    status, resp = http_request(
        "POST",
        f"{ollama_base_url}/v1/messages",
        {
            "model": MODEL,
            "max_tokens": 512,
            "messages": [{"role": "user", "content": "Think briefly then say hi"}],
        },
        headers={"anthropic-version": "2023-06-01"},
        timeout=60.0,
    )
    assert status == 200
    assert resp.get("role") == "assistant"
    assert resp.get("stop_reason") in ("end_turn", "max_tokens")
    content = resp.get("content", [])
    assert isinstance(content, list) and len(content) >= 1
    types = [item.get("type") for item in content]
    assert "text" in types or "thinking" in types


@pytest.mark.e2e
@pytest.mark.ollama
@pytest.mark.slow
def test_anthropic_messages_stream(ollama_base_url: str) -> None:
    status, raw = http_request(
        "POST",
        f"{ollama_base_url}/v1/messages",
        {
            "model": MODEL,
            "max_tokens": 512,
            "stream": True,
            "messages": [{"role": "user", "content": "Think briefly then say hi"}],
        },
        headers={"anthropic-version": "2023-06-01"},
        timeout=60.0,
    )
    assert status == 200
    text = str(raw)
    assert "message_start" in text
    assert "message_stop" in text or "message_delta" in text


@pytest.mark.e2e
@pytest.mark.ollama
@pytest.mark.slow
def test_anthropic_messages_tool_use(ollama_base_url: str) -> None:
    status, resp = http_request(
        "POST",
        f"{ollama_base_url}/v1/messages",
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
    assert status == 200
    assert resp.get("role") == "assistant"
    content = resp.get("content", [])
    types = [item.get("type") for item in content]
    assert "tool_use" in types or "text" in types
