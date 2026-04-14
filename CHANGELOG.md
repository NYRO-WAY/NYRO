# Changelog

All notable changes to Nyro will be documented in this file.

---

## v1.6.1

> Released on 2026-04-14

#### Features

- **Stream replay TPS throttle**: add `stream_replay_tps` (default 100) to `ExactCacheConfig` and `SemanticCacheConfig`; set to `0` to disable throttle and restore instant-flush behavior; implement `split_text_deltas` helper to chunk large `TextDelta`/`ReasoningDelta` into ~1-token pieces for smooth per-token pacing; first SSE chunk is always sent immediately to keep TTFT at zero
- **Per-cache response header control**: add `expose_headers` (default `true`) to both cache configs, independently controlling whether `X-NYRO-CACHE-*` headers are emitted per exact vs semantic cache; rename response headers to uppercase: `X-NYRO-CACHE` / `X-NYRO-CACHE-KEY` / `X-NYRO-CACHE-SCORE`
- **Embedded WebUI in server binary**: remove `--webui-dir` CLI param; embed `webui/dist` into the binary via `rust-embed`; add `--log-level` param (env `NYRO_LOG_LEVEL`) to replace hardcoded tracing filter; add env-var support for key params (`NYRO_PROXY_HOST`, `NYRO_ADMIN_TOKEN`, etc.)
- **Browser token authentication**: add `/login` page and token auth flow for browser-based WebUI access; add logout icon in web topbar when admin token is active (Tauri IPC path is unaffected)
- **Resource enable/disable toggles**: add enable/disable toggle buttons in WebUI list pages for providers, routes, and API keys; show danger badge only when resource is disabled

#### Improvements

- **Server CLI simplification**: reduce CLI params from 27 to 18; rename `--admin-key` → `--admin-token`, `--storage-dsn-env` → `--postgres-dsn`; prefix Postgres pool params with `postgres-*`; rename `--sqlite-migrate-on-start` → `--migrate-on-start`; remove 9 cache CLI params (now managed via Admin API / WebUI + DB)
- **Status field unification**: rename `providers.is_active`, `routes.is_active`, `api_keys.status` to `is_enabled` (BOOLEAN) across all storage backends, SQL queries, Rust models, and WebUI; add non-breaking schema migration for both SQLite and PostgreSQL

#### Fixes

- Fix missing `stream_replay_tps` and `expose_headers` fields in nyro-server cache config initializers (`main.rs` and `yaml_config.rs`) causing a compile error after feat #45
- Fix standalone mode proxy host/port priority bug where CLI value was silently overridden by the default
- Fix `backend.ts` null-data bug where `json.data ?? json` returned the full response object when `data` was `null`, causing `.trim()` crashes on Providers and Settings pages

#### Documentation

- Consolidate 8 stale design docs into a single `docs/design/architecture.md`
- Add `docs/standalone/` with full Standalone mode guide including cache section
- Remove `examples/` directory (content inlined into standalone docs)
- Fix stale CLI params across `docs/server/README.md`, `README.md`, and `README_CN.md`

---

## v1.6.0

> Released on 2026-04-12

#### Features

- **End-to-end cache system**: implement modular exact/semantic cache backends with SSE stream replay for streaming cache hits and singleflight request coalescing to prevent cache stampede under concurrent misses
- **Ingress route aliases**: add non-versioned route aliases (`/chat/completions`, `/messages`, `/responses`, `/models/:model_action`) alongside versioned paths for broader client compatibility
- **OpenAI-compatible models listing**: add `/v1/models` and `/models` endpoints returning route-aware model lists, with API-key-bound model filtering and graceful degradation
- **Semantic vector dimension lifecycle**: auto-rebuild semantic vector tables when embedding dimensions change, persist active dimensions in settings, and support transactional pgvector rebuild with clear permission fallback guidance

#### Improvements

