<p align="center">
  <img width="120" src="docs/images/NYRO-logo.png">
</p>

<h2 align="center">Nyro AI Gateway</h2>

<p align="center">
  让你的 AI 编码工具运行在任意模型、任意提供商之上。<br>
  一个网关，兼容所有协议，无需改代码。
</p>

<p align="center">
  <a href="https://github.com/NYRO-WAY/NYRO/releases/latest"><img src="https://img.shields.io/github/v/release/NYRO-WAY/NYRO" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-Apache%202.0-blue.svg" alt="License"></a>
  <a href="README.md"><img src="https://img.shields.io/badge/Language-English-2d7ff9" alt="English"></a>
</p>

---

<p align="center">
  <img src="docs/images/NYRO-ui-home-cn.png" width="800">
</p>

---

## Nyro 是什么？

Nyro 是一个本地 AI 网关，位于你的 AI 工具与模型提供商之间。它可实时转换协议格式，因此 Claude Code、Codex CLI、Gemini CLI、OpenCode，以及任何使用 OpenAI / Anthropic / Gemini SDK 的客户端，都可以在不改一行代码的前提下，路由到你指定的任意后端模型。

将客户端指向 `http://localhost:19530`，其余交给 Nyro。

```
Claude Code · Codex CLI · Gemini CLI · OpenCode
     OpenAI SDK · Anthropic SDK · Gemini SDK
              任意 HTTP API 客户端
                      ↓
              Nyro AI Gateway
            (localhost:19530)
                      ↓
   OpenAI · Anthropic · Google · DeepSeek
    MiniMax · xAI · Zhipu · Ollama · ...
```

Nyro 同时提供 **桌面应用**（macOS / Windows / Linux）和 **独立服务端二进制**，适用于无头部署与自托管场景。

---

## 为什么选择 Nyro？

**任意工具配任意模型。** Claude Code 使用 Anthropic 协议，Codex CLI 使用 OpenAI Responses API，Gemini CLI 使用 Gemini 协议。Nyro 可以在三者之间相互转换，让同一个模型同时服务所有工具。

**切换提供商不改工具配置。** 在 Nyro UI 中更换目标模型或提供商即可，工具侧配置保持不变。

**全程本地。** API Key 使用 AES-256-GCM 静态加密存储。请求仅在你的设备上流转，无云中转、无共享基础设施。

**一个 UI 管全部。** 在同一界面管理提供商、路由、API Key、日志与用量统计，可在桌面应用或浏览器访问。

---

## 界面截图

<table>
  <tr>
    <td align="center" width="50%"><img src="docs/images/NYRO-ui-providers-add-cn.png" height="260"><br><sub>Provider 管理</sub></td>
    <td align="center" width="50%"><img src="docs/images/NYRO-ui-routes-cn.png" height="260"><br><sub>路由配置</sub></td>
  </tr>
  <tr>
    <td align="center" width="50%"><img src="docs/images/NYRO-ui-apikeys-cn.png" height="260"><br><sub>API Key 管理</sub></td>
    <td align="center" width="50%"><img src="docs/images/NYRO-ui-connect-cn.png" height="260"><br><sub>Connect：代码与 CLI 集成</sub></td>
  </tr>
</table>

---

## 功能特性

### 协议转换

- **入口协议**：OpenAI（Chat Completions + Responses API）、Anthropic Messages、Gemini GenerateContent
- **出口协议**：可路由到任意 OpenAI 兼容、Anthropic 或 Gemini 上游
- **流式响应**：完整 SSE 透传与跨协议格式转换
- **推理内容**：支持 `<think>` 标签解析与跨协议转换
- **工具调用**：跨协议 tool call 与结果格式归一化

### 路由能力

- 基于 `virtual_model` 的精确匹配路由
- 通过虚拟模型名解耦客户端请求与真实后端模型
- 上游异常时自动触发 Fallback 失败转移
- 按路由进行 API Key 访问控制

### 安全能力

- AES-256-GCM 加密存储 API Key
- 代理层与管理层 Bearer Token 独立控制
- 默认拒绝策略：API Key 必须显式绑定路由才可访问
- 单 Key 配额：RPM / TPM / TPD / RPD

### 管理能力

- Provider、路由、API Key 的完整 CRUD
- 请求日志记录 Provider、模型、Token、延迟等信息
- 按模型与提供商的用量统计图表
- Provider 连通性测试与实时反馈

### Connect — 集成

**代码集成**：选择路由后一键复制可直接使用的示例：

| 协议 | 语言 |
|---------|---------|
| OpenAI | Python · TypeScript · cURL |
| Anthropic | Python · TypeScript · cURL |
| Gemini | Python · TypeScript · cURL |

**CLI 集成**：一键同步 AI 编码工具配置：

| 工具 | 协议 |
|------|---------|
| Claude Code | Anthropic |
| Codex CLI | OpenAI Responses API |
| Gemini CLI | Gemini |
| OpenCode | OpenAI |

Nyro 会自动检测已安装工具，为所选路由生成正确配置并一键写入，同时自动备份原始配置。

### 部署形态

**桌面应用**

| 平台 | 架构 |
|---|---|
| macOS | Apple Silicon (aarch64) · Intel (x64) |
| Windows | x64 · ARM64 |
| Linux | x86_64 · aarch64 |

