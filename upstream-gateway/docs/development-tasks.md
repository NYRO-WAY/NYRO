# upstream-gateway Development Tasks

## 1. Purpose

This document turns the architecture into an implementation sequence.

Primary design references:

- [architecture.md](./architecture.md)
- [reuse-inventory.md](./reuse-inventory.md)

Working branch:

- `feature/upstream-gateway`

Reference archive branch:

- `archive/upstream-rate-limit-old`

## 2. Execution rules

1. Build the standalone service outside the root Rust workspace first.
2. Extract by capability boundary, not by original file layout.
3. Do not reintroduce `nyro-core` protocol-conversion dependencies.
4. Prefer compiling, testable milestones after each phase.

## 3. Status legend

- `[todo]` not started
- `[doing]` in progress
- `[done]` completed
- `[blocked]` waiting on prerequisite

## 4. Ordered task list

### Phase 0. Planning and structure

- `[done]` T0.1 Create `upstream-gateway/` design subtree
  - Design link:
    - architecture sections 1, 6, 19, 20
  - Deliverables:
    - `README.md`
    - `docs/architecture.md`
    - `docs/reuse-inventory.md`
    - this task file

- `[done]` T0.2 Create standalone crate skeleton
  - Design link:
    - architecture sections 5, 6, 20
  - Deliverables:
    - `Cargo.toml`
    - `src/main.rs`
    - `src/lib.rs`
    - module directories
    - independent `cargo check`

### Phase 1. Minimal runtime shell

- `[done]` T1.1 Implement app state and config loader
  - Design link:
    - architecture sections 5, 6, 11
  - Deliverables:
    - `src/app/`
    - `src/config/`
    - startup config model

- `[done]` T1.2 Implement base router and health endpoints
  - Design link:
    - architecture sections 5, 6, 14
  - Deliverables:
    - `GET /healthz`
    - `GET /admin/healthz`
    - router wiring

- `[done]` T1.3 Add data-plane route shells for OpenAI / Anthropic / Gemini
  - Design link:
    - architecture sections 7, 8, 14
  - Deliverables:
    - provider-prefixed route registration
    - request collection shells
    - placeholder responses until dispatcher is implemented

### Phase 2. Extract reusable limiter core

- `[done]` T2.1 Port `errors`, `config`, `types`
  - Design link:
    - architecture sections 10, 12, 17
    - reuse-inventory sections 2, 4, 5
  - Source candidates:
    - archived `rate_limit/errors.rs`
    - archived `rate_limit/config.rs`
    - archived `rate_limit/types.rs`
  - Implemented in:
    - `src/errors.rs`
    - `src/config/rate_limit.rs`
    - `src/runtime/types.rs`

- `[done]` T2.2 Port `selector` with local names and dependencies
  - Design link:
    - architecture sections 9, 12, 17
    - reuse-inventory sections 2, 4, 5
  - Deliverables:
    - provider+model cursor
    - key-state sharding
    - lazy idle cleanup
    - no `nyro-core::db::models::Provider` dependency
  - Implemented in:
    - `src/selector/mod.rs`

- `[done]` T2.3 Port `runtime` trait and in-memory implementation
  - Design link:
    - architecture sections 12, 15, 17
    - reuse-inventory sections 2, 4
  - Deliverables:
    - acquire / settle / rollback
    - runtime snapshot contract
  - Implemented in:
    - `src/runtime/mod.rs`
    - `src/app/mod.rs`

- `[done]` T2.4 Port regression tests for limiter core
  - Design link:
    - architecture sections 12, 17, 19
    - reuse-inventory sections 2, 4
  - Source candidate:
    - archived `tests/rate_limit_runtime.rs`
  - Implemented in:
    - `tests/rate_limit_runtime.rs`
    - selector unit tests in `src/selector/mod.rs`

### Phase 3. Data-plane request admission

- `[done]` T3.1 Define local provider, key, and model-rule storage models
  - Design link:
    - architecture sections 10, 11
  - Deliverables:
    - persistence structs
    - runtime DTOs
  - Implemented in:
    - `src/provider/mod.rs`
    - `src/storage/mod.rs`

