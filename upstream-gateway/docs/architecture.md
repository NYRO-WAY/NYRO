# upstream-gateway Architecture

## 1. Goal

Build a standalone upstream relay service so that:

- `nyro` keeps protocol conversion and route selection
- upstream key-pool scheduling and rate limiting move out of `nyro-core`
- future upstream `nyro` merges stay low-conflict

Target request path:

```text
Claude Code / OpenAI SDK / Gemini CLI
  -> nyro
  -> upstream-gateway
  -> OpenAI / Anthropic / Gemini
```

The new service is an upstream-native relay, not a second protocol-conversion engine.

## 2. Why this split

The previous in-tree implementation placed these concerns directly inside `nyro-core`:

- upstream key selection
- provider-native input token estimation
- RPM / RPD / TPM enforcement
- lease / settle / rollback state
- runtime snapshot API and UI wiring

That touched `proxy`, `protocol`, `provider`, `db`, `storage`, `admin`, and `webui` at once. Rebase pain came from mixing vendor quota control into `nyro`'s core proxy and protocol pipeline.

The new boundary fixes that by moving real-upstream access control into a dedicated service.

## 3. Design principles

1. `nyro` remains the protocol-conversion layer.
2. `upstream-gateway` remains a provider-native relay layer.
3. Requests entering `upstream-gateway` must already be encoded in the target upstream protocol.
4. Rate limiting is enforced at the last possible point before real upstream dispatch.
5. Streaming responses must remain streaming end-to-end.
6. Runtime state is isolated from `nyro` and stored by the new service.
7. Reused logic is copied by capability boundary, not by old file layout.

## 4. Scope

### In scope

- upstream provider definitions
- provider-bound key pools
- per-model RPM / RPD / TPM rules
- provider-native token estimation
- lease / settle / rollback lifecycle
- runtime snapshots and admin APIs
- standalone admin panel and WebUI

### Out of scope

- OpenAI <-> Anthropic <-> Gemini cross-protocol conversion
- reuse of `nyro`'s end-user route auth model
- shared runtime limiter state with `nyro`
- re-embedding upstream rate-limit logic into `nyro-core`

## 5. Rust technology choices

Recommended stack:

- web framework: `axum`
- HTTP server runtime: `hyper` via `axum`
- async runtime: `tokio`
- outbound upstream client: `reqwest`
- body / stream primitives: `http-body-util`, `bytes`, `futures`
- routing / middleware: `tower`, `tower-http`
- serialization: `serde`, `serde_json`
- persistence:
  - phase 1 default: `sqlx + sqlite`
  - phase 2 optional: `sqlx + postgres`
- in-memory runtime maps:
  - `dashmap` for registries / lookup maps
  - `parking_lot::Mutex` for hot-path small critical sections

Why `axum`:

- `nyro` already uses `axum`, so the team already understands the ecosystem
- first-class streaming response support
- easy path extraction for Gemini-style routes
- low-friction integration with `tokio` and `reqwest`

Why `reqwest`:

- already proven in `nyro`
- good streaming support with `bytes_stream()`
- easy header/query/body composition for the three vendor families

## 6. Service shape

The new service has two planes:

- data plane
  - receives provider-native requests from `nyro`
  - performs selection, dispatch, stream passthrough
- control plane
  - provider/key/rule CRUD
  - runtime inspection
  - health and admin APIs

The planes should not share the same listener long-term.

Required target shape:

- proxy listener
  - serves only provider-native ingress routes and proxy health
- admin listener
  - serves only admin APIs, runtime inspection, and admin UI

Recommended environment split:

- `UPSTREAM_GATEWAY_PROXY_BIND_ADDR`
- `UPSTREAM_GATEWAY_ADMIN_BIND_ADDR`

Recommended defaults:

- proxy: `127.0.0.1:2080`
- admin: `127.0.0.1:2081`

Recommended high-level layout:

