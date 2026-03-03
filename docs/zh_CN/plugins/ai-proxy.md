# ai-proxy 插件

AI 协议转换插件。在 Nyro 的 `routes → services → backends` 资源模型内，实现 LLM 请求/响应的跨协议转换，支持流式 (SSE) 和非流式场景。

---

## 工作原理

```
客户端 (OpenAI/Anthropic/Gemini 格式)
    │
    ▼ http_access
    ai-proxy: 转换请求体 + 注入认证头 + 改写上游路径
    │
    ▼ balancer → proxy_pass
    上游 Provider
    │
    ▼ http_header_filter
    ai-proxy: 设置 SSE 响应头 + 清除 Content-Length
    │
    ▼ http_body_filter
    ai-proxy: 转换响应体 (非流式整体 / 流式逐行)
    │
    ▼
客户端收到原始协议格式的响应
```

插件 **不短路请求流**，转换后交由 Nyro 标准 balancer + proxy_pass 处理。

---

## 启用

在全局 `plugins` 中注册:

```yaml
plugins:
  - ai-proxy
```

在路由中挂载:

```yaml
routes:
  - name: chat
    service: openai
    paths:
      - /v1/chat/completions
    plugins:
      - id: ai-proxy
        config:
          api_key: "sk-xxxxx"
```

---

## 配置参数

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `api_key` | string | 否 | — | Provider API Key。backend `endpoint.headers` 可替代 |
| `to` | string | 否 | — | 上游协议。不填从 `service.provider` 自动推导 |
| `from` | string | 否 | — | 客户端协议。不填按请求 path 自动推断 |
| `model` | string | 否 | — | 覆盖请求中的 model 字段 |
| `max_tokens` | number | 否 | — | 覆盖最大 token 数 |
| `temperature` | number | 否 | — | 覆盖温度 (0~2) 

所有参数均可选。最简配置只需 `api_key`:

```yaml
- id: ai-proxy
  config:
    api_key: "sk-xxxxx"
```

---

## 协议名

`from` / `to` 支持三种写法:

### 短名 (推荐)

直接使用 provider 名，自动映射到默认能力:

| 短名 | 等价于 |
|------|--------|
| `openai` | OpenAI Chat Completions |
| `anthropic` | Anthropic Messages |
| `gemini` | Gemini Chat |
| `ollama` | Ollama Chat |

### 点号展开

指定具体能力:

| 名称 | 说明 |
|------|------|
| `openai.chat` | OpenAI Chat Completions |
| `openai.responses` | OpenAI Responses API |
| `anthropic.messages` | Anthropic Messages |
| `anthropic.code` | Claude Code |
| `gemini.chat` | Gemini Chat |
| `ollama.chat` | Ollama Chat |

### 内部名 (向后兼容)

`openai_chat`、`anthropic_messages`、`claude_code` 等 llm-converter 原始标识依然可用。

---

## from 自动推断

未配置 `from` 时，按优先级自动推断 (path → auth header → default):

**1) Path 匹配 (精确 / 前缀)**

| 请求 Path | 推断协议 |
|-----------|---------|
| `/v1/chat/completions` | `openai` |
| `/v1/responses` | `openai.responses` |
| `/v1/messages` | `anthropic` |
| `/v1beta/models/...` | `gemini` |

**2) Auth Header 辅助推断**

当 path 无法匹配时，检查请求中的认证头:

| Header | 推断协议 |
|--------|---------|
| `x-goog-api-key` | `gemini` |
| `x-api-key` | `anthropic` |

**3) 默认**: 以上均不匹配时默认为 `openai`。

---

## to 推导

`to` 优先级:

1. 插件配置 `config.to` (最高)
2. `service.provider` 自动推导
3. 都没有 → 插件跳过 (视为非 AI 服务)

| `service.provider` | 推导协议 |
|--------------------|---------|
| `openai` | `openai` |
| `anthropic` | `anthropic` |
| `gemini` | `gemini` |

---

## 认证注入

所有 Provider 统一通过请求头注入认证信息:

| Provider | Header |
|----------|--------|
| OpenAI | `Authorization: Bearer {key}` |
| Anthropic | `x-api-key: {key}` + `anthropic-version: 2023-06-01` |
| Gemini | `x-goog-api-key: {key}` |

### api_key 来源优先级

1. **endpoint.headers** (backend 节点级) — 最高优先，适用于多 Key 轮换
2. **config.api_key** (插件配置) — 适用于单 Key 直连
3. 都没有 → 不注入认证头 (上游可能返回 401)

---

## 上游路径

默认根据 `to` 自动设置:

