# Race Gateway 后端接口草案

## 1. 目的

本文把 [race-gateway-architecture.md](./race-gateway-architecture.md) 压成可直接开发的接口草案。

设计基线：

- 严格对齐旧生产并发竞速核心语义
- 不把 group 权重状态按协议拆开
- Phase 1 保留“共享 key 池 + 每请求随机选 key”
- WebUI 风格参考 `nyro`，但服务边界独立

主要关联章节：

- 架构文档 3.1 设计取舍结论
- 架构文档 6 核心对象
- 架构文档 9 RaceCore 语义
- 架构文档 10 协议适配器
- 架构文档 13 持久化与运行时状态

## 2. Phase 1 固定约束

Phase 1 必须固化以下约束：

- 一个 `RaceGroup` 至少 `1` 个 candidate，最多 `8` 个
- `response_protection_timeout_ms` 范围 `1000..120000`
- 同一组内 `candidate.name` 唯一
- 默认 key 选择策略为 `random`
- 同一 `RaceGroup` 的权重和惩罚状态按组共享，不按协议拆分
- `race_max_wait_time_ms` 为空时，默认 `max(candidate.response_protection_timeout_ms) * 3`

## 3. 基本枚举与通用类型

### 3.1 ProtocolFamily

```rust
pub enum ProtocolFamily {
    OpenAi,
    Anthropic,
    Google,
}
```

对外 JSON：

```json
"openai" | "anthropic" | "google"
```

### 3.2 AuthStrategy

```rust
pub enum AuthStrategy {
    Bearer,
    HeaderApiKey {
        header_name: String,
    },
    QueryApiKey {
        parameter_name: String,
    },
}
```

对外 JSON：

```json
{ "kind": "bearer" }
{ "kind": "header_api_key", "header_name": "x-api-key" }
{ "kind": "query_api_key", "parameter_name": "key" }
```

### 3.3 KeySelectionStrategy

Phase 1 只开放一个值：

```rust
pub enum KeySelectionStrategy {
    Random,
}
```

对外 JSON：

```json
"random"
```

### 3.4 DownstreamRouteKind

```rust
pub enum DownstreamRouteKind {
    OpenAiChatCompletions,
    OpenAiResponses,
    AnthropicMessages,
    GoogleV1BetaModels,
    GoogleV1Models,
}
```

说明：

- 它由 data plane 路由解析得出
- 运行时选择 candidate 后，用它构造下游真实请求

## 4. 配置态对象

### 4.1 RaceTargetEndpoint

一个模型在某个协议族下的下游原生入口。

```rust
pub struct RaceTargetEndpoint {
    pub protocol_family: ProtocolFamily,
    pub base_url: String,
    pub auth_strategy: AuthStrategy,
    pub key_pool_id: String,
    pub request_timeout_ms: Option<u64>,
    pub extra_headers: std::collections::BTreeMap<String, String>,
    pub extra_query: std::collections::BTreeMap<String, String>,
    pub enabled: bool,
}
```

说明：

- `base_url` 是协议根路径，不包含最终 body 里的 `model`
- `key_pool_id` 指向共享 key 池
- `request_timeout_ms` 是单 candidate 真实 HTTP 请求超时，不参与竞速 winner 判定
- `extra_headers` / `extra_query` 用于补充固定请求参数

对外 JSON：

```json
{
  "protocol_family": "openai",
  "base_url": "https://integrate.api.nvidia.com/v1",
  "auth_strategy": { "kind": "bearer" },
  "key_pool_id": "nim-global",
  "request_timeout_ms": 300000,
  "extra_headers": {},
  "extra_query": {},
  "enabled": true
}
```

### 4.2 RaceModelDescriptor

给候选提供可复用的模型元数据和协议端点映射。

```rust
pub struct RaceModelDescriptor {
    pub id: String,
    pub display_name: String,
    pub upstream_model: String,
    pub description: String,
    pub enabled: bool,
    pub endpoints: Vec<RaceTargetEndpoint>,
    pub metadata: serde_json::Value,
}
```

说明：

- `upstream_model` 对齐旧生产 `models.*.upstream_model`
- 一个模型可以声明多个 `endpoints`
- 同一个 `RaceGroup` 不按协议拆分，但每次请求必须能在 candidate 对应 model 上找到当前协议的 endpoint
- 同一 `RaceModelDescriptor` 不允许为同一 `protocol_family` 配置多个 enabled endpoint