- **Cache system unification**: unify exact/semantic cache runtime configuration and hot-reload behavior; enforce chat/embedding route type isolation with OpenAI endpoint validation; update WebUI route/settings flows accordingly
- **Settings save UX**: refactor settings modules to explicit save actions with unsaved-change feedback; split API key list status into management and validity badges; align SQLite semantic cache scoring with cosine distance expectations
- **Global cache/proxy linkage**: route list badges and form controls now reflect global cache/proxy enabled state; route form hides cache toggles and provider form hides proxy toggle when respective global setting is off; saved config is preserved and auto-restores on re-enable
- **Semantic cache config linkage**: clear `embedding_route` when semantic cache is toggled off; block deletion of an embedding route referenced by semantic cache config with an error dialog

#### Fixes

- Fix cache-hit log model names by persisting `actual_model` in cache entries so upstream models are reported consistently on cache hits
- Fix global cache/proxy toggles lacking proper linkage with route badges and provider proxy badge in list views
- Fix proxy backend returning 502 when global proxy is disabled but a route has `use_proxy=true`; fall back to direct HTTP client instead
- Standardize cache wording in UI and docs without changing existing config keys

#### Refactoring & Cleanup

- **Remove MySQL backend**: drop MySQL storage implementation, config/CLI paths, and sqlx mysql feature; supported backends are now SQLite / PostgreSQL / Memory
- **GitHub org rename**: update all references from `NYRO-WAY` to `nyroway` across configs, scripts, docs, install scripts, and frontend code
- Update Zai provider default capabilities source

---

## v1.5.0

> Released on 2026-04-02

#### Features

- **Storage backend expansion**: add multi-backend storage abstraction and server-side backend configuration support for SQLite / MySQL / PostgreSQL
- **Multi-target routing evolution**: add multi-target route selection and weighted/priority strategy flow, and support `weight=0` as an explicit disable state
- **Gateway protocol architecture refresh**: support multi-protocol providers, protocol-agnostic route behavior, and standalone YAML route/provider loading
- **Proxy extensibility upgrade**: extract `ProviderAdapter` and align provider-level proxy controls for cleaner provider integration

#### Improvements

- **Deprecated field cleanup**: remove legacy route/provider/log/storage fields and simplify schema/query paths around active routing behavior
- **Gateway error typing standardization**: unify proxy/auth error `type` payloads under `NYRO_*` naming for consistent client-side handling
- **CLI integration polish**: improve web CLI config preview and sync generated Claude Code settings with `CLAUDE_CODE_NO_FLICKER=1`
- **Repository migration alignment**: update project/release references to `NYRO-WAY` organization and align updater/release script paths
- **Build/runtime layout cleanup**: split Docker runtime image and dev container structure for clearer CI/CD maintenance

#### Tests & Docs

- Refresh smoke tests and docs to match protocol-agnostic routing and the latest route/provider data model

## v1.4.0

> Released on 2026-03-21

#### Features

- **Protocol normalization layer upgrade**: add semantic internal response normalization and emit item-level reasoning/function-call outputs for Responses API flows
- **Provider preset capability unification**: unify provider preset handling with capability source parsing and ship an embedded models.dev snapshot for offline metadata
- **Connect CLI workflow enhancements**: align Codex/OpenCode sync outputs with runtime defaults, refine route state anchoring, and improve config action UX

#### Improvements

- **WebUI configuration interactions**: refine provider preset behaviors and route-edit model interactions for more predictable admin flows
- **Admin error surface consistency**: return structured provider/route conflict payloads and localize conflict messages in the UI
- **CLI panel layout polish**: reorder API key vs update-config controls, keep half-width action layout, and align preview hint spacing/offset behavior
- **Local UX defaults**: default initial locale state to `en-US` and render request timestamps in local timezone in Logs

#### Fixes

- Fix cross-protocol tool-call semantics by hardening tool-call/result correlation and normalizing thinking/text delta behavior across adapters
- Fix Google model discovery auth path and model normalization in admin provider discovery flow

#### Tests & Docs

- Add protocol regression coverage for tool IDs, finish reasons, schema mapping, and provider-policy removal behavior
- Add protocol architecture hardening design doc and refresh README/UI screenshots for latest console pages

## v1.3.0

> Released on 2026-03-18

#### Features