| 协议 | 默认路径 |
|------|---------|
| `openai` | `/v1/chat/completions` |
| `openai.responses` | `/v1/responses` |
| `anthropic` | `/v1/messages` |
| `gemini` (非流式) | `/v1beta/models/{model}:generateContent` |
| `gemini` (流式) | `/v1beta/models/{model}:streamGenerateContent?alt=sse` |
| `ollama` | `/api/chat` |

配置 `upstream_path` 可覆盖默认值。

---

## 流式处理

当请求体中 `stream: true` 时:

- **请求**: Gemini 上游路径自动切换为 `streamGenerateContent?alt=sse`
- **请求**: OpenAI 上游自动注入 `stream_options.include_usage: true`，强制返回 token 统计
- **响应头**: 设置 `Content-Type: text/event-stream`、`X-Accel-Buffering: no`
- **响应体**: 逐行扫描 SSE `data:` 行，对每个 JSON payload 单独转换

非流式时缓冲完整响应体后一次性转换。

---

## Token 统计

ai-proxy 在 `http_body_filter` 阶段自动从上游响应中提取 token 用量，按上游协议 (`target_proto`) 匹配对应字段：

| 上游协议 | prompt 字段 | completion 字段 |
|---|---|---|
| openai_chat | `usage.prompt_tokens` | `usage.completion_tokens` |
| openai_responses | `usage.input_tokens` | `usage.output_tokens` |
| anthropic_messages | `usage.input_tokens` | `usage.output_tokens` |
| gemini_chat | `usageMetadata.promptTokenCount` | `usageMetadata.candidatesTokenCount` |
| ollama_chat | `prompt_eval_count` | `eval_count` |

提取后存入 `oak_ctx._ai_proxy`，供下游插件使用：

| 字段 | 说明 |
|------|------|
| `model` | 请求使用的模型名称（来自请求体或插件配置 `config.model`） |
| `input_tokens` | 输入 Token 数（上游 `usage` 权威值） |
| `output_tokens` | 输出 Token 数（上游 `usage` 权威值） |

- **local-metrics**: 按 route / service / consumer / model 维度聚合 token 计数和请求量
- **local-logs**: 写入 `$nyro_model` / `$nyro_input_tokens` / `$nyro_output_tokens`，输出到 `logs/access.json`

Token 数据来源为上游厂商返回的权威值，不做本地估算。非 AI 请求 token 字段为 0、model 为空。

---

## 配置示例

### 最简 — OpenAI 直连

```yaml
services:
  - name: openai
    provider: openai
    url: https://api.openai.com

routes:
  - name: chat
    service: openai
    paths: [/v1/chat/completions]
    methods: ["POST"]
    plugins:
      - id: ai-proxy
        config:
          api_key: "sk-xxxxx"
```

### 协议转换 — OpenAI 客户端 → Anthropic 后端

```yaml
services:
  - name: claude
    provider: anthropic
    url: https://api.anthropic.com

routes:
  - name: chat
    service: claude
    paths: [/v1/chat/completions]
    methods: ["POST"]
    plugins:
      - id: ai-proxy
        config:
          api_key: "sk-ant-xxxxx"
          model: "claude-sonnet-4-20250514"
```

### 多 Key 轮换 (无需 api_key)

```yaml
backends:
  - name: openai-keys
    algorithm: roundrobin
    endpoints:
      - address: "api.openai.com"
        port: 443
        headers:
          Authorization: "Bearer sk-A-xxxxx"
      - address: "api.openai.com"
        port: 443
        headers:
          Authorization: "Bearer sk-B-xxxxx"

services:
  - name: openai-ha
    provider: openai
    scheme: https
    backend: openai-keys

routes:
  - name: chat
    service: openai-ha
    paths: [/v1/chat/completions]
    methods: ["POST"]
    plugins:
      - id: ai-proxy
        config: {}
```

### 自定义 path + 显式 from

```yaml
routes:
  - name: gemini-chat
    service: gemini
    paths: [/ai/gemini/chat]
    methods: ["POST"]
    plugins:
      - id: ai-proxy
        config:
          api_key: "AIzaSy-xxxxx"
          model: "gemini-2.0-flash"
          from: openai
```

---

## 与其他插件配合

ai-proxy 只负责协议转换，可与其他插件组合:

```yaml
plugins:
  - id: key-auth           # 1. 先验证客户端身份
  - id: limit-req          # 2. 再限流
    config:
      rate: 20
      burst: 10
  - id: ai-proxy           # 3. 最后做协议转换
    config:
      api_key: "sk-xxxxx"
```

插件按数组顺序执行。建议认证 → 限流 → ai-proxy。

---

## 更多示例

完整配置场景参见 [AI 网关配置示例](../examples/ai-gateway.md)。