### 4.3 RaceCandidate

```rust
pub struct RaceCandidate {
    pub id: String,
    pub group_id: String,
    pub name: String,
    pub model_id: Option<String>,
    pub upstream_model: String,
    pub inline_endpoint_overrides: Vec<RaceTargetEndpoint>,
    pub initial_weight: f64,
    pub response_protection_timeout_ms: u64,
    pub enabled: bool,
    pub metadata: serde_json::Value,
}
```

说明：

- `model_id` 是推荐路径，优先引用 `RaceModelDescriptor`
- `upstream_model` 保留旧生产直接写模型名的能力
- `inline_endpoint_overrides` 只作迁移/调试兜底，常规配置优先走 `model_id`
- `response_protection_timeout_ms` 是竞速保护时间，不是请求超时
- 同一 candidate 不允许为同一 `protocol_family` 配置多个 enabled inline endpoint

对外 JSON：

```json
{
  "id": "cand-a",
  "group_id": "nv-fast",
  "name": "A",
  "model_id": "deepseek-v4-flash",
  "upstream_model": "deepseek-ai/deepseek-v4-flash",
  "inline_endpoint_overrides": [],
  "initial_weight": 100.0,
  "response_protection_timeout_ms": 5000,
  "enabled": true,
  "metadata": {}
}
```

### 4.4 RaceGroup

```rust
pub struct RaceGroup {
    pub id: String,
    pub display_name: String,
    pub fallback_ratio: f64,
    pub decay_factor: f64,
    pub penalty_rate: f64,
    pub recovery_rate: f64,
    pub race_max_wait_time_ms: Option<u64>,
    pub enabled: bool,
    pub candidates: Vec<RaceCandidate>,
}
```

说明：

- `RaceGroup` 不带 `protocol_family`
- 同一组可以被 OpenAI / Anthropic / Google 入口复用
- 但每次请求的协议族必须能在 winner candidate 的 endpoint 上解析成功

### 4.5 RaceKeyPool

```rust
pub struct RaceKeyPool {
    pub id: String,
    pub display_name: String,
    pub auth_strategy: AuthStrategy,
    pub selection_strategy: KeySelectionStrategy,
    pub enabled: bool,
    pub keys: Vec<RaceKey>,
}
```

```rust
pub struct RaceKey {
    pub id: String,
    pub key_pool_id: String,
    pub secret: String,
    pub enabled: bool,
    pub metadata: serde_json::Value,
}
```

说明：

- Phase 1 默认随机选 key
- 旧生产是一个 provider-global key pool；新服务抽象为可复用共享池，但默认使用方式保持一致

### 4.6 RaceSettings

```rust
pub struct RaceSettings {
    pub enable_race_diagnostics_header: bool,
    pub max_buffer_events: usize,
    pub buffer_backpressure_timeout_ms: u64,
}
```

Phase 1 默认值：

```json
{
  "enable_race_diagnostics_header": false,
  "max_buffer_events": 100000,
  "buffer_backpressure_timeout_ms": 100
}
```

### 4.7 请求时 endpoint 解析结果

为了保持 group 级权重共享，同时不把不存在当前协议 endpoint 的 candidate 错误拉进竞速，运行时需要一个显式的解析结果对象。

```rust
pub struct ResolvedCandidateTarget {
    pub candidate: RaceCandidate,
    pub endpoint: RaceTargetEndpoint,
    pub endpoint_source: ResolvedEndpointSource,
    pub key_pool_id: String,
}
```

```rust
pub enum ResolvedEndpointSource {
    CandidateInlineOverride,
    ModelDescriptor,
}
```

解析规则：

1. 只处理 `enabled == true` 的 candidate。
2. 解析顺序固定为：
   - `candidate.inline_endpoint_overrides`
   - `model.endpoints`
3. 只选取与本次 `protocol_family` 匹配且 `enabled == true` 的 endpoint。
4. 解析成功的 candidate 才进入本轮竞速。
5. 未解析成功的 candidate：
   - 不参加本轮排序
   - 不阻塞 winner
   - 不参与本轮惩罚
   - 但不清空其 group 级历史权重