```text
upstream-gateway/
├── README.md
├── docs/
│   ├── architecture.md
│   └── reuse-inventory.md
├── Cargo.toml                     # later
├── src/                           # later
│   ├── main.rs
│   ├── app/
│   ├── config/
│   ├── data_plane/
│   ├── control_plane/
│   ├── provider/
│   ├── selector/
│   ├── runtime/
│   ├── estimator/
│   ├── upstream/
│   ├── storage/
│   └── web/
└── webui/                         # later
```

## 7. Protocol boundary

`upstream-gateway` should expose provider-native ingress endpoints.

Supported inbound families:

- OpenAI-compatible
  - `POST /providers/:provider_id/openai/v1/chat/completions`
  - `POST /providers/:provider_id/openai/v1/responses`
  - `POST /providers/:provider_id/openai/v1/embeddings` optional
- Anthropic-compatible
  - `POST /providers/:provider_id/anthropic/v1/messages`
- Gemini-compatible
  - `POST /providers/:provider_id/google/v1beta/models/:model_action`
  - `POST /providers/:provider_id/google/models/:model_action`

Important rule:

- the service accepts native requests and forwards native requests
- it does not own a canonical IR for cross-vendor conversion

That keeps it narrow and avoids recreating `nyro`.

## 8. Nyro integration model

`nyro` should treat `upstream-gateway` as an ordinary provider endpoint.

Example:

- `nyro` receives Anthropic-format traffic from Claude Code
- `nyro` converts it to Gemini-native request body
- `nyro` sends that Gemini request to `upstream-gateway`
- `upstream-gateway` selects a Gemini key, enforces Gemini limits, and forwards to real Gemini

Recommended endpoint roots:

```text
/providers/{provider_id}/openai
/providers/{provider_id}/anthropic
/providers/{provider_id}/google
```

Then `nyro` `protocol_endpoints` can point to these roots and keep using its own egress path logic.

Example `nyro` provider endpoint mapping:

```text
openai    -> http://127.0.0.1:2080/providers/openai-prod/openai
anthropic -> http://127.0.0.1:2080/providers/claude-prod/anthropic
gemini    -> http://127.0.0.1:2080/providers/gemini-prod/google
```

## 9. Internal modules

Module responsibilities:

- `data_plane`
  - native ingress routes
  - request body collection
  - model extraction
  - stream vs non-stream branching
  - request passthrough orchestration
- `provider`
  - provider definitions
  - real upstream vendor type
  - key pool and model rules
- `selector`
  - choose an available key from key pool
  - round-robin cursor by `provider + model`
  - lazy window cleanup
- `runtime`
  - active leases
  - settle / rollback
  - runtime snapshots
  - idle state cleanup
- `estimator`
  - provider-native input token estimation
  - output reservation estimation
- `upstream`
  - real upstream HTTP request construction
  - provider auth/header/query rules
  - streaming relay helpers
- `storage`
  - config persistence
  - SQLite / Postgres abstraction
- `control_plane`
  - admin CRUD
  - runtime inspection APIs
- `web`
  - admin HTTP route wiring

## 10. Core data model

The new service owns its own provider model and does not reuse `nyro` provider rows directly.

Recommended top-level objects:

- `GatewayProvider`
- `GatewayKey`
- `GatewayModelRule`
- `DailyResetConfig`

Suggested provider shape:

```text
GatewayProvider
- id
- name
- vendor: openai | anthropic | gemini
- base_url
- auth_strategy
- inbound_access optional
- daily_reset
```

Suggested key shape:

```text
GatewayKey
- id
- provider_id
- display_name optional
- api_key
- enabled
- weight optional
```

Suggested model rule shape:

```text
GatewayModelRule
- provider_id
- model
- rpm optional
- rpd optional
- tpm optional
- tpm_mode: input_only | input_and_output
- tokenizer_override optional
- tokenizer_encoding optional for OpenAI family
- output_reservation optional
```

Recommended runtime snapshot objects:

- `ProviderRuntimeSnapshot`
- `ProviderModelRuntimeSnapshot`
- `KeyRuntimeSnapshot`
- `RateLimitMetricSnapshot`

## 11. Storage model

Phase 1 persistence:

- config persisted
- sliding-window runtime state in memory