- `[done]` T3.2 Implement request metadata extraction
  - Design link:
    - architecture sections 7, 14, 16
  - Deliverables:
    - parse body as `serde_json::Value`
    - extract `model`
    - detect `stream`
    - extract output reservation field
  - Implemented in:
    - `src/data_plane/request.rs`
    - `src/data_plane/mod.rs`

- `[done]` T3.3 Implement provider lookup and key lease acquisition
  - Design link:
    - architecture sections 8, 12, 16
  - Deliverables:
    - route provider id lookup
    - fetch provider config
    - call limiter acquire before upstream dispatch
  - Implemented in:
    - `src/app/mod.rs`
    - `src/data_plane/mod.rs`
    - `src/storage/mod.rs`
  - Current note:
    - while phase 4 passthrough is not wired yet, successful placeholder admission immediately rolls back the temporary lease to avoid dangling runtime state

### Phase 4. Upstream passthrough

- `[done]` T4.1 Implement upstream request builder per vendor family
  - Design link:
    - architecture sections 8, 14
  - Deliverables:
    - OpenAI auth/header policy
    - Anthropic auth/header policy
    - Gemini query/header policy
  - Implemented in:
    - `src/upstream/mod.rs`
    - `src/data_plane/mod.rs`

- `[done]` T4.2 Implement non-stream passthrough
  - Design link:
    - architecture sections 14, 16
  - Deliverables:
    - `reqwest` outbound call
    - body passthrough
    - lease settle / rollback
  - Implemented in:
    - `src/data_plane/mod.rs`
  - Current note:
    - non-stream responses now settle against provider-native `usage` fields when vendors return them
    - conservative fallback remains in place when usage is absent or unparsable

- `[done]` T4.3 Implement stream passthrough
  - Design link:
    - architecture sections 15, 16
  - Deliverables:
    - `bytes_stream()` relay
    - `Body::from_stream(...)`
    - preserve upstream SSE framing
    - do not aggregate stream
  - Implemented in:
    - `src/data_plane/mod.rs`
  - Current note:
    - once upstream HTTP response has started, phase 1 uses conservative `settle` on stream completion or mid-stream breakage
    - provider-native stream usage parsing is still pending and currently falls back to lease-reserved usage for settlement

### Phase 5. Token estimation

- `[done]` T5.1 Split and port Gemini estimator
  - Design link:
    - architecture sections 12, 13
    - reuse-inventory sections 2, 4
  - Implemented in:
    - `src/estimator/google.rs`
  - Current note:
    - admission is no longer using placeholder `0`
    - authoritative `gemini-tokenizer` counting is now wired for structured Gemini request bodies
    - unknown Gemini model names can fall back through `tokenizer_model` or built-in supported-model candidates
    - `inlineData` is excluded from tokenizer-native structured parts and is added back conservatively through modality-aware compensation

- `[done]` T5.2 Split and port OpenAI estimator
  - Design link:
    - architecture sections 12, 13
    - reuse-inventory sections 2, 4
  - Implemented in:
    - `src/estimator/openai.rs`
  - Current note:
    - phase 1 uses model-aware `tiktoken` fallback counting over final upstream JSON body

- `[done]` T5.3 Split and port Anthropic estimator
  - Design link:
    - architecture sections 12, 13
    - reuse-inventory sections 2, 4
  - Implemented in:
    - `src/estimator/anthropic.rs`
  - Current note:
    - phase 1 uses `claude-tokenizer` over final upstream JSON body

- `[done]` T5.4 Wire request_input_tokens + output reservation into admission
  - Design link:
    - architecture sections 12, 13, 16
  - Implemented in:
    - `src/estimator/mod.rs`
    - `src/data_plane/mod.rs`
  - Validation:
    - `google_tpm_rule_uses_estimated_input_tokens_for_admission`

### Phase 6. Control plane