6. 若一个 group 在本次协议下没有任何 `ResolvedCandidateTarget`，直接返回配置型错误，不进入竞速。

## 5. 运行时对象

### 5.1 CandidateWeightSnapshot

```rust
pub struct CandidateWeightSnapshot {
    pub initial_weight: f64,
    pub effective_weight: f64,
    pub weight_deviation: f64,
    pub status: String,
}
```

### 5.2 CandidateState

```rust
pub struct CandidateState {
    pub candidate_id: String,
    pub candidate_name: String,
    pub launched_at_mono_ms: Option<u64>,
    pub first_content_at_mono_ms: Option<u64>,
    pub winner_selected: bool,
    pub failed: bool,
    pub ended: bool,
    pub error: Option<String>,
    pub buffered_count: u64,
    pub relative_delay_ms: u64,
    pub selected_key_masked: Option<String>,
}
```

### 5.3 RaceRecord

```rust
pub struct RaceRecord {
    pub id: u64,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub group_id: String,
    pub protocol: ProtocolFamily,
    pub winner: Option<String>,
    pub duration_ms: u64,
    pub buffer_events: u64,
    pub participants: Vec<String>,
    pub first_content_times_ms: std::collections::BTreeMap<String, u64>,
    pub penalty_applied: bool,
    pub errors: std::collections::BTreeMap<String, String>,
}
```

### 5.4 GroupRuntimeSnapshot

```rust
pub struct GroupRuntimeSnapshot {
    pub group_id: String,
    pub display_name: String,
    pub enabled: bool,
    pub eligible_candidate_counts_by_protocol: std::collections::BTreeMap<String, usize>,
    pub effective_weights: std::collections::BTreeMap<String, CandidateWeightSnapshot>,
    pub candidate_statuses: Vec<CandidateRuntimeSnapshot>,
    pub race_stats: RaceStatsSnapshot,
}
```

```rust
pub struct CandidateRuntimeSnapshot {
    pub candidate_id: String,
    pub candidate_name: String,
    pub upstream_model: String,
    pub enabled: bool,
    pub eligible_protocol_families: Vec<ProtocolFamily>,
    pub response_protection_timeout_ms: u64,
    pub initial_weight: f64,
    pub effective_weight: f64,
    pub weight_deviation: f64,
    pub status: String,
}
```

```rust
pub struct RaceStatsSnapshot {
    pub total_races: u64,
    pub winner_distribution: std::collections::BTreeMap<String, u64>,
    pub protocol_distribution: std::collections::BTreeMap<String, u64>,
    pub by_protocol: std::collections::BTreeMap<String, ProtocolStatsSnapshot>,
    pub avg_race_duration_ms: f64,
    pub avg_buffer_events: f64,
    pub recent_races: Vec<RaceRecord>,
}
```

```rust
pub struct ProtocolStatsSnapshot {
    pub total: u64,
    pub wins: std::collections::BTreeMap<String, u64>,
    pub avg_race_duration_ms: f64,
    pub avg_buffer_events: f64,
    pub all_failed_count: u64,
}
```

### 5.5 Summary DTO

列表页不应要求前端自己拼 summary，因此配置态需要直接提供 summary 对象。

```rust
pub struct RaceModelSummary {
    pub id: String,
    pub display_name: String,
    pub upstream_model: String,
    pub enabled: bool,
    pub protocol_families: Vec<ProtocolFamily>,
    pub key_pool_ids: Vec<String>,
}
```

```rust
pub struct RaceGroupSummary {
    pub id: String,
    pub display_name: String,
    pub enabled: bool,
    pub candidate_count: usize,
    pub enabled_candidate_count: usize,
    pub protocol_families: Vec<ProtocolFamily>,
    pub candidate_names: Vec<String>,
}
```

```rust
pub struct RaceKeyPoolSummary {
    pub id: String,
    pub display_name: String,
    pub enabled: bool,
    pub auth_strategy: AuthStrategy,
    pub selection_strategy: KeySelectionStrategy,
    pub total_keys: usize,
    pub enabled_keys: usize,
}
```

### 5.6 RaceDiagnosticsHeaderPayload

响应头建议沿用旧生产语义，但改服务前缀：

- header 名：`x-nyro-race-diagnostics`

