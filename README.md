<p align="center">
  <img width="120" src="docs/images/NYRO-logo.png">
</p>

<h2 align="center">Nyro AI Gateway</h2>

<p align="center">
  Run your AI coding tools on any model, from any provider.<br>
  One gateway. All protocols. No code changes.
</p>

<p align="center">
  <a href="https://github.com/NYRO-WAY/NYRO/releases/latest"><img src="https://img.shields.io/github/v/release/NYRO-WAY/NYRO" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-Apache%202.0-blue.svg" alt="License"></a>
  <a href="README_CN.md"><img src="https://img.shields.io/badge/文档-中文-8A2BE2" alt="中文"></a>
</p>

---

<p align="center">
  <img src="docs/images/NYRO-ui-home-en.png" width="800">
</p>

---

## What is Nyro?

Nyro is a local AI gateway that sits between your AI tools and model providers. It translates protocol formats on the fly — so Claude Code, Codex CLI, Gemini CLI, OpenCode, and any client using OpenAI / Anthropic / Gemini SDKs can all route through any backend model you choose, without changing a single line of code.

Point your clients at `http://localhost:19530`. Nyro handles the rest.

```
Claude Code · Codex CLI · Gemini CLI · OpenCode
     OpenAI SDK · Anthropic SDK · Gemini SDK
              Any HTTP API Client
                      ↓
              Nyro AI Gateway
            (localhost:19530)
                      ↓
    OpenAI · Anthropic · Google · DeepSeek
    MiniMax · xAI · Zhipu · Ollama · ...
```

Nyro ships as a **desktop app** (macOS / Windows / Linux) and a **standalone server binary** for headless and self-hosted deployments.

---

## Why Nyro?

**Use any model with any tool.** Claude Code expects Anthropic protocol. Codex CLI uses OpenAI Responses API. Gemini CLI speaks Gemini. Nyro translates between all three so one model can serve all your tools simultaneously.

**Switch providers without touching your tools.** Change the target model or provider from Nyro's UI. Your tools never need reconfiguring.

**Keep everything local.** API keys are encrypted at rest with AES-256-GCM. Requests stay on your machine. No cloud relay, no shared infrastructure.

**One UI for everything.** Manage providers, routes, API keys, logs, and usage stats from a single interface — desktop app or browser.

---

## Screenshots

## Screenshots

<table>
  <tr>
    <td align="center" width="50%"><img src="docs/images/NYRO-ui-providers-add-en.png" height="260"><br><sub>Provider Management</sub></td>
    <td align="center" width="50%"><img src="docs/images/NYRO-ui-routes-en.png" height="260"><br><sub>Route Configuration</sub></td>
  </tr>
  <tr>
    <td align="center" width="50%"><img src="docs/images/NYRO-ui-apikeys-en.png" height="260"><br><sub>API Key Management</sub></td>
    <td align="center" width="50%"><img src="docs/images/NYRO-ui-connect-en.png" height="260"><br><sub>Connect — Code & CLI Integration</sub></td>
  </tr>
</table>

---

## Features

### Protocol Translation

- **Ingress**: OpenAI (Chat Completions + Responses API), Anthropic Messages, Gemini GenerateContent
- **Egress**: route to any OpenAI-compatible, Anthropic, or Gemini upstream
- **Streaming**: full SSE passthrough and cross-protocol format conversion
- **Reasoning**: `<think>` tag parsing and conversion across protocol boundaries
- **Tool calls**: cross-protocol tool call and result format normalization

### Routing

- Exact match routing on `virtual_model`
- Virtual model names decouple client requests from actual backend models
- Fallback routing with automatic failover on upstream errors
- Per-route access control with API key authorization

### Security

- AES-256-GCM encrypted API key storage
- Independent proxy and admin bearer token controls
- Default-deny route authorization — keys must be explicitly bound to routes
- Per-key quotas: RPM / TPM / TPD / RPD

### Management

- Full CRUD for providers, routes, and API keys
- Request logs with provider, model, token, and latency detail
- Usage charts by model and provider
- Provider connectivity testing with live feedback

### Connect — Integration

**Code Integration** — select a route and copy ready-to-use examples for:

| Protocol | Languages |
|---------|---------|
| OpenAI | Python · TypeScript · cURL |
| Anthropic | Python · TypeScript · cURL |
| Gemini | Python · TypeScript · cURL |

**CLI Integration** — one-click config sync for AI coding tools:

| Tool | Protocol |
|------|---------|
| Claude Code | Anthropic |
| Codex CLI | OpenAI Responses API |
| Gemini CLI | Gemini |
| OpenCode | OpenAI |

Nyro detects installed tools, generates the correct configuration for the selected route, and writes it with one click. Original configs are backed up automatically.

### Deployment

**Desktop App**

| Platform | Architecture |
|---|---|
| macOS | Apple Silicon (aarch64) · Intel (x64) |
| Windows | x64 · ARM64 |
| Linux | x86\_64 · aarch64 |

**Server Binary**