**服务端二进制**

| 平台 | 架构 | 访问 |
|---|---|---|
| macOS | x86_64 · aarch64 | Proxy `:19530` · WebUI `http://localhost:19531` |
| Linux | x86_64 · aarch64 | Proxy `:19530` · WebUI `http://localhost:19531` |
| Windows | x64 · ARM64 | Proxy `:19530` · WebUI `http://localhost:19531` |

---

## 安装

### 桌面应用

**Homebrew（macOS / Linux）**

```bash
brew tap nyro-way/nyro
brew install --cask nyro
```

**Shell 脚本**

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/NYRO-WAY/NYRO/master/scripts/install/install.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/NYRO-WAY/NYRO/master/scripts/install/install.ps1 | iex
```

**手动下载**

从 [GitHub Releases](https://github.com/NYRO-WAY/NYRO/releases/latest) 下载你平台对应的最新安装包。

> **macOS**：手动安装后请运行 `sudo xattr -rd com.apple.quarantine /Applications/Nyro.app`，或使用安装脚本自动处理。
>
> **Windows**：若 SmartScreen 提示 "Unknown publisher"，点击 **More info → Run anyway**。

### 服务端二进制

```bash
# 下载
curl -LO https://github.com/NYRO-WAY/NYRO/releases/latest/download/nyro-server-linux-x86_64
chmod +x nyro-server-linux-x86_64

# 启动（仅 localhost，无需鉴权）
./nyro-server-linux-x86_64

# 启动（暴露到网络，必须鉴权）
./nyro-server-linux-x86_64 \
  --proxy-host 0.0.0.0:19530 \
  --admin-host 0.0.0.0:19531 \
  --proxy-key YOUR_PROXY_KEY \
  --admin-key YOUR_ADMIN_KEY
```

可用服务端二进制：`linux-x86_64`、`linux-aarch64`、`macos-x86_64`、`macos-aarch64`、`windows-x86_64.exe`、`windows-arm64.exe`

打开 `http://localhost:19531` 进入管理界面。

### 补充：其他存储后端配置

服务端默认行为保持不变：如果不传存储相关参数，Nyro 仍然使用 `--data-dir` 下的本地 SQLite。

对于 `postgres`，服务端二进制也支持在启动时切换后端。为了避免把凭据暴露在进程参数中，建议通过环境变量提供 DSN，再用 `--storage-dsn-env` 引用。

```bash
# PostgreSQL
export NYRO_STORAGE_DSN='postgresql://user:pass@host:5432/db'
./nyro-server-linux-x86_64 \
  --storage-backend postgres
```

服务端新增的存储相关参数：

- `--storage-backend sqlite|postgres`
- `--storage-dsn-env`，默认 `NYRO_STORAGE_DSN`
- `--sqlite-migrate-on-start true|false`
- `--storage-max-connections`
- `--storage-min-connections`
- `--storage-acquire-timeout-secs`
- `--storage-idle-timeout-secs`
- `--storage-max-lifetime-secs`

### Docker

Nyro 的 Docker 方案拆成两类用途：

- `docker/runtime/Dockerfile`：面向分发的服务端镜像（`nyro-server` + `webui/dist`）
- `docker/dev/Dockerfile`：面向贡献者的开发容器

构建并运行服务端分发镜像：

```bash
docker build -f docker/runtime/Dockerfile -t nyro:runtime .

docker run --rm \
  -e NYRO_ADMIN_KEY=change-me \
  -p 19530:19530 \
  -p 19531:19531 \
  -v nyro-data:/var/lib/nyro \
  nyro:runtime
```

打开 `http://127.0.0.1:19531` 进入管理界面。

管理 API 请求时，请使用同一个 `NYRO_ADMIN_KEY` 作为 Bearer Token。

如果你需要改用其他镜像仓库，或使用自定义基础镜像，可以通过 `--build-arg` 覆盖：

```bash
docker build \
  --build-arg RUST_IMAGE=<custom-rust-image> \
  --build-arg NODE_IMAGE=<custom-node-image> \
  --build-arg RUNTIME_IMAGE=<custom-runtime-image> \
  -f docker/runtime/Dockerfile \
  -t nyro:runtime .
```

直接使用开发容器：

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

使用 VS Code / Cursor 时，也可以直接打开 `.devcontainer/devcontainer.json`。

---

## 快速开始

**1. 添加 Provider**

进入 **Providers → New**，填写 Provider 的 Base URL 和 API Key。Nyro 会根据 URL 自动识别协议。

**2. 创建路由**

进入 **Routes → New**，设置虚拟模型名（例如 `gpt-4o`），选择 Provider 与目标模型。需要时可开启访问控制。

**3. 将客户端指向 Nyro**

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://127.0.0.1:19530/v1",
    api_key="your-proxy-key"  # 若关闭访问控制可用 "no-auth"
)

response = client.chat.completions.create(
    model="gpt-4o",  # 对应你配置的虚拟模型名
    messages=[{"role": "user", "content": "Hello"}]
)
```

**4. 同步你的 AI 工具（可选）**

进入 **Connect**，选择路由后点击 Claude Code、Codex、Gemini CLI 或 OpenCode 旁边的 **Sync**。Nyro 会自动写入正确配置。

---

## 开源协议

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

完整协议见 [LICENSE](LICENSE)。
