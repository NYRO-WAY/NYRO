# Testing Guide

Nyro's automated tests are organised by **what they validate**, not by an
abstract "pyramid level". Every suite is independently runnable from a
single `pytest` invocation (or `cargo test` for unit tests) and is wired
into CI under [.github/workflows/ci.yml](../../.github/workflows/ci.yml).

| Suite | Layer | Validates | CI Job |
|-------|-------|-----------|--------|
| Unit tests | Rust | Pure protocol/transformer logic, `nyro-tools` internals | `unit-tests` |
| Proxy E2E (protocol matrix) | Python pytest | `client_protocol × recorded_fixture` round-trip through `nyro-server` | `e2e-proxy-tests` |
| Admin E2E | Python pytest | `nyro-server` full-mode admin API + SQLite | `e2e-admin-tests` |
| Storage E2E | Python pytest | SQLite ↔ Postgres / pgvector behavioural equivalence | `storage-tests` (daily) |

> The Proxy E2E suite replays **real LLM bytes** that you record once via
> `nyro-tools record` and serve forever via `nyro-tools replay`. There is
> no external mock framework — everything ships from `crates/nyro-tools`.

---

## Unit Tests

- **Location**: `crates/nyro-core/src/protocol/**`, `crates/nyro-tools/src/**`
- **Coverage**: protocol transformer edge cases (tool-call streaming
  fragments, DeepSeek `reasoning_content`, Anthropic `thinking`, Gemini
  `thought_summary`, OpenAI Responses `output_item`, `<think>` tag
  extraction, tool-call correlation) plus `nyro-tools` internals
  (fixture round-trip, scenario body templates, replay HashMap, header
  scrubbing).
- **Run locally**:
  ```bash
  cargo test --workspace --exclude nyro-desktop --exclude nyro-server
  ```
- **CI**: `unit-tests` job on every push / PR.

---

## Proxy E2E (`nyro-tools replay` protocol matrix)

- **Location**: `tests/e2e/proxy/`
- **Fixtures**: `tests/e2e/fixtures/<protocol>/<vendor>/<scenario>.jsonl`
- **Coverage**: Two-dimensional matrix `client-protocol × recorded-fixture`.
  For every recorded `replay_model = <vendor>--<protocol>--<scenario>` the
  test sends one request through *every* nyro ingress path, asserting that
  the recorded LLM bytes survive nyro's protocol conversion, that the
  scenario anchor token (e.g. `NYRO_PROBE_BASIC_STREAM`) is preserved, and
  that protocol-specific fields (e.g. `choices` / `delta` / `tool_calls`)
  appear in the converted response.

### How it boots
1. pytest scans `tests/e2e/fixtures/` for `*.jsonl` files.
2. Four `nyro-tools replay` subprocesses start on ports 25208–25211, one
   per upstream protocol (`openai-chat`, `openai-responses`,
   `anthropic-messages`, `google-content`).
3. pytest synthesises `standalone.yaml` with 4 fixed providers and one
   route per `replay_model` (route `name`, `vmodel`, and `target.model`
   are all the `replay_model` string).
4. `nyro-server` boots in standalone mode against that config.
5. Tests cartesian-product `(4 ingress protocols) × (N recorded fixtures)`.

If `tests/e2e/fixtures/` is empty the suite skips gracefully — CI stays
green until the first recordings land.

### Run locally
```bash
# 1. build the binaries
cargo build -p nyro-server -p nyro-tools

# 2. record at least one vendor (see ./recording.md)
export DEEPSEEK_API_KEY=sk-...
target/debug/nyro-tools record \
  --vendor deepseek \
  -p openai-chat \
  -e https://api.deepseek.com/v1 \
  -o tests/e2e/fixtures \
  --model deepseek-chat \
  --reasoning-model deepseek-reasoner \
  --api-key-env DEEPSEEK_API_KEY

# 3. run the matrix
python3 -m pytest tests/e2e/proxy -q -m proxy
```

- **CI**: `e2e-proxy-tests` job on every push / PR. Reuses the
  `nyro-tools-linux-x86_64` artifact from the `build` job.

---

## Admin E2E

