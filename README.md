<p align="center">
  <img width="120" src="docs/images/NYRO-logo.png">
</p>

<p align="center">
  <strong>Nyro AI Gateway</strong>
</p>
<p align="center">
  Local-first AI protocol gateway for OpenAI / Anthropic / Gemini.<br>
  Keep your existing SDK, switch providers by configuration.
</p>

<p align="center">
  <a href="https://github.com/shuaijinchao/nyro/releases/latest"><img src="https://img.shields.io/github/v/release/shuaijinchao/nyro" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-Apache%202.0-blue.svg" alt="License"></a>
  <a href="README_CN.md"><img src="https://img.shields.io/badge/Language-%E4%B8%AD%E6%96%87-8A2BE2" alt="中文"></a>
</p>

---

## What is Nyro?

Nyro is a local AI protocol gateway that runs on your machine. It translates between OpenAI, Anthropic, and Gemini protocol formats so one client SDK can work with different providers.

Nyro ships in two forms:

- **Desktop App**: Tauri-based app for macOS / Windows / Linux
- **Server Binary**: standalone HTTP service with WebUI access

### Why Nyro?

| Principle | Meaning |
|---|---|
| **Local-first** | Requests and configuration stay under your control |
| **Protocol-neutral** | Any supported client protocol can route to any supported upstream |
| **Operational simplicity** | Manage providers, routes, logs, and stats from one UI |
| **Production-ready defaults** | Auth, encrypted key storage, fallback, release workflows |

### At a Glance

- **Ingress protocols**: OpenAI, Anthropic, Gemini
- **Response modes**: non-streaming + SSE streaming
- **Storage**: SQLite (`sqlx`)
- **Runtime**: Rust (`axum`, `tokio`, `reqwest`)
- **UI stack**: React 19 + TypeScript + Vite
- **Desktop shell**: Tauri v2 (tray, auto-start, updater)

---

## Core Capabilities

### Gateway

- **Multi-protocol ingress**: OpenAI / Anthropic / Gemini
- **Any provider as target**: route to OpenAI-compatible, Anthropic, Gemini upstreams
- **Streaming support**: full SSE passthrough/format conversion
- **Fallback routing**: automatic failover to secondary provider on upstream failures
- **Token and latency observability**: request-level metrics in logs and stats

### Security

- **Encrypted API key storage**: AES-256-GCM at rest
- **Admin and proxy auth**: independent bearer token controls
- **Safer defaults for network exposure**: non-loopback binding requires auth keys

### Management UI

- **Provider management**: create / edit / delete providers
- **Route management**: priority matching, model override, fallback
- **Logs and stats**: persistent logs with charted usage insights
- **Desktop ergonomics**: tray menu, optional auto-start, in-app updater

---

## Installation

### Desktop App

Download the latest installer for your platform from [Releases](https://github.com/shuaijinchao/nyro/releases/latest):

| Platform | File |
|---|---|
| macOS (Apple Silicon) | `Nyro_*_aarch64.dmg` |
| macOS (Intel) | `Nyro_*_x86_64.dmg` |
| Windows | `Nyro_*_x64-setup.exe` |
| Linux | `Nyro_*_amd64.AppImage` |

> **macOS / Windows note**: The app is not platform-signed. On macOS, right-click → Open on first launch. On Windows, click "More info" → "Run anyway" in SmartScreen.

### Server Binary

```bash
# Download for your platform
curl -LO https://github.com/shuaijinchao/nyro/releases/latest/download/nyro-server-linux-x86_64

chmod +x nyro-server-linux-x86_64

# Start with defaults (proxy :19530, admin :19531, localhost only)
./nyro-server-linux-x86_64

# Expose to network (requires auth keys)
./nyro-server-linux-x86_64 \
  --proxy-host 0.0.0.0:19530 \
  --admin-host 0.0.0.0:19531 \
  --proxy-key YOUR_PROXY_KEY \
  --admin-key YOUR_ADMIN_KEY
```

Open `http://localhost:19531` in your browser to access the management UI.

---

## Quick Start

1. **Add a Provider** — go to Providers → New, enter your provider's base URL and API key
2. **Add a Route** — go to Routes → New, set a route name, select target provider and model
3. **Point your client** — set `base_url` to `http://127.0.0.1:19530` (and `api_key` to your proxy key if set)
4. **Make requests** — use any OpenAI/Anthropic/Gemini SDK as normal

```python
from openai import OpenAI

client = OpenAI(base_url="http://127.0.0.1:19530/v1", api_key="sk-local-proxy")
response = client.chat.completions.create(
    model="my-route-name",
    messages=[{"role": "user", "content": "Hello"}]
)
```

---

## Build from Source

**Prerequisites**: Rust stable, Node.js 20+, pnpm 9+

```bash
git clone https://github.com/shuaijinchao/nyro.git
cd nyro

# Desktop app (dev mode)
make dev

# Desktop app (release build)
make build

# Server binary only
make server

# Run checks + smoke tests
make release-check
```

---

## License

```
Copyright 2026 Shuaijinchao

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```