```rust
pub struct RaceDiagnosticsHeaderPayload {
    pub group: String,
    pub protocol: ProtocolFamily,
    pub winner: Option<String>,
    pub penalty_applied: bool,
    pub penalized_candidates: Vec<String>,
    pub duration_ms: Option<u64>,
    pub all_failed: bool,
    pub candidates: Vec<CandidateDiagnosticsPayload>,
}
```

```rust
pub struct CandidateDiagnosticsPayload {
    pub name: String,
    pub upstream_model: String,
    pub key: String,
    pub delay_s: f64,
    pub launch_offset_s: Option<f64>,
    pub first_content_offset_s: Option<f64>,
    pub initial_weight: f64,
    pub effective_weight: f64,
    pub weight_deviation: f64,
    pub status: String,
    pub failed: bool,
    pub error: Option<String>,
}
```

要求：

- `key` 必须脱敏
- `error` 必须截断
- diagnostics 关闭时不构建此对象

### 5.7 Validate API 错误对象

```rust
pub struct ValidationIssue {
    pub field: String,
    pub code: String,
    pub message: String,
}
```

```rust
pub struct ValidationErrorResponse {
    pub valid: bool,
    pub issues: Vec<ValidationIssue>,
}
```

说明：

- `field` 使用稳定路径，例如：
  - `display_name`
  - `candidates[0].name`
  - `candidates[1].inline_endpoint_overrides[0].protocol_family`
- `code` 使用稳定错误码，便于 WebUI 做定向提示
- `valid == true` 时 `issues` 返回空数组

## 6. 核心 trait 与签名草案

### 6.1 配置存储

```rust
#[async_trait::async_trait]
pub trait RaceConfigStore: Send + Sync {
    async fn list_models(&self) -> anyhow::Result<Vec<RaceModelSummary>>;
    async fn get_model(&self, model_id: &str) -> anyhow::Result<Option<RaceModelDescriptor>>;
    async fn put_model(&self, model: RaceModelDescriptor) -> anyhow::Result<RaceModelDescriptor>;
    async fn delete_model(&self, model_id: &str) -> anyhow::Result<bool>;

    async fn list_groups(&self) -> anyhow::Result<Vec<RaceGroupSummary>>;
    async fn get_group(&self, group_id: &str) -> anyhow::Result<Option<RaceGroup>>;
    async fn put_group(&self, group: RaceGroup) -> anyhow::Result<RaceGroup>;
    async fn delete_group(&self, group_id: &str) -> anyhow::Result<bool>;

    async fn list_key_pools(&self) -> anyhow::Result<Vec<RaceKeyPoolSummary>>;
    async fn get_key_pool(&self, key_pool_id: &str) -> anyhow::Result<Option<RaceKeyPool>>;
    async fn put_key_pool(&self, pool: RaceKeyPool) -> anyhow::Result<RaceKeyPool>;
    async fn delete_key_pool(&self, key_pool_id: &str) -> anyhow::Result<bool>;

    async fn get_settings(&self) -> anyhow::Result<RaceSettings>;
    async fn put_settings(&self, settings: RaceSettings) -> anyhow::Result<RaceSettings>;
}
```

### 6.2 KeyPoolSelector

```rust
pub trait KeyPoolSelector: Send + Sync {
    fn select_key<'a>(
        &self,
        pool: &'a RaceKeyPool,
        request_seed: Option<u64>,
    ) -> anyhow::Result<&'a RaceKey>;
}
```

Phase 1 默认实现：

- `RandomKeyPoolSelector`

### 6.3 WeightTracker

```rust
pub trait GroupWeightTracker: Send + Sync {
    fn reconfigure(&self, group_id: &str, candidates: &[RaceCandidate], penalty_rate: f64, recovery_rate: f64) -> anyhow::Result<()>;
    fn snapshot(&self, group_id: &str) -> anyhow::Result<std::collections::BTreeMap<String, CandidateWeightSnapshot>>;
    fn apply_penalty(&self, group_id: &str, penalized_names: &[String]) -> anyhow::Result<()>;
    fn reset(&self, group_id: &str) -> anyhow::Result<()>;
}
```

### 6.4 RaceRunner

```rust
#[async_trait::async_trait]
pub trait RaceRunner: Send + Sync {
    async fn race_stream(
        &self,
        ctx: RaceRequestContext,
    ) -> anyhow::Result<RaceStreamResult>;
}
```

