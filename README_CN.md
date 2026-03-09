<p align="center">
  <img width="120" src="docs/images/NYRO-logo.png">
</p>

<p align="center">
  <strong>Nyro AI Gateway</strong>
</p>
<p align="center">
  本地优先 AI 协议网关，支持 OpenAI / Anthropic / Gemini。<br>
  保留现有 SDK，通过配置完成 Provider 切换。
</p>

<p align="center">
  <a href="https://github.com/shuaijinchao/nyro/releases/latest"><img src="https://img.shields.io/github/v/release/shuaijinchao/nyro" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-Apache%202.0-blue.svg" alt="License"></a>
  <a href="README.md"><img src="https://img.shields.io/badge/Language-English-2d7ff9" alt="English"></a>
</p>

---

## 简介

Nyro 是一个运行在本地的 AI 协议网关，可在 OpenAI、Anthropic、Gemini 协议之间进行转换，让一个客户端 SDK 连接多个上游提供商。

Nyro 提供两种形态：

- **桌面应用**：基于 Tauri，支持 macOS / Windows / Linux
- **服务端二进制**：独立 HTTP 服务，可通过 WebUI 管理

### 为什么选择 Nyro？

| 原则 | 说明 |
|---|---|
| **本地优先** | 配置和请求由你掌控 |
| **协议中立** | 任意支持的入口协议可路由到任意支持的上游协议 |
| **运维简单** | 在一个 UI 中管理 Provider、路由、日志和统计 |
| **可发布可运营** | 内置鉴权、密钥加密、Fallback、自动发布流程 |

### 快速概览

- **入口协议**：OpenAI、Anthropic、Gemini
- **响应模式**：非流式 + SSE 流式
- **存储**：SQLite（`sqlx`）
- **运行时**：Rust（`axum`、`tokio`、`reqwest`）
- **前端栈**：React 19 + TypeScript + Vite
- **桌面壳**：Tauri v2（托盘、自启、自动更新）

---

## 核心能力

### 网关能力

- **多协议入口**：OpenAI / Anthropic / Gemini
- **任意 Provider 出口**：支持 OpenAI 兼容、Anthropic、Gemini 上游
- **流式支持**：SSE 全链路转换与透传
- **Fallback 路由**：上游失败时自动降级到备用 Provider
- **可观测性**：日志中记录延迟、状态、Token 等关键指标

### 安全能力

- **API Key 加密存储**：AES-256-GCM 静态加密
- **双层鉴权控制**：代理层和管理层独立 Bearer Token
- **默认安全策略**：非本地绑定必须配置鉴权密钥

### 管理能力

- **Provider 管理**：新增 / 编辑 / 删除
- **路由管理**：优先级匹配、模型覆盖、Fallback
- **日志与统计**：持久化日志 + 可视化统计看板
- **桌面体验**：托盘菜单、可选开机自启、应用内更新

---

## 安装

### 桌面应用

从 [Releases](https://github.com/shuaijinchao/nyro/releases/latest) 下载对应平台的安装包：

| 平台 | 文件 |
|---|---|
| macOS（Apple Silicon） | `Nyro_*_aarch64.dmg` |
| macOS（Intel） | `Nyro_*_x86_64.dmg` |
| Windows | `Nyro_*_x64-setup.exe` |
| Linux | `Nyro_*_amd64.AppImage` |

> **macOS / Windows 提示**：应用未进行平台签名/公证。macOS 首次打开请右键 → 打开；Windows 在 SmartScreen 提示中点击「更多信息」→「仍要运行」。

### 服务端二进制

```bash
# 下载对应平台的二进制
curl -LO https://github.com/shuaijinchao/nyro/releases/latest/download/nyro-server-linux-x86_64

chmod +x nyro-server-linux-x86_64

# 默认启动（代理 :19530，管理 :19531，仅本地访问）
./nyro-server-linux-x86_64

# 暴露到网络（必须配置鉴权 Key）
./nyro-server-linux-x86_64 \
  --proxy-host 0.0.0.0:19530 \
  --admin-host 0.0.0.0:19531 \
  --proxy-key YOUR_PROXY_KEY \
  --admin-key YOUR_ADMIN_KEY
```

打开浏览器访问 `http://localhost:19531` 进入管理界面。

---

## 快速上手

1. **添加 Provider** — 进入 Providers → 新建，填写 Provider 的 base URL 和 API Key
2. **添加路由** — 进入 Routes → 新建，填写路由名称，选择目标 Provider 和模型
3. **配置客户端** — 将 `base_url` 设置为 `http://127.0.0.1:19530`（如设置了 proxy key，同步配置 `api_key`）
4. **正常使用** — 使用任意 OpenAI / Anthropic / Gemini SDK 发送请求

```python
from openai import OpenAI

client = OpenAI(base_url="http://127.0.0.1:19530/v1", api_key="sk-local-proxy")
response = client.chat.completions.create(
    model="my-route-name",
    messages=[{"role": "user", "content": "你好"}]
)
```

---

## 从源码构建

**依赖**：Rust stable、Node.js 20+、pnpm 9+

```bash
git clone https://github.com/shuaijinchao/nyro.git
cd nyro

# 桌面应用（开发模式）
make dev

# 桌面应用（发布构建）
make build

# 仅构建服务端二进制
make server

# 代码检查 + 冒烟测试
make release-check
```

---

## 开源协议

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