- **Location**: `tests/e2e/admin/`
- **Coverage**: Nyro in **full mode** (admin API + SQLite database):
  authentication, Provider/Route/API-key CRUD, route access control,
  `export_config` round-trip, log persistence, stats increments.
- **Run locally**:
  ```bash
  python3 -m pip install -r requirements-dev.txt
  NYRO_BINARY=./target/debug/nyro-server python3 -m pytest tests/e2e/admin -q -m admin
  ```
- **CI**: `e2e-admin-tests` job on every push / PR.

---

## Storage E2E

- **Location**: `tests/e2e/storage/`
- **Coverage**: Cross-backend behavioural equivalence (admin CRUD, proxy
  auth, log + stats persistence) on SQLite and Postgres / pgvector.
- **Run locally**:
  ```bash
  python3 -m pytest tests/e2e/storage/test_storage.py -q -m storage -k sqlite

  DB_URL=postgresql://nyro:nyro@localhost:5432/nyro_test \
    python3 -m pytest tests/e2e/storage/test_storage.py -q -m storage -k postgres
  ```
- **CI**: daily at 03:00 UTC via `.github/workflows/storage-backends.yml`.

---

## CI Jobs at a Glance

```
ci.yml  (push master / release/** / pull_request / workflow_dispatch)
│
├── build              — cargo check + nyro-server + nyro-tools → artifacts
│
├── unit-tests         ← needs: build  — cargo test --workspace
├── e2e-proxy-tests    ← needs: build  — nyro-tools replay × 4 + pytest matrix
└── e2e-admin-tests    ← needs: build  — pytest (full-mode nyro-server)

storage-backends.yml  (daily 03:00 UTC / workflow_dispatch)
└── storage-tests       — pgvector:pg16 service + pytest (sqlite + postgres)
```

All E2E jobs download the `nyro-server` and (where needed) `nyro-tools`
artifacts produced by the `build` job to avoid redundant compilation.

---

## Shared Helpers

`tests/common/helpers.py`:

| Helper | Purpose |
|--------|---------|
| `find_free_port()` | Allocate an ephemeral port |
| `is_port_free()` | Probe a fixed port |
| `http_request()` | Thin `urllib` wrapper (no third-party deps) |
| `wait_until_ready()` | Poll a health URL until ready or timeout |
| `render_standalone_yaml()` | Substitute `proxy_port` and `{KEY}` placeholders in YAML templates |
| `resolve_nyro_binary()` | Honour `$NYRO_BINARY` or fall back to `target/debug/nyro-server` |
| `start_nyro_server()` / `stop_nyro_server()` | Manage `nyro-server` subprocess lifecycle |
| `minimal_mock_provider()` | Spin up a minimal OpenAI-compatible mock |

`tests/e2e/proxy/conftest.py` adds:

| Fixture | Scope | Purpose |
|---------|-------|---------|
| `nyro_tools_binary` | session | Locate `target/debug/nyro-tools` (or `$NYRO_TOOLS_BINARY`) |
| `scenario_metadata` | session | Parse `nyro-tools print-scenarios` JSON |
| `replay_models` | session | Scan `tests/e2e/fixtures/` for `replay_model` strings |
| `replay_cluster` | module | Boot 4 `nyro-tools replay` subprocesses |
| `nyro_proxy_base` | module | Synthesise `standalone.yaml` and boot `nyro-server` |

---

## Naming Conventions

| Pattern | Meaning |
|---------|---------|
| `tests/e2e/<suite>/test_*.py` | Pytest-discovered test module |
| `tests/e2e/<suite>/conftest.py` | Suite-local fixtures and lifecycle |
| `tests/e2e/fixtures/<protocol>/<vendor>/<scenario>.jsonl` | One recorded interaction per file |
| `replay_model` field | Fully-qualified key `<vendor>--<protocol>--<scenario>` used by replay's HashMap |
| `NYRO_BINARY` env var | Override `nyro-server` path |
| `NYRO_TOOLS_BINARY` env var | Override `nyro-tools` path |

---

## See Also

- [recording.md](./recording.md) — step-by-step recording SOP
- [coverage-matrix.md](./coverage-matrix.md) — vendor × protocol × scenario progress table