```rust
pub struct RaceRequestContext {
    pub group: RaceGroup,
    pub protocol_family: ProtocolFamily,
    pub route_kind: DownstreamRouteKind,
    pub request_headers: axum::http::HeaderMap,
    pub request_body: bytes::Bytes,
    pub diagnostics_enabled: bool,
}
```

### 6.5 CandidateTargetResolver

```rust
pub trait CandidateTargetResolver: Send + Sync {
    fn resolve_candidates(
        &self,
        group: &RaceGroup,
        models: &std::collections::BTreeMap<String, RaceModelDescriptor>,
        protocol_family: ProtocolFamily,
    ) -> anyhow::Result<Vec<ResolvedCandidateTarget>>;
}
```

说明：

- 这个 resolver 是 `RaceRunner` 进入 `RaceCore` 前的固定步骤
- 它只负责“谁在本轮可参赛、用哪个 endpoint、走哪个 key pool”
- 它不负责 winner 判定，也不负责选 key

```rust
pub struct RaceStreamResult {
    pub status: http::StatusCode,
    pub headers: http::HeaderMap,
    pub body: axum::body::Body,
    pub diagnostics_header: Option<String>,
}
```

### 6.5 DownstreamDispatcher

```rust
#[async_trait::async_trait]
pub trait DownstreamDispatcher: Send + Sync {
    async fn dispatch_stream(
        &self,
        candidate: &RaceCandidate,
        endpoint: &RaceTargetEndpoint,
        selected_key: &RaceKey,
        route_kind: DownstreamRouteKind,
        body: bytes::Bytes,
        request_headers: &axum::http::HeaderMap,
    ) -> anyhow::Result<reqwest::Response>;
}
```

说明：

- `dispatch_stream` 只负责真正发起一个 candidate 的下游请求
- winner 判定和候选缓冲仍由 `RaceCore` 控制

## 7. SQLite 表草案

### 7.1 `race_models`

```sql
CREATE TABLE race_models (
  id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  upstream_model TEXT NOT NULL,
  description TEXT NOT NULL,
  enabled INTEGER NOT NULL,
  metadata_json TEXT NOT NULL
);
```

### 7.2 `race_model_endpoints`

```sql
CREATE TABLE race_model_endpoints (
  model_id TEXT NOT NULL,
  protocol_family TEXT NOT NULL,
  base_url TEXT NOT NULL,
  auth_strategy_json TEXT NOT NULL,
  key_pool_id TEXT NOT NULL,
  request_timeout_ms INTEGER NULL,
  extra_headers_json TEXT NOT NULL,
  extra_query_json TEXT NOT NULL,
  enabled INTEGER NOT NULL,
  PRIMARY KEY (model_id, protocol_family),
  FOREIGN KEY(model_id) REFERENCES race_models(id) ON DELETE CASCADE,
  FOREIGN KEY(key_pool_id) REFERENCES race_key_pools(id)
);
```

### 7.3 `race_groups`

```sql
CREATE TABLE race_groups (
  id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  fallback_ratio REAL NOT NULL,
  decay_factor REAL NOT NULL,
  penalty_rate REAL NOT NULL,
  recovery_rate REAL NOT NULL,
  race_max_wait_time_ms INTEGER NULL,
  enabled INTEGER NOT NULL
);
```

### 7.4 `race_group_candidates`

```sql
CREATE TABLE race_group_candidates (
  id TEXT PRIMARY KEY,
  group_id TEXT NOT NULL,
  name TEXT NOT NULL,
  candidate_order INTEGER NOT NULL,
  model_id TEXT NULL,
  upstream_model TEXT NOT NULL,
  inline_endpoints_json TEXT NOT NULL,
  initial_weight REAL NOT NULL,
  response_protection_timeout_ms INTEGER NOT NULL,
  enabled INTEGER NOT NULL,
  metadata_json TEXT NOT NULL,
  UNIQUE(group_id, name),
  FOREIGN KEY(group_id) REFERENCES race_groups(id) ON DELETE CASCADE,
  FOREIGN KEY(model_id) REFERENCES race_models(id)
);
```

### 7.5 `race_key_pools`