| Platform | Architecture | Access |
|---|---|---|
| macOS | x86\_64 · aarch64 | Proxy `:19530` · WebUI `http://localhost:19531` |
| Linux | x86\_64 · aarch64 | Proxy `:19530` · WebUI `http://localhost:19531` |
| Windows | x64 · ARM64 | Proxy `:19530` · WebUI `http://localhost:19531` |

---

## Installation

### Desktop App

**Homebrew (macOS / Linux)**

```bash
brew tap nyro-way/nyro
brew install --cask nyro
```

**Shell Script**

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/NYRO-WAY/NYRO/master/scripts/install/install.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/NYRO-WAY/NYRO/master/scripts/install/install.ps1 | iex
```

**Manual Download**

Download the latest installer for your platform from [GitHub Releases](https://github.com/NYRO-WAY/NYRO/releases/latest).

> **macOS**: After manual install run `sudo xattr -rd com.apple.quarantine /Applications/Nyro.app`, or use the install script which handles this automatically.
>
> **Windows**: SmartScreen may show "Unknown publisher" — click **More info → Run anyway**.

### Server Binary

```bash
# Download
curl -LO https://github.com/NYRO-WAY/NYRO/releases/latest/download/nyro-server-linux-x86_64
chmod +x nyro-server-linux-x86_64

# Start (localhost only, no auth required)
./nyro-server-linux-x86_64

# Start (network-exposed, auth required)
./nyro-server-linux-x86_64 \
  --proxy-host 0.0.0.0:19530 \
  --admin-host 0.0.0.0:19531 \
  --proxy-key YOUR_PROXY_KEY \
  --admin-key YOUR_ADMIN_KEY
```

Available server binaries: `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64.exe`, `windows-arm64.exe`

Open `http://localhost:19531` for the management UI.

### Additional Storage Backend Configuration

Default server behavior remains unchanged: if you do not pass storage-related options, Nyro continues to use local SQLite under `--data-dir`.

For `postgres`, the server binary also supports selecting a storage backend at startup. To avoid exposing credentials in process arguments, provide the DSN through an environment variable and reference it via `--storage-dsn-env`.

```bash
# PostgreSQL
export NYRO_STORAGE_DSN='postgresql://user:pass@host:5432/db'
./nyro-server-linux-x86_64 \
  --storage-backend postgres
```

Additional storage-related server options:

- `--storage-backend sqlite|postgres`
- `--storage-dsn-env` (defaults to `NYRO_STORAGE_DSN`)
- `--sqlite-migrate-on-start true|false`
- `--storage-max-connections`
- `--storage-min-connections`
- `--storage-acquire-timeout-secs`
- `--storage-idle-timeout-secs`
- `--storage-max-lifetime-secs`

### Docker

Nyro's Docker support is split into two separate use cases:

- `docker/runtime/Dockerfile`: production-style server distribution image (`nyro-server` + `webui/dist`)
- `docker/dev/Dockerfile`: development container image for contributors

Build and run the server distribution image:

```bash
docker build -f docker/runtime/Dockerfile -t nyro:runtime .

docker run --rm \
  -e NYRO_ADMIN_KEY=change-me \
  -p 19530:19530 \
  -p 19531:19531 \
  -v nyro-data:/var/lib/nyro \
  nyro:runtime
```

Open `http://127.0.0.1:19531` for the management UI.

Use the same `NYRO_ADMIN_KEY` value as the Bearer token for admin API requests.

Override base images with `--build-arg` if you need to use a different registry or a mirrored base image:

```bash
docker build \
  --build-arg RUST_IMAGE=<custom-rust-image> \
  --build-arg NODE_IMAGE=<custom-node-image> \
  --build-arg RUNTIME_IMAGE=<custom-runtime-image> \
  -f docker/runtime/Dockerfile \
  -t nyro:runtime .
```

Use the development container directly:

```bash
docker build -f docker/dev/Dockerfile -t nyro:dev .

docker run --rm -it \
  -v "$(pwd)":/workspace/nyro \
  -w /workspace/nyro \
  -p 5173:5173 \
  -p 19530:19530 \
  -p 19531:19531 \
  nyro:dev
```

VS Code / Cursor users can also open the repository with `.devcontainer/devcontainer.json`.

---

## Quick Start

**1. Add a Provider**

Go to **Providers → New**. Enter your provider's Base URL and API key. Nyro auto-detects the protocol from the URL.

**2. Create a Route**

Go to **Routes → New**. Set a virtual model name (e.g. `gpt-4o`), select your provider and target model. Enable access control if needed.

**3. Point your client at Nyro**

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://127.0.0.1:19530/v1",
    api_key="your-proxy-key"  # or "no-auth" if access control is off
)

response = client.chat.completions.create(
    model="gpt-4o",  # matches your virtual model name
    messages=[{"role": "user", "content": "Hello"}]
)
```

**4. Sync your AI tools (optional)**

Go to **Connect**, select a route, and click **Sync** next to Claude Code, Codex, Gemini CLI, or OpenCode. Nyro writes the correct config automatically.

---

## License

```
Copyright 2026 The Nyro Authors

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

See the full license text in [LICENSE](LICENSE).