Recommended tables:

- `gateway_providers`
- `gateway_keys`
- `gateway_model_rules`
- `gateway_settings`

Not persisted in phase 1:

- active leases
- RPM sliding window contents
- TPM sliding window contents
- live cursor positions

Reason:

- keeps hot-path simple
- avoids premature distributed-state complexity
- restart loss is acceptable for initial phase

## 12. Rate-limit semantics

### Unit of enforcement

Rate limiting is tracked per:

```text
provider + key + model
```

### Window semantics

- `RPD`: fixed window
- `RPM`: sliding window
- `TPM`: sliding window

### Daily reset

`RPD` uses configured wall-clock reset:

- timezone
- hour
- minute

### TPM rule

Before dispatch:

```text
current_window_tokens + request_input_tokens + optional_output_reservation <= limit
```

Rules:

- when `tpm_mode = input_only`, `optional_output_reservation = 0`
- when `tpm_mode = input_and_output`, reservation participates in admission check
- token estimation must run after the request body is fully finalized in upstream-native format

### Key behavior

- a limit hit does not mutate persistent `key.enabled`
- temporary unavailability is runtime-only
- candidate keys are scanned from a rotating cursor
- cursor is maintained at `provider + model` granularity

## 13. Token estimation

Token estimation belongs in `upstream-gateway` because it sees the final provider-native payload.

Rules:

1. estimate from final outbound body, not from user-origin body
2. use provider-native tokenizers or structured estimators
3. conservative overestimation is acceptable
4. underestimation is not acceptable
5. unknown tokenizer mappings should degrade to configured fallback, not panic the request path

Provider guidance:

- Gemini
  - use `gemini-tokenizer`
  - count structured request fields, not raw `json.to_string()`
  - keep `inlineData` conservative
- OpenAI
  - structured estimation over messages, tools, multimodal parts
  - support explicit tokenizer override for unknown model names
- Anthropic
  - structured block estimation
  - align thinking-block counting with official semantics

## 14. Request passthrough design

This is the most important implementation boundary.

### 14.1 Inbound handling

The service should not deserialize every inbound request into a giant shared enum first.

Recommended approach:

1. read path and identify:
   - provider id
   - protocol family
   - route kind
2. collect raw body bytes
3. parse body into `serde_json::Value`
4. extract:
   - actual model
   - stream flag
   - output limit field
5. estimate input tokens from the final native JSON body
6. acquire a key lease
7. forward upstream

This keeps the service flexible and avoids rebuilding `nyro`'s semantic IR.

### 14.2 Outbound request construction

Outbound request should be built from:

- provider base URL
- inbound route kind
- selected key
- original native JSON body

The request body should be forwarded unchanged except for vendor-required auth/query mechanics.

Allowed mutations:

- add auth header
- add vendor-specific query parameter
- add or override transport headers

Disallowed mutations on the hot path:

- semantic field rewriting
- protocol structure rewriting
- message normalization

Those belong to `nyro`, not `upstream-gateway`.

### 14.3 Header passthrough rules

Recommended policy:

- drop inbound auth intended for `upstream-gateway`
- do not forward caller auth token to real upstream
- forward content-type if appropriate
- forward accept headers if needed for SSE behavior
- explicitly set vendor-required auth/query fields from selected key

### 14.4 Query handling

OpenAI / Anthropic:

- typically auth via headers

Gemini:

- support provider policy for `key=...` query parameter
- optionally also emit bearer header if needed by a compatible endpoint

### 14.5 Timeout and retry

Recommended phase 1 behavior:

- one upstream attempt per selected key
- no hidden automatic retries inside a single key lease
- selector may try another key only before response body starts streaming
- once bytes start flowing, never switch upstream mid-stream

## 15. Streaming passthrough design

Streaming must remain transparent.

### 15.1 Core rule

`upstream-gateway` must not aggregate the full response before returning it.

### 15.2 OpenAI / Anthropic / Gemini behavior

- OpenAI SSE stays incremental
- Anthropic SSE stays event-by-event
- Gemini SSE stays chunk-by-chunk

