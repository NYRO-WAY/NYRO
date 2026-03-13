# Changelog

All notable changes to Nyro will be documented in this file.

---

## v1.1.0

> Released on 2026-03-13

#### Features

- **Route matching redesign**: switch from fuzzy `match_pattern` to exact matching on `(ingress_protocol, virtual_model)` for OpenAI / Anthropic / Gemini ingress
- **New API key system**: add `api_keys` + `api_key_routes` data model and full CRUD management with `sk-nyro-xxxx` key format
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
