# Testing Guide

## Overview — Three-Layer Pyramid

```
┌─────────────────────────────────────┐
│   L1  Unit Tests  (cargo test)      │  fast, pure-Rust, no I/O
├─────────────────────────────────────┤
│   L2  Static E2E  (aimock)          │  deterministic, byte-level
├─────────────────────────────────────┤
│   L3  Dynamic E2E (Ollama)          │  structural, model-driven
└─────────────────────────────────────┘
        +  E2E Admin    (full-mode Nyro + SQLite)
        +  E2E Storage  (SQLite / pgvector — daily)
```

---

## Layer Descriptions

### L1 — Unit Tests

- **Location**: `crates/nyro-core/src/protocol/**`
- **What they cover**: Protocol transformer edge cases — tool-call streaming
  fragments, DeepSeek `reasoning_content` separation, Anthropic `thinking`
  deltas, Gemini `thought_summary`, OpenAI Responses `output_item`, `<think>`
  tag extraction, tool-call correlation (by-ID and FIFO fallback).
- **Run locally**:
  ```bash
  cargo test --workspace --exclude nyro-desktop
  ```
- **CI trigger**: every push / PR via `unit-tests` job in `ci.yml`.

---

### L2 — Static E2E (aimock)

- **Location**: `tests/e2e/aimock/`
- **What they cover**: Byte-level regression for the full Nyro proxy pipeline
  using pre-recorded fixtures.  Four isolated `aimock` Docker containers run
  on fixed ports (4010–4013), each loaded with fixtures for exactly one API
  schema:

  | Port | Schema | Fixtures dir |
  |------|--------|--------------|
  | 4010 | OpenAI Chat Completions | `fixtures/openai-completions/` |
  | 4011 | OpenAI Responses | `fixtures/openai-responses/` |
  | 4012 | Anthropic Messages | `fixtures/anthropic-messages/` |
  | 4013 | Google GenerateContent | `fixtures/google-generatecontent/` |

- **Test scenarios (8 total)**:
  - OpenAI Chat: basic stream, Azure `content_filter`, DeepSeek `reasoning`, chaos mid-stream disconnect
  - OpenAI Responses: reasoning stream
  - Anthropic: basic stream with thinking, chaos malformed SSE
  - Gemini: basic stream

- **Run locally** (requires Docker):
  ```bash
  python3 -m pip install -r requirements-dev.txt
  docker pull ghcr.io/copilotkit/aimock:latest
  python3 -m pytest tests/e2e/aimock -q -m aimock
  # override binary path:
  NYRO_BINARY=./target/debug/nyro-server python3 -m pytest tests/e2e/aimock -q -m aimock
  ```
- **CI trigger**: every push / PR via `e2e-static-tests` job in `ci.yml`.

---

### L3 — Dynamic E2E (Ollama)

- **Location**: `tests/e2e/ollama/`
- **What they cover**: Structural validation of OpenAI and Anthropic protocol
  transformations using a live local LLM (`qwen3.5:0.8b` via Ollama).
  7 distinct interaction links:
  1. OpenAI Chat — non-streaming
  2. OpenAI Chat — streaming
  3. OpenAI Responses — non-streaming
  4. OpenAI Responses — streaming
  5. Anthropic Messages — non-streaming
  6. Anthropic Messages — streaming
  7. Anthropic Messages — tool use

- **Run locally** (requires Ollama with `qwen3.5:0.8b`):
  ```bash
  python3 -m pip install -r requirements-dev.txt
  ollama pull qwen3.5:0.8b
  python3 -m pytest tests/e2e/ollama -q -m ollama
  # override Ollama host:
  OLLAMA_HOST=http://192.168.1.10:11434 python3 -m pytest tests/e2e/ollama -q -m ollama
  ```
- **CI trigger**: every push / PR via `e2e-inference-tests` job in `ci.yml`
  (timeout: 20 min, Ollama started as a Docker container).