- **OpenAI Responses pipeline support**: add request/response transformation path for `/v1/responses` to improve tool-chain compatibility with modern OpenAI-style clients
- **Provider model test workflow**: introduce staged provider testing with unified action feedback in provider management flows
- **Ollama capability detection**: add vendor-aware capability checks to auto-handle tool-support differences by model
- **Gemini cURL preview improvement**: preserve `:` in model IDs (for example `gemma3:1b`) when rendering Connect page Gemini endpoint snippets

#### Improvements

- **Provider UX consistency**: improve vendor/channel synchronization and keep route-edit state reset behavior predictable
- **Route model discovery behavior**: only enable discovery dropdown when provider model endpoint is actually available
- **Admin error handling UX**: localize backend error messages consistently and unify failure dialog presentation across admin pages

#### Fixes

- Fix MiniMax + Codex interoperability issue where upstream rejects `system` role by normalizing responses instructions for MiniMax on Responses API ingress
- Fix OpenRouter model discovery behavior and restore provider auto-test flow after provider create
- Fix Windows desktop dropdown/search selection regression caused by drag-capture conflict in Tauri title-drag handling

## v1.2.0

> Released on 2026-03-15

#### Features

- **New Connect module**: add `Connect` page with `Code` / `CLI` tabs, protocol-aware route selection, and generated examples for Python / TypeScript / cURL
- **Desktop CLI integration**: support readiness detection plus config sync/restore for Claude Code, Codex CLI, Gemini CLI, and OpenCode
- **CLI config preview & copy flow**: show per-file update fragments and inline copy action in preview area
- **API key policy upgrade**: enforce default-deny route authorization for protected routes and adopt `sk-<32 hex>` key format
- **Quota extension**: add `RPD` (requests per day) to API key model, admin CRUD, UI forms, and proxy enforcement

#### Improvements

- **API Keys page restructure**: split create/edit forms into three sections (Basic Information, Access Permission, Access Quota), align widths, and keep key/validity immutable in edit mode
- **Provider form polish**: add API key show/hide icon, restore API key echo in edit form, and align edit/create layout behavior
- **Route form consistency**: align edit layout with create layout and keep single-row inputs/selects at half width
- **Statistics time-range coverage**: make selected hours apply consistently to overview, model, and provider stats across WebUI + backend + Tauri commands

#### Fixes

- Fix `build-and-smoke` CI script for the new auth flow (remove deprecated `--proxy-key`, create/bind smoke API key to routes)
- Fix CLI sync argument mismatch (`toolId` / `apiKey`) and improve frontend error message parsing for failed sync operations
- Restore Codex `wire_api` compatibility by switching back to `responses`
- Improve dropdown/search panel visual consistency in forms and access-control layout details

#### CI & Release

- Automate Homebrew Cask checksum updates in desktop release workflow after artifacts are built
- Update release and design docs for latest route/API key behavior and installation guidance

---

## v1.1.0

> Released on 2026-03-13

#### Features

- **Route matching redesign**: switch from fuzzy `match_pattern` to exact matching on `(ingress_protocol, virtual_model)` for OpenAI / Anthropic / Gemini ingress
- **New API key system**: add `api_keys` + `api_key_routes` data model and full CRUD management with `sk-<32 hex>` key format
- **Route-level access control**: route first, then authorize API key only when `access_control` is enabled; support key binding to specific routes or global access
- **Quota enforcement for API keys**: add `RPM`, `TPM`, `TPD`, key status, and expiration checks in proxy authorization flow

#### Improvements

- **Backend migration & compatibility**:
  - add and backfill new route/provider/log fields (`ingress_protocol`, `virtual_model`, `access_control`, `channel`, `api_key_id`)
  - remove legacy route/provider fallback and priority behavior from active flow
- **Admin/API surface expansion**: add server + Tauri management endpoints/commands for API key operations
- **WebUI route & key experience refresh**:
  - new API Keys page with searchable multi-select route binding
  - create-route layout aligns provider/model fields in one row and auto-inherits target model into virtual model
  - provider create/edit flow now persists and re-anchors provider preset/channel identifiers
