"""Protocol-conversion matrix: client_protocol × recorded_fixture.

For every recorded ``replay_model`` (any vendor × any upstream protocol ×
any scenario) we send a request via *every* nyro ingress path and assert:

    * status == 200
    * the scenario's anchor token survives the round-trip
    * every protocol-specific expected field name appears in the response

This validates Nyro's bidirectional protocol conversion against real
recorded LLM bytes.
"""

from __future__ import annotations

import json
import re

import pytest

from tests.common.helpers import http_request
from tests.e2e.proxy.conftest import (
    NYRO_BASE_URL_PATH,
    PROTOCOLS,
    _scan_replay_models,
)

INGRESS_PROTOCOLS = list(PROTOCOLS)

# Real LLMs tokenise the anchor (e.g. ``NYRO_PROBE_BASIC_STREAM``) into
# ``NY`` / ``RO`` / ``_PRO`` / ... fragments emitted across many SSE frames.
# The naive ``anchor in raw_text`` check fails because of the JSON/SSE
# framing in between. Extract every value of the text-carrying JSON keys
# (``content`` / ``text`` / ``reasoning_content`` / ``thinking`` / ``delta``)
# and concatenate them so a fragmented anchor reassembles into the original
# token, while ignoring ``modelVersion`` and other framing noise.
_TEXT_FIELD_RE = re.compile(
    r'"(?:content|text|reasoning_content|thinking|delta)"\s*:\s*"((?:[^"\\]|\\.)*)"'
)


def _extract_visible_text(text: str) -> str:
    parts: list[str] = []
    for match in _TEXT_FIELD_RE.finditer(text):
        try:
            parts.append(json.loads('"' + match.group(1) + '"'))
        except json.JSONDecodeError:
            parts.append(match.group(1))
    return "".join(parts)


# ---------------------------------------------------------------------------
# collection-phase helpers (must run before pytest fixtures kick in)
# ---------------------------------------------------------------------------


def _collect_replay_models() -> list[str]:
    out: list[str] = []
    for models in _scan_replay_models().values():
        out.extend(models)
    return sorted(out)


def _parse_replay_model(rm: str) -> tuple[str, str, str]:
    parts = rm.split("--")
    if len(parts) != 3:
        pytest.fail(f"invalid replay_model `{rm}` (expected vendor--protocol--scenario)")
    return parts[0], parts[1], parts[2]


ALL_REPLAY_MODELS = _collect_replay_models()


# ---------------------------------------------------------------------------
# request body / URL builders per ingress protocol
# ---------------------------------------------------------------------------


def _build_request_body(ingress: str, vmodel: str, stream: bool) -> dict:
    prompt = f"nyro-replay probe for {vmodel}"
    if ingress == "openai-chat":
        return {
            "model": vmodel,
            "stream": stream,
            "messages": [{"role": "user", "content": prompt}],
        }
    if ingress == "openai-responses":
        return {"model": vmodel, "stream": stream, "input": prompt}
    if ingress == "anthropic-messages":
        return {
            "model": vmodel,
            "stream": stream,
            "max_tokens": 256,
            "messages": [{"role": "user", "content": prompt}],
        }
    if ingress == "google-content":
        return {
            "contents": [{"role": "user", "parts": [{"text": prompt}]}],
        }
    pytest.fail(f"unknown ingress protocol: {ingress}")


def _build_request_url(base: str, ingress: str, vmodel: str, stream: bool) -> str:
    if ingress == "openai-chat":
        return f"{base}/v1/chat/completions"
    if ingress == "openai-responses":
        return f"{base}/v1/responses"
    if ingress == "anthropic-messages":
        return f"{base}/v1/messages"
    if ingress == "google-content":
        action = "streamGenerateContent" if stream else "generateContent"
        query = "?alt=sse" if stream else ""
        return f"{base}/v1beta/models/{vmodel}:{action}{query}"
    pytest.fail(f"unknown ingress protocol: {ingress}")


def _request_headers(ingress: str) -> dict[str, str]:
    if ingress == "anthropic-messages":
        return {"anthropic-version": "2023-06-01"}
    return {}


# ---------------------------------------------------------------------------
# tests
# ---------------------------------------------------------------------------


@pytest.mark.e2e
@pytest.mark.proxy
@pytest.mark.parametrize("ingress_protocol", INGRESS_PROTOCOLS)
@pytest.mark.parametrize("replay_model", ALL_REPLAY_MODELS)
def test_protocol_matrix(
    nyro_proxy_base: str,
    scenario_metadata: dict[str, dict],
    ingress_protocol: str,
    replay_model: str,
) -> None:
    _, _, scenario_name = _parse_replay_model(replay_model)
    meta = scenario_metadata.get(scenario_name)
    if meta is None:
        pytest.fail(
            f"replay_model `{replay_model}` references unknown scenario "
            f"`{scenario_name}` (run `nyro-tools print-scenarios` to inspect)"
        )

    stream = bool(meta["stream"])
    anchor: str = meta["anchor"]
    expected = meta["expected_fields"].get(ingress_protocol, [])

    url = _build_request_url(nyro_proxy_base, ingress_protocol, replay_model, stream)
    body = _build_request_body(ingress_protocol, replay_model, stream)

    status, raw = http_request(
        "POST",
        url,
        body,
        headers=_request_headers(ingress_protocol),
        timeout=20.0,
    )
    text = raw if isinstance(raw, str) else json.dumps(raw)

    assert status == 200, (
        f"{ingress_protocol} <- {replay_model}: HTTP {status}, body={text[:512]}"
    )

    # Tool-use scenarios: the model is supposed to emit a tool_call, NOT echo
    # the anchor token. We trust ``expected_fields`` (e.g. ``functionCall``,
    # ``tool_use``) to assert the structured tool-call survived conversion.
    if scenario_name != "tool-use-stream":
        visible = _extract_visible_text(text)
        assert anchor in visible, (
            f"{ingress_protocol} <- {replay_model}: anchor `{anchor}` missing "
            f"from converted response (first 512 bytes: {text[:512]})"
        )

    for field in expected:
        assert field in text, (
            f"{ingress_protocol} <- {replay_model}: expected field "
            f"`{field}` missing from converted response (first 512 bytes: {text[:512]})"
        )


# ---------------------------------------------------------------------------
# sanity: ensure the in-memory protocol map mirrors nyro-server's YAML schema
# ---------------------------------------------------------------------------


def test_protocol_map_complete() -> None:
    assert set(NYRO_BASE_URL_PATH.keys()) == set(INGRESS_PROTOCOLS)