---

### E2E Admin Tests

- **Location**: `tests/e2e/admin/`
- **What they cover**: Nyro in **full mode** (admin API + SQLite database).
  - Admin authentication
  - CRUD for Providers, Routes, API Keys
  - Access control on routes
  - `export_config` round-trip
  - Log persistence and stats increments after a proxy request

- **Run locally**:
  ```bash
  python3 -m pip install -r requirements-dev.txt
  python3 -m pytest tests/e2e/admin -q -m admin
  NYRO_BINARY=./target/debug/nyro-server python3 -m pytest tests/e2e/admin -q -m admin
  ```
- **CI trigger**: every push / PR via `e2e-admin-tests` job in `ci.yml`.

---

### E2E Storage Tests

- **Location**: `tests/e2e/storage/`
- **What they cover**: Cross-backend behavioural equivalence.
  - SQLite: always enabled
  - Postgres / pgvector (`pgvector/pgvector:pg16`): opt-in via `--backend postgres`

  Per backend: admin CRUD, proxy auth (`401` without key, `200` with key),
  log + stats persistence.

- **Run locally**:
  ```bash
  python3 -m pip install -r requirements-dev.txt
  # SQLite only
  python3 -m pytest tests/e2e/storage/test_storage.py -q -m storage -k sqlite

  # Postgres (requires a running pgvector instance)
  DB_URL=postgresql://nyro:nyro@localhost:5432/nyro_test \
    python3 -m pytest tests/e2e/storage/test_storage.py -q -m storage -k postgres
  ```
- **CI trigger**: daily at 03:00 UTC via `.github/workflows/storage-backends.yml`.

---

## CI Jobs at a Glance

```
ci.yml  (push master / release/** / pull_request / workflow_dispatch)
│
├── build            — cargo check + cargo build -p nyro-server → artifact
│
├── unit-tests       ← needs: build  — cargo test --workspace
├── e2e-static-tests ← needs: build  — aimock Docker × 4 + pytest
├── e2e-inference-tests ← needs: build  — Ollama Docker + pytest
└── e2e-admin-tests  ← needs: build  — pytest (full-mode Nyro)

storage-backends.yml  (daily 03:00 UTC / workflow_dispatch)
└── storage-tests  — pgvector:pg16 service + pytest (sqlite + postgres)
```

All E2E jobs download the same `nyro-server-linux-x86_64` artifact produced by
the `build` job to avoid redundant compilation.

---

## Shared Helpers

`tests/common/helpers.py` provides shared utilities used by the pytest fixtures:

| Helper | Purpose |
|--------|---------|
| `find_free_port()` | Allocate an ephemeral port |
| `http_request()` | Thin `urllib` wrapper (no third-party deps) |
| `wait_until_ready()` | Poll a health URL until ready or timeout |
| `render_standalone_yaml()` | Substitute `proxy_port` and `{KEY}` placeholders in YAML templates |
| `resolve_nyro_binary()` | Honour `$NYRO_BINARY` or fall back to `target/debug/nyro-server` |
| `start_nyro_server()` | Launch `nyro-server --config <path>`, capture logs |
| `stop_nyro_server()` | Terminate server, print log tail on error |
| `minimal_mock_provider()` | Spin up a minimal OpenAI-compatible mock server |

---

## Naming Conventions

| Pattern | Meaning |
|---------|---------|
| `tests/e2e/<suite>/test_*.py` | Pytest-discovered test module |
| `tests/e2e/<suite>/conftest.py` | Suite-local fixtures and lifecycle management |
| `tests/e2e/<suite>/standalone.yaml` | Nyro standalone-mode config template |
| `tests/e2e/aimock/fixtures/<schema>/` | One fixture dir per aimock instance |
| `tests/common/helpers.py` | Shared Python utilities |
| `NYRO_BINARY` env var | Override the server binary path in all E2E suites |