```sql
CREATE TABLE race_key_pools (
  id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  auth_strategy_json TEXT NOT NULL,
  selection_strategy TEXT NOT NULL,
  enabled INTEGER NOT NULL
);
```

### 7.6 `race_keys`

```sql
CREATE TABLE race_keys (
  id TEXT PRIMARY KEY,
  key_pool_id TEXT NOT NULL,
  secret TEXT NOT NULL,
  enabled INTEGER NOT NULL,
  metadata_json TEXT NOT NULL,
  FOREIGN KEY(key_pool_id) REFERENCES race_key_pools(id) ON DELETE CASCADE
);
```

### 7.7 `race_settings`

```sql
CREATE TABLE race_settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
```

## 8. Proxy 端点草案

### 8.1 健康检查

- `GET /healthz`

返回：

```json
{
  "status": "ok",
  "service": "race-gateway",
  "bind_addr": "127.0.0.1:2090",
  "proxy_bind_addr": "127.0.0.1:2090"
}
```

### 8.2 OpenAI Chat

- `POST /groups/:group_id/openai/v1/chat/completions`

行为：

1. 读取 `group_id`
2. 解析 OpenAI 请求体
3. 以 `ProtocolFamily::OpenAi` 进入 `RaceRunner`
4. 并发发起候选
5. winner 按 OpenAI SSE 透传

### 8.3 OpenAI Responses

- `POST /groups/:group_id/openai/v1/responses`

Phase 1 建议：

- 先只支持 stream passthrough
- 若旧生产没有同等成熟实现，可标为 `[todo after chat]`

### 8.4 Anthropic Messages

- `POST /groups/:group_id/anthropic/v1/messages`

行为：

- 复用同一 `RaceGroup`
- 使用 Anthropic adapter 做有效内容判断和协议收尾

### 8.5 Gemini

- `POST /groups/:group_id/google/v1beta/models/:model_action`
- `POST /groups/:group_id/google/models/:model_action`

行为：

- `model_action` 参与下游 URL 构造
- 由 Google adapter 判断有效内容

## 9. Admin API 草案

### 9.1 健康与总览

- `GET /admin/healthz`
- `GET /admin/runtime/groups`
- `GET /admin/runtime/groups/:group_id`

### 9.2 模型管理

- `GET /admin/models`
- `GET /admin/models/:model_id`
- `PUT /admin/models/:model_id`
- `DELETE /admin/models/:model_id`

说明：

- `GET /admin/models` 返回 `RaceModelSummary[]`
- `GET /admin/models/:model_id` 返回完整 `RaceModelDescriptor`

`PUT /admin/models/:model_id` 请求体：

```json
{
  "id": "deepseek-v4-flash",
  "display_name": "DeepSeek V4 Flash",
  "upstream_model": "deepseek-ai/deepseek-v4-flash",
  "description": "Fast candidate",
  "enabled": true,
  "endpoints": [
    {
      "protocol_family": "openai",
      "base_url": "https://integrate.api.nvidia.com/v1",
      "auth_strategy": { "kind": "bearer" },
      "key_pool_id": "nim-global",
      "request_timeout_ms": 300000,
      "extra_headers": {},
      "extra_query": {},
      "enabled": true
    }
  ],
  "metadata": {}
}
```

### 9.3 组管理

- `GET /admin/groups`
- `GET /admin/groups/:group_id`
- `PUT /admin/groups/:group_id`
- `DELETE /admin/groups/:group_id`

说明：

- `GET /admin/groups` 返回 `RaceGroupSummary[]`
- `GET /admin/groups/:group_id` 返回完整 `RaceGroup`

`PUT /admin/groups/:group_id` 请求体：

```json
{
  "id": "nv-fast",
  "display_name": "NV Fast",
  "fallback_ratio": 0.5,
  "decay_factor": 0.8,
  "penalty_rate": 5.0,
  "recovery_rate": 0.0042,
  "race_max_wait_time_ms": 15000,
  "enabled": true,
  "candidates": [
    {
      "id": "cand-a",
      "group_id": "nv-fast",
      "name": "A",
      "model_id": "deepseek-v4-flash",
      "upstream_model": "deepseek-ai/deepseek-v4-flash",
      "inline_endpoint_overrides": [],
      "initial_weight": 100.0,
      "response_protection_timeout_ms": 5000,
      "enabled": true,
      "metadata": {}
    }
  ]
}
```

