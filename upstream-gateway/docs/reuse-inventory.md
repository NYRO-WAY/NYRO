# Reuse Inventory From `archive/upstream-rate-limit-old`

## 1. Purpose

This document records which parts of the archived in-tree implementation are worth reusing for `upstream-gateway`, which need reshaping, and which should be left behind.

Reference source branch:

```text
archive/upstream-rate-limit-old
```

Reference commit:

```text
d918766
```

## 2. Reuse categories

### Reuse mostly as-is

These modules contain logic that is conceptually service-local and not tightly coupled to `nyro` protocol conversion:

1. `crates/nyro-core/src/rate_limit/config.rs`
   - reusable config normalization
   - reusable `TpmMode`
   - reusable daily reset parsing
   - needs rename from `UpstreamRateLimit*` to service-local model names

2. `crates/nyro-core/src/rate_limit/errors.rs`
   - reusable error taxonomy
   - should be split into admin/config/data-plane error layers later

3. `crates/nyro-core/src/rate_limit/types.rs`
   - reusable lease, snapshot, summary, and selection input shapes
   - remove direct dependency on `ProtocolId` if not needed in the final service

4. `crates/nyro-core/src/rate_limit/selector.rs`
   - core value of old implementation
   - contains round-robin cursor, lazy cleanup, key availability logic
   - should be one of the first extraction targets

5. `crates/nyro-core/src/rate_limit/runtime.rs`
   - good trait boundary for acquire / settle / rollback
   - useful as the new runtime service contract

6. `crates/nyro-core/tests/rate_limit_runtime.rs`
   - high-value behavioral regression tests
   - should be ported early to protect extraction work

### Reuse with reshaping

These modules contain valuable logic but are currently entangled with `nyro` internals or naming:

1. `crates/nyro-core/src/rate_limit/estimator.rs`
   - valuable provider-native estimation logic
   - should be split into:
     - `estimator/openai.rs`
     - `estimator/anthropic.rs`
     - `estimator/google.rs`
   - remove dependency on `nyro` request IR where possible
   - keep structured counting logic, not the old fallback assumptions blindly

2. `docs/design/upstream-rate-limiting.md`
   - useful as prior detailed design history
   - should not be copied verbatim as the new architecture source of truth
   - concepts should be re-expressed under the new service boundary

3. `docs/design/upstream-rate-limiting-tasks.md`
   - good source for development sequencing ideas
   - task files should be regenerated once crate boundaries settle

4. `webui/src/components/upstream-rate-limit-runtime-view.tsx`
   - useful as UI interaction reference
   - should not be copied directly into a new service without decoupling backend types

5. `webui/src/pages/upstream-rate-limits.tsx`
   - good reference for runtime panel layout
   - should be rewritten against new admin APIs

### Reuse conceptually only

These areas teach us what the new service needs, but should not be copied directly because they are too coupled to `nyro`'s in-process architecture:

1. `crates/nyro-core/src/proxy/handler.rs`
   - old admission / settle hook placement is conceptually useful
   - direct code copy is not recommended
   - in new architecture, this logic belongs in `upstream-gateway/data_plane`

2. `crates/nyro-core/src/admin/mod.rs`
   - useful for understanding provider summary and runtime snapshot wiring
   - do not reuse directly because it is tightly coupled to `nyro` admin services

3. `crates/nyro-core/src/db/models.rs`
   - useful for seeing old config fields
   - do not reuse directly because the new service should own a cleaner schema

4. `src-server/src/admin_routes.rs`
   - helpful as an admin HTTP style reference
   - new service should define its own routes

5. `src-server/src/yaml_config.rs`
   - useful only if phase 1 wants optional file-based bootstrap
   - not a required migration dependency

## 3. Do not reuse

These changes were high-intrusion by nature and should stay out of the new extraction target:

1. direct edits to `nyro` protocol codec registration structure
2. direct edits to `nyro` proxy dispatcher / handler integration
3. direct edits to `nyro` provider persistence models
4. direct edits to `nyro` existing provider page runtime embedding

These were acceptable for the experimental in-tree implementation but are exactly what made upstream merging painful.

## 4. First extraction order

Recommended extraction order from the archive branch:

1. `rate_limit/errors.rs`
2. `rate_limit/config.rs`
3. `rate_limit/types.rs`
4. `rate_limit/selector.rs`
5. `rate_limit/runtime.rs`
6. `rate_limit/estimator.rs` split by provider
7. `rate_limit_runtime.rs` tests

This order works because:

- config and types define stable contracts
- selector and runtime provide core behavior
- estimator is more complex and easier to split once the core boundary exists
- tests can then be ported onto the new service-local APIs

## 5. Expected refactors during extraction

### Naming refactor

Expected rename direction:

- `UpstreamRateLimitConfig` -> `GatewayProviderRateLimitConfig`
- `UpstreamKeyConfig` -> `GatewayKeyConfig`
- `UpstreamRateLimiter` -> `GatewayRateLimiter`

This is not mandatory immediately, but renaming early reduces confusion once the service gets its own provider model.

### Dependency refactor

Expected dependency cuts:

- remove `nyro-core::db::models::Provider`
- remove dependence on `nyro` `ProtocolId` where a local enum is sufficient
- remove dependence on `nyro` admin/runtime wrappers

### Layout refactor

Recommended future split:

```text
src/
  estimator/
    mod.rs
    openai.rs
    anthropic.rs
    google.rs
  runtime/
    mod.rs
    selector.rs
    state.rs
    lease.rs
```

## 6. Immediate next coding step

Before copying any code, the next implementation phase should:

1. create the standalone crate skeleton under `upstream-gateway/`
2. define local config and provider structs
3. port the selector/runtime core with minimal outside dependencies
4. only then start moving token estimator logic

That sequencing keeps the new service boundary clean instead of dragging `nyro-core` assumptions forward.

