# Rate Limit Fix Plan

This document turns the latest limiter review into an execution plan.

## 1. Scope

Target module set:

- `src/config/rate_limit.rs`
- `src/provider/mod.rs`
- `src/selector/mod.rs`
- `src/data_plane/mod.rs`
- `src/data_plane/usage.rs`
- related tests

## 2. Priority

### P1

No current stop-ship blocker was identified.

### P2

#### P2.1 Stream-side actual usage settlement

Problem:

- stream requests currently settle with reserved input/output tokens
- when `tpm_mode = input_and_output`, large output reservations can keep hot keys artificially exhausted

Planned fix:

- add stream usage tracker for OpenAI / OpenAI Responses / Anthropic / Gemini SSE payloads
- settle with observed vendor usage when present
- keep conservative fallback only when usage is absent

Status:

- `[done]` non-stream provider-native usage settlement
- `[done]` stream provider-native usage settlement
- `[done]` regression coverage for normal completion and mid-stream break fallback

#### P2.2 Key weight must affect selection

Problem:

- `GatewayKey.weight` is stored and exposed in config/UI
- selector currently ignores it and behaves as plain round-robin

Planned fix:

- carry `weight` into normalized internal key config
- change selection from uniform round-robin to weighted logical-slot rotation
- preserve disabled/over-limit skipping behavior

Status:

- `[done]` wire weight into internal config and selector
- `[done]` weighted rotation regression test

#### P2.3 Shrink acquire hot-path lock scope

Problem:

- same `provider + model` acquire operations still serialize behind cursor lock

Planned fix:

- keep cursor lock only for start-position reservation
- perform quota scan and key-state checks outside the cursor mutex
- accept slight fairness approximation in exchange for much better hot-shard concurrency

Status:

- `[done]` minimize cursor critical section
- `[done]` keep only short cursor reservation/update locking on hot path

### P3

#### P3.1 Avoid per-request config re-normalization

Planned fix:

- cache normalized rate-limit config per provider bundle revision
- rebuild only on config writes

Status:

- `[done]` normalized rate-limit config cache is rebuilt on store writes
- `[done]` data-plane and runtime snapshot paths read from cached normalized config

#### P3.2 Tighten selector input abstraction

Planned fix:

- remove selector inputs that do not affect selection behavior
- keep protocol-specific concerns in estimator / data-plane layers

Status:

- `[done]` removed selector/runtime dependency on `upstream_protocol`
- `[done]` protocol-specific logic stays in request parsing, estimator, and settlement layers

#### P3.3 Expand production-like regression coverage

Planned fix:

- add stream usage settlement tests
- add weighted rotation tests
- add hot-shard concurrency benchmark or regression harness

Status:

- `[done]` stream usage settlement tests are in place
- `[done]` weighted rotation tests are in place
- `[done]` heavy-weight saturated-key regression harness guards the hot-shard scan path

## 3. Acceptance

The P2 set is considered complete when:

1. stream requests settle against actual usage when upstream emits usage
2. key `weight` changes real selection order
3. selector no longer holds cursor mutex across full key scan
4. regression tests cover these behaviors

Current result:

- `cargo test` passes in `upstream-gateway/`
- remaining P2/P3 items in this document have been implemented