- **UI component standardization**: introduce shadcn-style `Badge`, `Switch`, `Checkbox`, `Dialog`, `Combobox`, `Command`, `Popover`, `MultiSelect`, `Tabs`, and related fields
- **Provider icon behavior polish**: provider list uses supplier icon as primary (light: color, dark: monochrome), protocol badge icon remains protocol-based
- **Version display automation**: settings page version now reads build-injected app version instead of hard-coded text

#### Fixes

- Fix searchable dropdown visual layering and non-transparent panel background
- Fix search result filtering and hover/highlight feedback in custom dropdown components
- Align Homebrew install docs to standard `brew install --cask nyro` flow

#### Documentation

- Add route/API key design spec: `docs/design/route-apikey.md`
- Add provider base URL/channel design note: `docs/design/provider-base-urls.md`
- Update `README.md` and `README_CN.md` installation commands and related notes

---

## v1.0.1

> Released on 2026-03-10

#### Improvements

- **Full ARM64 / aarch64 support**: native builds for all platforms using GitHub Actions ARM runners (`ubuntu-24.04-arm`, `windows-11-arm`, `macos-latest`)
  - Desktop: Linux aarch64 AppImage, Windows ARM64 NSIS installer
  - Server: Linux aarch64, macOS aarch64, Windows ARM64 binaries
- **macOS Intel native build**: use `macos-15-intel` runner instead of cross-compiling on ARM
- **Homebrew Cask**: `brew tap shuaijinchao/nyro && brew install --cask nyro` (separate `homebrew-nyro` tap repo with auto version bump on release)
- **Install scripts**: one-line install for macOS/Linux (`install.sh`) and Windows (`install.ps1`), with automatic quarantine removal on macOS
- **Frontend chunk splitting**: Vite `manualChunks` for react, query, and charts to eliminate >500kB bundle warning

#### Fixes

- **CI**: exclude `nyro-desktop` from `cargo check --workspace` to avoid GTK dependency on Linux CI
- **CI**: remove unsupported `--manifest-path` from `cargo tauri build`
- **CI**: add `pkg-config` and `libssl-dev` for server build on ubuntu-latest

#### Cleanup

- Remove MSI and deb packages from desktop release (NSIS + AppImage only)
- Remove desktop SHA256SUMS.txt (updater `.sig` files provide integrity verification)
- Move Homebrew Cask to dedicated `homebrew-nyro` repository
- Fix `main` → `master` branch references in install scripts and README

---

## v1.0.0

> Released on 2026-03-09

First public release of Nyro AI Gateway — a complete rewrite from the original OpenResty/Lua API Gateway to a pure Rust local AI protocol gateway.

#### Features

- **Multi-protocol ingress**: OpenAI (`/v1/chat/completions`), Anthropic (`/v1/messages`), Gemini (`/v1beta/models/*/generateContent`) — both streaming (SSE) and non-streaming
- **Any upstream target**: routes to any OpenAI-compatible, Anthropic, or Gemini provider
- **Provider management**: create, edit, delete providers with base URL and encrypted API key
- **Route management**: priority-based routing rules with model override and fallback provider support
- **Request logging**: persistent SQLite log with protocol, model, latency, status, and token counts
- **Usage statistics**: overview dashboard with hourly/daily charts and provider/model breakdowns
- **API key encryption**: AES-256-GCM encryption for stored API keys
- **Bearer token auth**: optional independent authentication for proxy and admin endpoints
- **Desktop application**: Tauri v2 cross-platform desktop app (macOS / Windows / Linux)
  - System tray with quick access menu
  - Optional auto-start on system login
  - In-app auto-update via Tauri updater
  - Native macOS title bar integration
  - Dark / light mode toggle
  - Chinese / English language switching
- **Server binary**: standalone `nyro-server` binary for server deployment with HTTP WebUI access
  - Configurable bind addresses for proxy and admin ports
  - CORS allowlist configuration
  - Non-loopback binding enforces auth key requirement
- **CI/CD**: GitHub Actions workflows for cross-platform desktop bundle and server binary releases