### 9.4 key 池管理

- `GET /admin/key-pools`
- `GET /admin/key-pools/:key_pool_id`
- `PUT /admin/key-pools/:key_pool_id`
- `DELETE /admin/key-pools/:key_pool_id`

说明：

- `GET /admin/key-pools` 返回 `RaceKeyPoolSummary[]`
- `GET /admin/key-pools/:key_pool_id` 返回完整 `RaceKeyPool`

`PUT /admin/key-pools/:key_pool_id` 请求体：

```json
{
  "id": "nim-global",
  "display_name": "NIM Global",
  "auth_strategy": { "kind": "bearer" },
  "selection_strategy": "random",
  "enabled": true,
  "keys": [
    {
      "id": "key-a",
      "key_pool_id": "nim-global",
      "secret": "nvapi-xxx",
      "enabled": true,
      "metadata": {}
    }
  ]
}
```

### 9.5 设置管理

- `GET /admin/settings`
- `PUT /admin/settings`

### 9.6 校验预览

- `POST /admin/validate/group`
- `POST /admin/validate/model`
- `POST /admin/validate/key-pool`

说明：

- 返回结构化错误，给 WebUI 做表单即时校验
- 返回体统一采用 `ValidationErrorResponse`

## 10. Runtime API 草案

### 10.1 组运行时列表

- `GET /admin/runtime/groups`

返回：

```json
[
  {
    "group_id": "nv-fast",
    "display_name": "NV Fast",
    "enabled": true,
    "eligible_candidate_counts_by_protocol": {
      "openai": 2,
      "anthropic": 1
    },
    "effective_weights": {
      "A": {
        "initial_weight": 100.0,
        "effective_weight": 95.0,
        "weight_deviation": -5.0,
        "status": "recovering"
      }
    },
    "candidate_statuses": [
      {
        "candidate_id": "cand-a",
        "candidate_name": "A",
        "upstream_model": "deepseek-ai/deepseek-v4-flash",
        "enabled": true,
        "eligible_protocol_families": ["openai", "anthropic"],
        "response_protection_timeout_ms": 5000,
        "initial_weight": 100.0,
        "effective_weight": 95.0,
        "weight_deviation": -5.0,
        "status": "recovering"
      }
    ],
    "race_stats": {
      "total_races": 12,
      "winner_distribution": { "A": 8, "B": 4 },
      "protocol_distribution": { "anthropic": 7, "openai": 5 },
      "by_protocol": {
        "anthropic": { "total": 7, "wins": { "A": 4, "B": 3 } },
        "openai": { "total": 5, "wins": { "A": 4, "B": 1 } }
      },
      "avg_race_duration_ms": 3310.5,
      "avg_buffer_events": 18.0,
      "recent_races": []
    }
  }
]
```

### 10.2 单组运行时

- `GET /admin/runtime/groups/:group_id`

说明：

- 与列表版同结构，但可以返回更多 `recent_races`

## 11. WebUI 数据需求草案

为便于前端开发，后端应保证：

- 列表页接口直接返回可展示 summary，不要求前端二次拼装
- 单项详情接口返回完整编辑对象
- runtime 接口返回已经展开好的权重和统计
- 表单校验接口返回字段级错误

前端需要的最小接口集合：

- `/admin/healthz`
- `/admin/models`
- `/admin/models/:model_id`
- `/admin/groups`
- `/admin/groups/:group_id`
- `/admin/key-pools`
- `/admin/key-pools/:key_pool_id`
- `/admin/settings`
- `/admin/runtime/groups`
- `/admin/runtime/groups/:group_id`
- `/admin/validate/group`
- `/admin/validate/model`
- `/admin/validate/key-pool`

## 12. 与旧生产逻辑对齐的特别说明

必须在实现和测试里显式覆盖这些点：

1. `RaceGroup` 权重状态按组共享，不按协议拆。
2. `RaceStats` 可以按协议分桶，但不能影响 `WeightTracker`。
3. key 池默认语义是共享池随机选 key。
4. `failed` 但已有 usable content 的 candidate 仍然可能胜出。
5. `race_max_wait_time_ms` 到达后先走 usable-content fallback，再决定 all-failed。
6. winner 已确定后中途失败，只做协议收尾，不重新竞选。
