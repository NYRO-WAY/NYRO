# Changelog

All notable changes to Nyro will be documented in this file.

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
