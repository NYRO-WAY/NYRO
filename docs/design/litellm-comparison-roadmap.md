# Nyro vs LiteLLM 对标分析与优化路线图

**Nyro Gateway · 架构设计文档**
**日期：2026-03-24**

---

## 1. 背景

本文基于对 [LiteLLM](https://github.com/BerriAI/litellm)（Python，120+ Provider，40k+ Stars）源码的深度分析，系统梳理 Nyro 在协议处理、路由容错、可观测性等方面的差距，并给出带优先级的优化路线图与核心实现方案。

> 参考对标版本：LiteLLM main 分支（2026-03-23）。

---

## 2. 架构模式对比

LiteLLM 支持三种请求处理模式：

| 模式 | 端点示例 | 行为 | Nyro 支持 |
|------|---------|------|-----------|
| **桥接 (Bridge)** | `/v1/chat/completions` | 入口 OpenAI 格式 → 内部转换 → 各 Provider 原生格式 → 响应转回 OpenAI | ✅ 已有 |
| **混合 (Hybrid)** | `/v1/messages` | 入口 Anthropic 格式 → 内部转换 → 各 Provider → 响应转回 Anthropic | ✅ 已有（与桥接共享管线） |
| **直通 (Passthrough)** | `/anthropic/*`, `/gemini/*` | 零转换，原始字节流转发，仅替换鉴权 | ❌ 缺失 |

Nyro 当前所有请求均走 `decode → InternalRequest → encode` 全链路，即使入口出口同协议也无法跳过转换。

---

## 3. 优化项总览（按优先级）

| 优先级 | 编号 | 优化项 | 影响范围 | 预估工作量 | 状态 |
|--------|------|--------|----------|-----------|------|
| **P0** | 3.1 | 参数声明与安全阀 | protocol/, proxy/ | S | |
| **P0** | 3.2 | 结构化错误映射 | proxy/handler.rs | S | |
| **P0** | 3.3 | 重试与 Fallback | proxy/handler.rs, db/models | M | |
| **P1** | 3.4 | ProviderAdapter trait | proxy/ | S | ✅ 已完成 |
| **P1** | 3.5 | 响应 provider_fields | protocol/types.rs | S | |
| **P1** | 3.6 | TokenUsage 扩展 | protocol/types.rs, 各 parser | S | |
| **P1** | 3.7 | 同协议短路优化 | proxy/handler.rs | M | |
| **P2** | 3.8 | 直通 (Passthrough) 模式 | proxy/server.rs, 新模块 | L | |
| **P2** | 3.9 | 可观测性集成 | 新模块 | M | |
| **P2** | 3.10 | 成本追踪 | 新模块 | M | |
| **P2** | 3.11 | Docker 支持 | 根目录 | S | |
| **P3** | 3.12 | 缓存层 | 新模块 | L | |
| **P3** | 3.13 | Guardrails 内容安全 | 新模块 | L | |
| **P3** | 3.14 | A2A / MCP 协议 | 新模块 | L | |

> 工作量：S = 1-2 天，M = 3-5 天，L = 1-2 周

---

## 4. P0：必须优先解决

### 4.1 参数声明与安全阀

**问题**：`InternalRequest.extra` 中的参数在 encoder 中通过 `for (k, v) in &req.extra` 无条件注入出口 body。用户传了 Provider 不支持的参数（如对 Ollama 传 `response_format`），不会报错也不会告知，参数悄悄丢失或导致上游错误。

**LiteLLM 做法**：每个 Provider 有 `get_supported_openai_params()` 声明支持列表，不支持的参数要么 `UnsupportedParamsError` 要么 `drop_params` 静默丢弃并记日志。

**实现方案**：

在 `EgressEncoder` trait 上增加参数声明：

```rust
// protocol/mod.rs
pub trait EgressEncoder {
    /// 该出口协议/Provider 支持的额外参数名
    fn supported_extra_params(&self) -> &[&str] {
        &[] // 默认不支持任何 extra
    }

    fn encode_request(&self, req: &InternalRequest) -> Result<(Value, HeaderMap)>;
    fn egress_path(&self, model: &str, stream: bool) -> String;
}
```

在 `proxy_pipeline` 编码前增加检查：

```rust
// proxy/handler.rs - proxy_pipeline 中，encode 之前
let supported = encoder.supported_extra_params();
let unsupported: Vec<&str> = internal.extra.keys()
    .filter(|k| !supported.contains(&k.as_str()))
    .map(|k| k.as_str())
    .collect();

if !unsupported.is_empty() {
    match gw.config.unsupported_params_policy {
        ParamPolicy::Error => {
            return error_response(400, &format!(
                "provider does not support parameters: {:?}. \
                 Set unsupported_params_policy=drop to silently ignore.",
                unsupported
            ));
        }
        ParamPolicy::Drop => {
            for key in &unsupported {
                tracing::warn!(param = key, "dropping unsupported parameter");
                internal.extra.remove(*key);
            }
        }
        ParamPolicy::Passthrough => { /* 当前行为，不做处理 */ }
    }
}
```

配置枚举：

```rust
// config.rs
#[derive(Debug, Clone, Default)]
pub enum ParamPolicy {
    #[default]
    Error,
    Drop,
    Passthrough,
}
```

各 encoder 实现示例：

```rust
// protocol/openai/encoder.rs
impl EgressEncoder for OpenAIEncoder {
    fn supported_extra_params(&self) -> &[&str] {
        &["response_format", "seed", "logprobs", "top_logprobs",
          "frequency_penalty", "presence_penalty", "stop", "n",
          "reasoning_effort", "user"]
    }
    // ...
}

// protocol/anthropic/encoder.rs
impl EgressEncoder for AnthropicEncoder {
    fn supported_extra_params(&self) -> &[&str] {
        &["thinking", "metadata", "stop_sequences", "top_k"]
    }
    // ...
}
```

---

### 4.2 结构化错误映射

**问题**：所有错误统一返回 `"type": "gateway_error"`，客户端（如 Claude Code）无法区分 429（限流）和 400（参数错误）进行差异化处理。

**LiteLLM 做法**：`exception_mapping_utils.py` 按 Provider + status_code + 错误文本子串映射为 OpenAI SDK 标准异常类型。

**实现方案**：

定义错误类型枚举：

```rust
// proxy/errors.rs（新文件）
#[derive(Debug, Clone, Copy)]
pub enum GatewayErrorKind {
    InvalidRequest,          // 400
    AuthenticationError,     // 401
    PermissionDenied,        // 403
    NotFound,                // 404
    RateLimitExceeded,       // 429
    ContextWindowExceeded,   // 400 (从上游识别)
    ContentFiltered,         // 400 (内容策略)
    UpstreamError,           // 502
    ServiceUnavailable,      // 503
    InternalError,           // 500
}

impl GatewayErrorKind {
    /// OpenAI SDK 兼容的 error type 字符串
    pub fn error_type(&self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request_error",
            Self::AuthenticationError => "authentication_error",
            Self::RateLimitExceeded => "rate_limit_error",
            Self::ContextWindowExceeded => "invalid_request_error",
            Self::ContentFiltered => "content_policy_violation",
            _ => "server_error",
        }
    }

    pub fn status_code(&self) -> u16 {
        match self {
            Self::InvalidRequest | Self::ContextWindowExceeded
            | Self::ContentFiltered => 400,
            Self::AuthenticationError => 401,
            Self::PermissionDenied => 403,
            Self::NotFound => 404,
            Self::RateLimitExceeded => 429,
            Self::UpstreamError => 502,
            Self::ServiceUnavailable => 503,
            Self::InternalError => 500,
        }
    }
}

/// 从上游响应的 status + body 推断错误类型
pub fn classify_upstream_error(status: u16, body: &Value) -> GatewayErrorKind {
    let msg = body
        .pointer("/error/message")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let msg_lower = msg.to_lowercase();

    match status {
        401 => GatewayErrorKind::AuthenticationError,
        403 => GatewayErrorKind::PermissionDenied,
        429 => GatewayErrorKind::RateLimitExceeded,
        503 | 529 => GatewayErrorKind::ServiceUnavailable,
        _ if msg_lower.contains("context length")
            || msg_lower.contains("max tokens")
            || msg_lower.contains("too many tokens") =>
        {
            GatewayErrorKind::ContextWindowExceeded
        }
        _ if msg_lower.contains("content filter")
            || msg_lower.contains("content policy") =>
        {
            GatewayErrorKind::ContentFiltered
        }
        400..=499 => GatewayErrorKind::InvalidRequest,
        _ => GatewayErrorKind::UpstreamError,
    }
}
```

改造 `error_response`：

```rust
fn typed_error_response(kind: GatewayErrorKind, message: &str) -> Response {
    let code = StatusCode::from_u16(kind.status_code())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        code,
        Json(serde_json::json!({
            "error": {
                "message": message,
                "type": kind.error_type(),
                "code": kind.status_code()
            }
        })),
    )
        .into_response()
}
```

在 `handle_non_stream` / `handle_stream` 中，上游返回 4xx/5xx 时调用 `classify_upstream_error`：

```rust
if status >= 400 {
    let kind = classify_upstream_error(status, &resp);
    // ... 日志 ...
    return typed_error_response(kind, &extract_error_message(&resp));
}
```

---

### 4.3 重试与 Fallback

**问题**：上游失败直接返回 502。数据库 schema 有 `fallback_provider`/`fallback_model` 列，但 `Route` 结构体和 handler 未使用。生产环境单点故障不可接受。

**LiteLLM 做法**：`Router` 约 9800 行，含 `function_with_fallbacks`、指数退避重试、多策略负载均衡。

**实现方案（第一阶段：简单重试 + Fallback）**：

扩展 Route 模型：

```rust
// db/models.rs
pub struct Route {
    // ... 现有字段 ...
    pub fallback_provider: Option<String>,
    pub fallback_model: Option<String>,
    pub max_retries: Option<i32>,       // 默认 0 不重试
}
```

在 `proxy_pipeline` 中增加重试逻辑：

```rust
async fn proxy_pipeline(/* ... */) -> Response {
    // ... 路由匹配、鉴权 ...

    let max_retries = route.max_retries.unwrap_or(0).max(0) as usize;

    // 首次尝试 + 重试
    let mut last_error = None;
    for attempt in 0..=max_retries {
        if attempt > 0 {
            let backoff = Duration::from_millis(100 * 2u64.pow(attempt as u32 - 1));
            tokio::time::sleep(backoff).await;
            tracing::warn!(attempt, "retrying upstream request");
        }
        match try_upstream(&gw, &provider, egress, &internal, /* ... */).await {
            Ok(response) => return response,
            Err(e) if e.is_retryable() => { last_error = Some(e); continue; }
            Err(e) => return e.into_response(),
        }
    }

    // 重试用尽，尝试 fallback
    if let (Some(fb_provider_id), Some(fb_model)) =
        (&route.fallback_provider, &route.fallback_model)
    {
        tracing::warn!("primary failed, trying fallback provider");
        if let Ok(fb_provider) = get_provider(&access_store, fb_provider_id).await {
            let fb_egress: Protocol = fb_provider.protocol.parse()
                .unwrap_or(Protocol::OpenAI);
            // ... 用 fb_provider + fb_model 重跑一次 ...
        }
    }

    // 全部失败
    last_error.map(|e| e.into_response())
        .unwrap_or_else(|| error_response(502, "all attempts exhausted"))
}
```

可重试判定：

```rust
impl UpstreamError {
    fn is_retryable(&self) -> bool {
        matches!(self.status, 429 | 500 | 502 | 503 | 408)
    }
}
```

---

## 5. P1：核心体验提升

### 5.1 ProviderAdapter trait

**问题**：Provider 个性化逻辑（鉴权、URL、Ollama 能力检测）散落在 `client.rs` 和 `handler.rs` 的 match/if 分支中，新增 Provider（如 Bedrock SigV4）需要改动多处。

**实现方案**：

新增 `proxy/adapter.rs`，定义 trait + 4 个实现：

```rust
// proxy/adapter.rs
use async_trait::async_trait;

#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    fn auth_headers(&self, api_key: &str) -> HeaderMap;
    fn build_url(&self, base_url: &str, path: &str, api_key: &str) -> String;

    /// 请求发出前的调整（默认无操作）
    async fn pre_request(
        &self,
        _req: &mut InternalRequest,
        _http: &reqwest::Client,
        _provider: &Provider,
    ) {}

    /// 解析上游错误为结构化类型（默认走通用逻辑）
    fn classify_error(&self, status: u16, body: &Value) -> GatewayErrorKind {
        classify_upstream_error(status, body)
    }
}

// ── OpenAI 兼容 (xAI, DeepSeek, Moonshot, Groq...) ──
pub struct OpenAICompatAdapter;

#[async_trait]
impl ProviderAdapter for OpenAICompatAdapter {
    fn auth_headers(&self, api_key: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Ok(v) = HeaderValue::from_str(&format!("Bearer {api_key}")) {
            h.insert("Authorization", v);
        }
        h
    }

    fn build_url(&self, base_url: &str, path: &str, _api_key: &str) -> String {
        let base = base_url.trim_end_matches('/');
        let adjusted = if has_base_path(base) && path.starts_with("/v1/") {
            &path[3..]
        } else {
            path
        };
        format!("{base}{adjusted}")
    }
}

// ── Anthropic ──
pub struct AnthropicAdapter;

#[async_trait]
impl ProviderAdapter for AnthropicAdapter {
    fn auth_headers(&self, api_key: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-api-key", HeaderValue::from_str(api_key).unwrap());
        h.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        h
    }

    fn build_url(&self, base_url: &str, path: &str, _api_key: &str) -> String {
        format!("{}{path}", base_url.trim_end_matches('/'))
    }
}

// ── Gemini ──
pub struct GeminiAdapter;

#[async_trait]
impl ProviderAdapter for GeminiAdapter {
    fn auth_headers(&self, _api_key: &str) -> HeaderMap {
        HeaderMap::new() // Gemini 用 URL 参数
    }

    fn build_url(&self, base_url: &str, path: &str, api_key: &str) -> String {
        let url = format!("{}{path}", base_url.trim_end_matches('/'));
        if url.contains('?') {
            format!("{url}&key={api_key}")
        } else {
            format!("{url}?key={api_key}")
        }
    }
}

// ── Ollama ──
pub struct OllamaAdapter;

#[async_trait]
impl ProviderAdapter for OllamaAdapter {
    fn auth_headers(&self, api_key: &str) -> HeaderMap {
        OpenAICompatAdapter.auth_headers(api_key) // 复用
    }

    fn build_url(&self, base_url: &str, path: &str, api_key: &str) -> String {
        OpenAICompatAdapter.build_url(base_url, path, api_key) // 复用
    }

    async fn pre_request(
        &self,
        req: &mut InternalRequest,
        http: &reqwest::Client,
        provider: &Provider,
    ) {
        // 将 handler.rs 中的 maybe_strip_ollama_tools 逻辑移到这里
        // 检测模型是否支持 tools，不支持则剥离
    }
}

// ── 工厂 ──
pub fn get_adapter(provider: &Provider, egress: Protocol) -> Box<dyn ProviderAdapter> {
    if provider.vendor.as_deref().is_some_and(|v| v.eq_ignore_ascii_case("ollama")) {
        return Box::new(OllamaAdapter);
    }
    match egress {
        Protocol::Anthropic => Box::new(AnthropicAdapter),
        Protocol::Gemini => Box::new(GeminiAdapter),
        _ => Box::new(OpenAICompatAdapter),
    }
}
```

改造后 `client.rs` 简化为：

```rust
pub async fn call_non_stream(
    &self,
    adapter: &dyn ProviderAdapter,
    base_url: &str,
    path: &str,
    api_key: &str,
    body: Value,
    extra_headers: HeaderMap,
) -> Result<(Value, u16)> {
    let url = adapter.build_url(base_url, path, api_key);
    let mut headers = adapter.auth_headers(api_key);
    headers.extend(extra_headers);
    let resp = self.http.post(&url).headers(headers).json(&body).send().await?;
    Ok((resp.json().await?, resp.status().as_u16()))
}
```

---

### 5.2 响应 provider_fields

**问题**：`InternalResponse` 是固定结构，Anthropic 的 `cache_creation_input_tokens`、OpenAI 的 `system_fingerprint` 等字段无法保留。

**实现方案**：

```rust
// protocol/types.rs
#[derive(Debug, Clone, Default)]
pub struct InternalResponse {
    // ... 现有字段 ...
    /// Provider 专有字段（无法映射到标准结构的信息）
    pub provider_fields: HashMap<String, Value>,
}
```

各 ResponseParser 在解析时将额外字段收集到 `provider_fields`：

```rust
// 以 Anthropic 为例
fn parse_response(&self, resp: Value) -> Result<InternalResponse> {
    let mut provider_fields = HashMap::new();
    // 提取 Anthropic 专有字段
    if let Some(v) = resp.pointer("/usage/cache_creation_input_tokens") {
        provider_fields.insert("cache_creation_input_tokens".into(), v.clone());
    }
    if let Some(v) = resp.pointer("/usage/cache_read_input_tokens") {
        provider_fields.insert("cache_read_input_tokens".into(), v.clone());
    }
    // ... 正常解析 ...
    Ok(InternalResponse { /* ..., */ provider_fields })
}
```

各 ResponseFormatter 可选择性输出：

```rust
fn format_response(&self, resp: &InternalResponse) -> Value {
    let mut output = /* 标准格式 */;
    if !resp.provider_fields.is_empty() {
        output["_provider"] = serde_json::to_value(&resp.provider_fields).unwrap();
    }
    output
}
```

---

### 5.3 TokenUsage 扩展

```rust
// protocol/types.rs
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: Option<u32>,       // Anthropic prompt caching
    pub cache_creation_tokens: Option<u32>,    // Anthropic prompt caching
    pub reasoning_tokens: Option<u32>,         // OpenAI o1/o3 reasoning
    pub total_tokens: Option<u32>,             // 便捷字段
}

impl TokenUsage {
    pub fn total(&self) -> u32 {
        self.total_tokens.unwrap_or(self.input_tokens + self.output_tokens)
    }
}
```

---

### 5.4 同协议短路优化

**问题**：OpenAI 入口 → OpenAI 出口（如转发到 Groq/DeepSeek），仍走完整 decode → InternalRequest → encode，有信息损耗风险和不必要的性能开销。

**实现方案**：在 `proxy_pipeline` 中增加短路判断：

```rust
async fn proxy_pipeline(/* ... */) -> Response {
    // ... 路由匹配 ...
    let egress: Protocol = provider.protocol.parse().unwrap_or(Protocol::OpenAI);

    // 同协议且不需要 semantic 处理时，跳过转换
    let can_shortcut =
        ingress.route_protocol() == egress.route_protocol()
        && !needs_ollama_tools_strip
        && (route.target_model.is_empty() || route.target_model == "*"
            || route.target_model == request_model);

    let (egress_body, extra_headers) = if can_shortcut {
        // 直接使用原始 body，仅做 model 名覆盖
        let mut body = original_body; // 保存一份原始 JSON
        if !route.target_model.is_empty() && route.target_model != "*" {
            body["model"] = Value::String(route.target_model.clone());
        }
        (body, HeaderMap::new())
    } else {
        // 完整桥接转换
        let encoder = crate::protocol::get_encoder(egress);
        encoder.encode_request(&internal)?
    };
    // ...
}
```

需要在 `universal_proxy` 中保存原始 body 的副本传入 pipeline。

---

## 6. P2：运维与生态

### 6.1 直通 (Passthrough) 模式

新增通配路由，对匹配的 Provider 做原始字节流转发：

```rust
// proxy/server.rs
.route("/passthrough/:provider/*rest", any(handler::passthrough_proxy))

// proxy/passthrough.rs（新文件）
pub async fn passthrough_proxy(
    State(gw): State<Gateway>,
    Path((provider_name, rest)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,          // 原始字节，不做 JSON 解析
) -> Response {
    let provider = /* 按名称查找 provider */;
    let adapter = get_adapter(&provider, /* ... */);
    let url = adapter.build_url(&provider.base_url, &format!("/{rest}"), &provider.api_key);
    let auth_headers = adapter.auth_headers(&provider.api_key);

    // 流式转发
    let upstream_resp = gw.http_client
        .request(method, &url)
        .headers(merge_headers(auth_headers, forward_client_headers(&headers)))
        .body(body)
        .send()
        .await?;

    // 直接流式返回，不解析
    let status = upstream_resp.status();
    let resp_headers = upstream_resp.headers().clone();
    let stream = upstream_resp.bytes_stream();
    Response::builder()
        .status(status)
        .headers(resp_headers)
        .body(Body::from_stream(stream))
}
```

### 6.2 可观测性集成

新增 `observability/` 模块，定义回调 trait：

```rust
#[async_trait]
pub trait ObservabilityCallback: Send + Sync {
    async fn on_request_start(&self, metadata: &RequestMetadata);
    async fn on_request_end(&self, metadata: &RequestMetadata, result: &RequestResult);
}
```

首期实现 Prometheus metrics 端点（`GET /metrics`），暴露：
- `nyro_requests_total{ingress, egress, provider, model, status}`
- `nyro_request_duration_seconds{...}`
- `nyro_tokens_total{direction=input|output, ...}`

### 6.3 成本追踪

新增 `cost/` 模块 + 定价数据文件：

```rust
pub struct CostCalculator {
    prices: HashMap<String, ModelPrice>, // 从 JSON 加载
}

pub struct ModelPrice {
    pub input_per_1k: f64,
    pub output_per_1k: f64,
    pub cache_read_per_1k: Option<f64>,
}

impl CostCalculator {
    pub fn calculate(&self, model: &str, usage: &TokenUsage) -> Option<f64> {
        let price = self.prices.get(model)?;
        Some(
            (usage.input_tokens as f64 / 1000.0) * price.input_per_1k
            + (usage.output_tokens as f64 / 1000.0) * price.output_per_1k
        )
    }
}
```

在日志中记录 `estimated_cost`，管理面板可展示费用统计。

### 6.4 Docker 支持

在仓库根目录添加：

```dockerfile
# Dockerfile
FROM rust:1.85-slim AS builder
WORKDIR /app
COPY . .
RUN cargo build -p nyro-server --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/nyro-server /usr/local/bin/
EXPOSE 19530 19531
ENTRYPOINT ["nyro-server"]
```

```yaml
# docker-compose.yml
services:
  nyro:
    build: .
    ports:
      - "19530:19530"
      - "19531:19531"
    volumes:
      - nyro-data:/root/.nyro
    environment:
      - NYRO_PROXY_HOST=0.0.0.0
volumes:
  nyro-data:
```

---

## 7. P3：远期规划

### 7.1 缓存层

- 基于请求 hash（model + messages hash）的内存缓存
- 可选 Redis 后端
- TTL 可配，按路由/模型粒度控制

### 7.2 Guardrails 内容安全

- 输入/输出的关键词过滤
- PII 检测与脱敏
- 自定义规则引擎（正则或外部 webhook）

### 7.3 A2A / MCP 协议

- A2A (Agent-to-Agent)：JSON-RPC 2.0 Agent Gateway
- MCP (Model Context Protocol)：Streamable HTTP / SSE 传输
- 在 Agent 生态日益重要的背景下，这是中长期的竞争力

---

## 8. Encoder trait 拆分建议

当前 `EgressEncoder` 把参数映射、消息编码、payload 校验混在一个 `encode_request` 中。建议拆分为更细粒度的步骤，与参数安全阀更好配合：

```rust
pub trait EgressEncoder {
    /// 该协议/Provider 支持的 extra 参数
    fn supported_extra_params(&self) -> &[&str] { &[] }

    /// 参数映射（独立于消息体）
    fn map_params(&self, req: &InternalRequest) -> Result<HashMap<String, Value>> {
        Ok(HashMap::new())
    }

    /// 消息体编码
    fn encode_messages(&self, messages: &[InternalMessage]) -> Result<Value>;

    /// 组装最终请求体（默认实现调用上面两个方法）
    fn encode_request(&self, req: &InternalRequest) -> Result<(Value, HeaderMap)>;

    /// 出口路径
    fn egress_path(&self, model: &str, stream: bool) -> String;

    /// 校验最终 payload 合法性（如 Anthropic 的结构检查）
    fn validate_payload(&self, body: &Value) -> Result<()> { Ok(()) }
}
```

---

## 9. Nyro 现有优势（应保持）

在追赶 LiteLLM 的同时，Nyro 的以下设计优势应当保持：

| 优势 | 说明 |
|------|------|
| **Rust 性能** | 协议转换零 GC，流式 SSE 内存可控 |
| **语义层 (semantic/)** | `tool_correlation`、`reasoning` 归一化是 LiteLLM 没有的独立层 |
| **Tauri 桌面端** | 本地化体验，LiteLLM 无此形态 |
| **多存储后端** | SQLite/Postgres/Memory，LiteLLM 仅 Prisma |
| **轻量部署** | 单二进制，无 Python 运行时依赖 |
| **InternalRequest/Response 中间表示** | 比 LiteLLM 的 per-provider transform 更清晰的双层架构 |

---

## 10. 实施节奏建议

```
Phase 1 (1-2 周)：P0 三项
├── 参数安全阀
├── 错误映射
└── 基础重试 + Fallback

Phase 2 (2-3 周)：P1 四项
├── ProviderAdapter trait
├── provider_fields + TokenUsage 扩展
└── 同协议短路

Phase 3 (3-4 周)：P2 四项
├── Passthrough 直通模式
├── Prometheus metrics
├── 成本追踪
└── Docker

Phase 4 (远期)：P3
├── 缓存层
├── Guardrails
└── A2A / MCP
```