- `[done]` T6.1 Implement SQLite-backed config storage
  - Design link:
    - architecture sections 5, 11
  - Implemented in:
    - `src/storage/mod.rs`
    - `src/storage/sqlite.rs`
    - `src/storage/bootstrap.rs`
    - `src/config/app.rs`
    - `src/app/mod.rs`
  - Current note:
    - persisted tables now include `gateway_providers`, `gateway_keys`, `gateway_model_rules`, and `gateway_settings`
    - startup can seed SQLite from `UPSTREAM_GATEWAY_BOOTSTRAP_JSON_PATH` when the database is empty
    - current control-plane still reads through the shared `GatewayConfigStore` trait, so existing data-plane code paths did not need route-level changes

- `[done]` T6.2 Implement provider / key / rule CRUD APIs
  - Design link:
    - architecture sections 10, 18
  - Implemented in:
    - `src/control_plane/mod.rs`
    - `src/storage/mod.rs`
    - `src/storage/sqlite.rs`
  - Current note:
    - provider bundle CRUD is exposed at `/admin/providers/:provider_id`
    - key CRUD is exposed at `/admin/providers/:provider_id/keys/:key_id`
    - model-rule CRUD is exposed at `/admin/providers/:provider_id/model-rules/*model`
    - slash-containing model ids are supported through the wildcard route
    - deleting the last key is rejected as invalid config instead of mutating the provider into an unusable state

- `[done]` T6.3 Implement runtime snapshot APIs
  - Design link:
    - architecture sections 17, 18
  - Implemented in:
    - `src/control_plane/mod.rs`
  - Current note:
    - provider-level runtime snapshots are exposed at `/admin/providers/:provider_id/runtime`
    - aggregated runtime list is exposed at `/admin/runtime/providers`
    - responses are shaped for direct WebUI consumption and reuse existing limiter snapshot types

### Phase 7. UI and Nyro integration

- `[done]` T7.1 Build standalone runtime/admin UI
  - Design link:
    - architecture section 18
    - reuse-inventory section 2
  - Implemented in:
    - `src/web/mod.rs`
    - `src/app/mod.rs`
    - `webui/src/App.tsx`
    - `webui/src/index.css`
    - `webui/src/lib/api.ts`
  - Current note:
    - standalone admin panel is served at `/admin`
    - provider management uses `/admin/providers` and `/admin/providers/:provider_id`
    - runtime view uses `/admin/runtime/providers` and `/admin/providers/:provider_id/runtime`
    - UI now uses a Chinese `React + Vite + Tailwind` frontend under `webui/`
    - Rust serves the built `webui/dist` assets from `/admin`

- `[done]` T7.2 Validate `nyro -> upstream-gateway -> real upstream`
  - Design link:
    - architecture sections 8, 15, 19
  - Implemented in:
    - `examples/gemini-first/bootstrap.template.json`
    - `examples/gemini-first/nyro.standalone.yaml`
    - `examples/gemini-first/validate.ps1`
    - `docs/gemini-first-cutover.md`
  - Validation note:
    - reproducible PowerShell validation now exercises `Anthropic client request -> Nyro -> upstream-gateway -> real Gemini`
    - provider config is persisted through SQLite bootstrap seeding before startup

- `[done]` T7.3 Switch one family first, preferably Gemini
  - Design link:
    - architecture section 19
  - Implemented in:
    - `docs/gemini-first-cutover.md`
    - `examples/gemini-first/README.md`
  - Current note:
    - Gemini is now the documented first-family cutover path
    - OpenAI and Anthropic can remain direct while Nyro Gemini egress alone points at `upstream-gateway`

## 5. Immediate next tasks

Current execution target:

1. ordered migration tasks are complete
2. non-stream provider-native usage parsing is wired into post-response settlement
3. remaining follow-up work is limited to stream-side usage extraction when vendors emit it

## 6. Migration notes

The old implementation is not being replayed into `nyro`.

Instead:

- the archive branch remains the reference
- `upstream-gateway` becomes the new implementation target
- `nyro` stays synced upstream and later points to this service through provider endpoints
