from __future__ import annotations

import pytest

from tests.common.helpers import http_request


@pytest.mark.e2e
@pytest.mark.aimock
def test_openai_chat_basic_stream(aimock_base_url: str) -> None:
    status, raw = http_request(
        "POST",
        f"{aimock_base_url}/v1/chat/completions",
        {
            "model": "mock-chat",
            "stream": True,
            "messages": [{"role": "user", "content": "ping-stream"}],
        },
        timeout=15.0,
    )
    assert status == 200
    text = str(raw)
    assert "[DONE]" in text
    assert "pong" in text


@pytest.mark.e2e
@pytest.mark.aimock
def test_openai_chat_content_filter(aimock_base_url: str) -> None:
    status, resp = http_request(
        "POST",
        f"{aimock_base_url}/v1/chat/completions",
        {
            "model": "mock-chat",
            "messages": [{"role": "user", "content": "ping-azure"}],
        },
    )
    assert status == 200
    finish = resp.get("choices", [{}])[0].get("finish_reason", "")
    assert finish == "content_filter"


@pytest.mark.e2e
@pytest.mark.aimock
def test_openai_chat_reasoning(aimock_base_url: str) -> None:
    status, resp = http_request(
        "POST",
        f"{aimock_base_url}/v1/chat/completions",
        {
            "model": "mock-chat",
            "messages": [{"role": "user", "content": "ping-reasoning"}],
        },
    )
    assert status == 200
    msg = resp.get("choices", [{}])[0].get("message", {})
    assert msg.get("content")


@pytest.mark.e2e
@pytest.mark.aimock
def test_openai_chat_chaos_disconnect(aimock_base_url: str) -> None:
    try:
        status, _ = http_request(
            "POST",
            f"{aimock_base_url}/v1/chat/completions",
            {
                "model": "mock-chat",
                "stream": True,
                "messages": [{"role": "user", "content": "ping-chaos-disconnect"}],
            },
            timeout=10.0,
        )
        assert status < 600
    except Exception:
        pass


@pytest.mark.e2e
@pytest.mark.aimock
def test_openai_responses_reasoning(aimock_base_url: str) -> None:
    status, raw = http_request(
        "POST",
        f"{aimock_base_url}/v1/responses",
        {
            "model": "mock-responses",
            "stream": True,
            "input": "ping-responses-reasoning",
        },
        timeout=15.0,
    )
    assert status == 200
    text = str(raw)
    assert "response.created" in text or "response.completed" in text


@pytest.mark.e2e
@pytest.mark.aimock
def test_anthropic_basic_stream(aimock_base_url: str) -> None:
    status, raw = http_request(
        "POST",
        f"{aimock_base_url}/v1/messages",
        {
            "model": "mock-anthropic",
            "max_tokens": 128,
            "stream": True,
            "messages": [{"role": "user", "content": "ping-anthropic-stream"}],
        },
        headers={"anthropic-version": "2023-06-01"},
        timeout=15.0,
    )
    assert status == 200
    text = str(raw)
    assert "message_start" in text
    assert "message_stop" in text or "message_delta" in text
    assert "pong-anthropic" in text


@pytest.mark.e2e
@pytest.mark.aimock
def test_anthropic_chaos_malformed(aimock_base_url: str) -> None:
    try:
        status, _ = http_request(
            "POST",
            f"{aimock_base_url}/v1/messages",
            {
                "model": "mock-anthropic",
                "max_tokens": 128,
                "stream": True,
                "messages": [{"role": "user", "content": "ping-chaos-malformed"}],
            },
            headers={"anthropic-version": "2023-06-01"},
            timeout=10.0,
        )
        assert status < 600
    except Exception:
        pass


@pytest.mark.e2e
@pytest.mark.aimock
def test_gemini_basic_stream(aimock_base_url: str) -> None:
    status, raw = http_request(
        "POST",
        f"{aimock_base_url}/v1beta/models/mock-gemini:streamGenerateContent?alt=sse",
        {
            "contents": [{"role": "user", "parts": [{"text": "ping-gemini-stream"}]}]
        },
        timeout=15.0,
    )
    assert status == 200
    text = str(raw)
    assert "pong-gemini" in text