### 15.3 Implementation approach

Recommended server-side flow:

1. execute upstream request with `reqwest`
2. read `resp.bytes_stream()`
3. wrap that stream into an `axum` response body
4. stream bytes through as they arrive
5. in parallel, accumulate minimal settlement signals:
   - final usage
   - final finish reason
   - stream completed or aborted

Recommended response construction:

- use `axum::body::Body::from_stream(...)`
- set `content-type` to the real upstream stream type
- preserve SSE framing exactly

### 15.4 Settlement during streaming

Lease settlement must be post-stream bookkeeping.

Recommended behavior:

- on normal completion:
  - settle using final usage if present
  - otherwise settle using reserved / inferred fallback
- on transport failure before completion:
  - rollback or settle conservatively depending on whether upstream already accepted and emitted bytes

Phase 1 simplification:

- if no usage is emitted, settle with:
  - actual input tokens already known
  - output tokens = reserved output or 0 by policy

## 16. Request lifecycle

```text
receive native request
-> resolve provider
-> parse JSON body
-> extract actual model
-> detect stream mode
-> estimate request_input_tokens
-> compute optional output reservation
-> selector.acquire(provider, model, request_input_tokens, reservation)
-> construct real upstream URL + auth
-> dispatch upstream request with reqwest
-> if non-stream:
   -> await full response
   -> parse usage / finish reason
   -> settle lease
   -> return body
-> if stream:
   -> relay bytes as stream
   -> observe final usage / completion
   -> settle or rollback lease
```

## 17. Runtime state strategy

Initial runtime strategy:

- configuration persisted
- sliding-window state in memory
- no cross-process shared limiter in phase 1

Recommended runtime internals:

- shard locks by `provider + model` and `provider + key + model`
- separate cursor state from key-state maps
- idle provider-model and key-state cleanup
- active lease map for settle / rollback

Suggested internal state split:

- `ProviderModelCursorState`
- `ProviderModelKeyState`
- `ActiveLeaseState`

## 18. Control plane and UI

The runtime panel should live with `upstream-gateway`, not inside `nyro`'s provider page.

Recommended admin capabilities:

- provider CRUD
- key-pool CRUD
- model rule CRUD
- runtime snapshot
- per-key occupancy view
- per-model summary
- config validation preview

Recommended UI split:

- Providers
- Keys
- Model Rules
- Runtime
- Settings

Recommended network split:

- proxy port
  - data-plane only
- admin port
  - control-plane APIs and UI only

This keeps `nyro` UI low-intrusion and easier to sync with upstream.

## 19. Migration design

### 19.1 Migration goal

Migrate complete functionality, not the old file structure.

### 19.2 Reuse strategy

Reuse by category:

- direct capability reuse
  - config
  - runtime
  - selector
  - tests
- split-and-adapt reuse
  - estimator
  - runtime UI concepts
- conceptual reuse only
  - old `proxy` hook placement
  - old `admin` wiring

### 19.3 Phase breakdown

#### Phase 0

- keep `nyro` synced with upstream
- create `upstream-gateway/` as design-first directory
- preserve old implementation in archive branch

#### Phase 1

- create standalone crate skeleton
- define local provider/config types
- port `errors`, `config`, `types`, `selector`, `runtime`
- port `rate_limit_runtime` tests

#### Phase 2

- implement data-plane ingress routes
- implement upstream request builder
- wire real `reqwest` passthrough
- verify stream passthrough first on Gemini

#### Phase 3

- split and port estimator logic
- implement provider-native token fallback rules
- add runtime snapshot API

#### Phase 4

- build standalone admin panel
- point one `nyro` provider family at the new service
- validate end-to-end with Claude Code and Gemini

#### Phase 5

- move remaining families
- stop carrying old in-tree limiter path in `nyro`

## 20. Initial implementation constraint

For the next coding phase, `upstream-gateway` should remain outside the main Rust workspace until:

- crate boundaries are confirmed
- the minimal executable skeleton is ready
- reused code has been copied and renamed cleanly

That avoids breaking the main build while we extract logic from the archived implementation.
