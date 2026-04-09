# Nyro Gateway · 路由多目标 (1:N) 设计文档

**日期：2026-03-24**
**状态：Draft v3**
**关联：** `litellm-comparison-roadmap.md` §4.3 重试与 Fallback

---

## 一、背景与动机

### 1.1 现状

当前每条 Route 绑定**单一目标**（`target_provider` + `target_model`），即 1:1 映射：

```
Route
├── ingress_protocol: "openai"
├── virtual_model: "gpt-4o"
├── target_provider: <provider_id>   ← 唯一
├── target_model: "claude-sonnet-4-5" ← 唯一
└── access_control: bool
```

数据库 schema 保留了 `fallback_provider` / `fallback_model` / `priority` 列，但 **`Route` 结构体和 `proxy_pipeline` 均未使用**，runtime 是纯 1:1。

### 1.2 问题

| 问题 | 影响 |
|------|------|
| 单点故障 | Provider 宕机 → 该虚拟模型完全不可用 |
| 无法分流 | 无法按权重将流量分配到多个 Provider（成本优化、A/B） |
| 运维依赖手动切换 | 故障时需人工改路由目标 |
| 与竞品差距 | LiteLLM / OneAPI / LobeChat 均支持 1:N + 负载均衡 |

### 1.3 目标

在**最小化破坏性**的前提下，让一条 Route 支持 N 个目标，V1 提供两种负载均衡策略：

1. **加权轮询 (weighted)** — 按 weight 将流量分配到多个等价目标，支持成本调控
2. **主备分级 (priority)** — 始终优先主目标，全部失败时自动降级到备用目标

### 1.4 设计原则

- **两种策略覆盖核心场景**：weighted 做流量分发，priority 做高可用，不过度设计
- **策略字段存在 Route 表**：1:1 关系，无需独立表。代码层面 `selector.rs` 独立模块方便扩展
- **无状态负载均衡**：纯内存实现，不依赖 Redis 或外部存储
- **向后兼容**：现有单目标路由 = 只有一条 target 的特殊情况
- **移除 MongoDB 后端**：减少维护成本，SQL 三后端覆盖所有场景

### 1.5 V2 迭代方向（本期不做）

| 特性 | 时机 |
|------|------|
| `max_retries`（对同一 target 重试 N 次） | 用户需要应对 429 限流时 |
| `is_active`（单独禁用某个 target） | 用户需要"维护模式"时 |
| 更多策略（least-busy / latency-based / cost-based） | 有明确用户需求时 |
| `strategy_config` JSON 列（策略参数） | 新策略需要参数时 |
| 主动健康检查（定时 ping） | 大规模部署时 |

---

## 二、术语

| 术语 | 含义 |
|------|------|
| Route | 一条路由规则，由 `ingress_protocol + virtual_model` 唯一标识 |
| RouteTarget | 路由下的一个目标，绑定到 `provider + model` |
| Strategy | 路由的负载均衡策略：`weighted`（加权轮询）或 `priority`（主备分级） |
| Weight | `weighted` 策略下的流量权重，值越大流量越多 |
| Priority | `priority` 策略下的目标分级：1=主目标，2=备用目标 |
| HealthState | 运行时内存中的目标健康状态（不持久化） |

---

## 三、数据模型

### 3.1 Route 结构体变更

```rust
// db/models.rs

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Route {
    pub id: String,
    pub name: String,
    pub ingress_protocol: String,
    pub virtual_model: String,
    pub strategy: String,         // "weighted" | "priority"
    pub access_control: bool,
    pub is_active: bool,
    pub created_at: String,
}
```

策略枚举（代码层面）：

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RouteStrategy {
    Weighted,   // 加权轮询
    Priority,   // 主备分级
}

impl Default for RouteStrategy {
    fn default() -> Self {
        Self::Weighted
    }
}
```

> Route 表的 `strategy` 列为 `TEXT`，与枚举的序列化/反序列化对应。新增策略只需扩展枚举，不改 schema。

### 3.2 RouteTarget 结构体（新增）

```rust
// db/models.rs

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct RouteTarget {
    pub id: String,
    pub route_id: String,
    pub provider_id: String,
    pub model: String,
    pub weight: i32,        // weighted 模式有效，默认 100
    pub priority: i32,      // priority 模式有效，默认 1
    pub created_at: String,
}
```

字段在不同策略下的语义：

| 字段 | weighted 模式 | priority 模式 |
|------|--------------|---------------|
| `weight` | 流量权重，值越大流量越多。全等时退化为均匀随机 | **忽略** |
| `priority` | **忽略** | 1=主目标，2=备用。主全部失败后才启用备用 |

### 3.3 聚合视图

```rust
// db/models.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteWithTargets {
    #[serde(flatten)]
    pub route: Route,
    pub targets: Vec<RouteTarget>,
}
```

### 3.4 CRUD 结构体

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRoute {
    pub name: String,
    pub ingress_protocol: String,
    pub virtual_model: String,
    pub strategy: Option<String>,     // 默认 "weighted"
    pub access_control: Option<bool>,
    pub targets: Vec<CreateRouteTarget>,  // 至少 1 个
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRouteTarget {
    pub provider_id: String,
    pub model: String,
    pub weight: Option<i32>,    // 默认 100
    pub priority: Option<i32>,  // 默认 1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRoute {
    pub name: Option<String>,
    pub ingress_protocol: Option<String>,
    pub virtual_model: Option<String>,
    pub strategy: Option<String>,
    pub access_control: Option<bool>,
    pub is_active: Option<bool>,
    pub targets: Option<Vec<UpsertRouteTarget>>,  // 提供时全量替换
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertRouteTarget {
    pub id: Option<String>,     // 有 id 则更新，无则新建
    pub provider_id: String,
    pub model: String,
    pub weight: Option<i32>,
    pub priority: Option<i32>,
}
```

---

## 四、数据库 Schema

### 4.1 routes 表变更

```sql
ALTER TABLE routes ADD COLUMN strategy TEXT DEFAULT 'weighted';
```

旧列 `target_provider`、`target_model`、`fallback_provider`、`fallback_model` 保留不动，等迁移稳定后择机移除。

### 4.2 route_targets 表（新增）

#### SQLite

```sql
CREATE TABLE IF NOT EXISTS route_targets (
    id          TEXT PRIMARY KEY,
    route_id    TEXT NOT NULL REFERENCES routes(id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL REFERENCES providers(id),
    model       TEXT NOT NULL,
    weight      INTEGER DEFAULT 100,
    priority    INTEGER DEFAULT 1,
    created_at  TEXT DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_route_targets_route_id ON route_targets(route_id);
```

#### PostgreSQL

```sql
CREATE TABLE IF NOT EXISTS route_targets (
    id          TEXT PRIMARY KEY,
    route_id    TEXT NOT NULL REFERENCES routes(id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL REFERENCES providers(id),
    model       TEXT NOT NULL,
    weight      INTEGER DEFAULT 100,
    priority    INTEGER DEFAULT 1,
    created_at  TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_route_targets_route_id ON route_targets(route_id);
```

### 4.3 数据迁移

现有路由自动迁移为 `route_targets` 记录，策略默认 `weighted`：

```sql
-- 主目标迁移（SQLite 语法，PG 需替换 UUID 生成方式）
INSERT INTO route_targets (id, route_id, provider_id, model, weight, priority)
SELECT
    lower(hex(randomblob(16))),
    id,
    target_provider,
    target_model,
    100,
    1
FROM routes
WHERE target_provider IS NOT NULL
  AND target_provider != ''
  AND NOT EXISTS (
      SELECT 1 FROM route_targets rt WHERE rt.route_id = routes.id
  );

-- 备用目标迁移（有 fallback_provider 的路由，策略改为 priority）
INSERT INTO route_targets (id, route_id, provider_id, model, weight, priority)
SELECT
    lower(hex(randomblob(16))),
    id,
    fallback_provider,
    COALESCE(NULLIF(fallback_model, ''), target_model),
    100,
    2
FROM routes
WHERE fallback_provider IS NOT NULL
  AND fallback_provider != ''
  AND NOT EXISTS (
      SELECT 1 FROM route_targets rt
      WHERE rt.route_id = routes.id AND rt.priority = 2
  );

-- 有 fallback 的路由策略改为 priority
UPDATE routes SET strategy = 'priority'
WHERE fallback_provider IS NOT NULL
  AND fallback_provider != ''
  AND (strategy IS NULL OR strategy = 'weighted');
```

迁移在各存储后端的 `initialize()` 中执行，使用 `NOT EXISTS` 保证幂等。

---

## 五、移除 MongoDB 后端

### 5.1 理由

| 维度 | 现状 |
|------|------|
| 代码量 | `storage/mongo/` 约 1800 行，4 个文件 |
| 独有功能 | 无，与 SQL 三后端完全对等 |
| 测试覆盖 | 无 Rust 单测，无 CI 覆盖 |
| 维护成本 | 每新增一个 trait 方法需额外实现一套 |
| 用户价值 | AI 网关数据量小，SQLite 够用；分布式场景 PostgreSQL 是更主流选择 |

### 5.2 移除范围

| 文件/目录 | 操作 |
|-----------|------|
| `crates/nyro-core/src/storage/mongo/` | 删除整个目录 |
| `crates/nyro-core/src/storage/mod.rs` | 移除 `mod mongo` 引用 |
| `crates/nyro-core/src/lib.rs` | 移除 Mongo 分支和 `to_mongo_backend_config` |
| `crates/nyro-core/src/config.rs` | 移除 `MongoStorageConfig`、`MongoCollectionNames`、`Mongo` 枚举变体 |
| `crates/nyro-core/Cargo.toml` | 移除 `mongodb` 依赖 |
| `src-server/src/main.rs` | 移除 `--mongo-*` CLI 参数 |
| `README.md` / `README_CN.md` | 更新支持的存储后端列表 |
| `scripts/smoke/storage_backends_smoke.py` | 移除 `--backend mongo` 分支 |

### 5.3 时机

作为本次 1:N 改动的**前置步骤**执行。

---

## 六、存储层 Trait 变更

### 6.1 RouteTargetStore（新增）

```rust
// storage/traits.rs 或 storage/mod.rs

#[async_trait]
pub trait RouteTargetStore: Send + Sync {
    async fn list_targets_by_route(&self, route_id: &str) -> anyhow::Result<Vec<RouteTarget>>;
    async fn set_targets(
        &self,
        route_id: &str,
        targets: &[CreateRouteTarget],
    ) -> anyhow::Result<Vec<RouteTarget>>;
    async fn delete_targets_by_route(&self, route_id: &str) -> anyhow::Result<()>;
}
```

`set_targets` 语义为**全量替换**：先删除该 route 下所有旧 targets，再批量插入新 targets。

### 6.2 RouteSnapshotStore 变更

```rust
#[async_trait]
pub trait RouteSnapshotStore: Send + Sync {
    async fn load_active_snapshot(&self) -> anyhow::Result<Vec<RouteWithTargets>>;
}
```

SQL 后端实现：

```sql
SELECT r.*, rt.*
FROM routes r
LEFT JOIN route_targets rt ON rt.route_id = r.id
WHERE r.is_active = 1
ORDER BY r.id, rt.priority, rt.weight DESC;
```

应用层按 `route_id` 分组组装为 `Vec<RouteWithTargets>`。

### 6.3 存储实现矩阵

| Trait 方法 | SQLite | PostgreSQL |
|-----------|--------|------------|
| `list_targets_by_route` | ✓ | ✓ |
| `set_targets` | ✓ | ✓ |
| `delete_targets_by_route` | ✓ | ✓ |
| `load_active_snapshot` (改) | ✓ | ✓ |

---

## 七、路由缓存 (RouteCache) 重构

### 7.1 当前实现

```rust
pub struct RouteCache {
    pub routes: Vec<Route>,
}

impl RouteCache {
    pub fn match_route(&self, ingress_protocol: &str, model: &str) -> Option<&Route> { ... }
}
```

### 7.2 新实现

```rust
pub struct RouteCache {
    entries: Vec<RouteWithTargets>,
}

impl RouteCache {
    pub async fn load(store: &dyn RouteSnapshotStore) -> anyhow::Result<Self> {
        let entries = store.load_active_snapshot().await?;
        Ok(Self { entries })
    }

    pub fn match_route(&self, ingress_protocol: &str, model: &str) -> Option<&RouteWithTargets> {
        self.entries.iter().find(|e| {
            e.route.ingress_protocol == ingress_protocol
                && e.route.virtual_model == model
        })
    }
}
```

---

## 八、目标选择与负载均衡

### 8.1 模块结构

```
router/
├── mod.rs          // RouteCache + 公开接口
├── matcher.rs      // match_route（现有）
├── selector.rs     // TargetSelector + 策略实现
└── health.rs       // HealthRegistry
```

### 8.2 TargetSelector

```rust
// router/selector.rs（新增）

pub struct TargetSelector;

impl TargetSelector {
    /// 按策略返回排序后的候选目标列表
    /// 调用方依次尝试，直到成功或全部用尽
    pub fn select_ordered<'a>(
        &self,
        entry: &'a RouteWithTargets,
        health: &HealthRegistry,
    ) -> Vec<&'a RouteTarget> {
        match entry.route.strategy.parse().unwrap_or_default() {
            RouteStrategy::Weighted => self.weighted_select(&entry.targets, health),
            RouteStrategy::Priority => self.priority_select(&entry.targets, health),
        }
    }
}
```

### 8.3 weighted 策略（加权轮询）

所有 target 地位平等，按 weight 加权随机排序。priority 字段被忽略。

```rust
impl TargetSelector {
    fn weighted_select<'a>(
        &self,
        targets: &'a [RouteTarget],
        health: &HealthRegistry,
    ) -> Vec<&'a RouteTarget> {
        let (healthy, sick): (Vec<_>, Vec<_>) = targets.iter()
            .partition(|t| health.is_healthy(&t.id));

        // 健康的按权重随机排序，unhealthy 追加末尾（给自愈机会）
        let mut ordered = weighted_shuffle(&healthy);
        ordered.extend(weighted_shuffle(&sick));
        ordered
    }
}

/// 加权随机排序 (Efraimidis-Spirakis 算法)
/// weight 全相等时退化为均匀随机 ≈ round-robin 效果
fn weighted_shuffle<'a>(targets: &[&'a RouteTarget]) -> Vec<&'a RouteTarget> {
    if targets.is_empty() {
        return vec![];
    }
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let mut items: Vec<(&RouteTarget, f64)> = targets.iter()
        .map(|t| {
            let w = t.weight.max(1) as f64;
            let key = rng.gen::<f64>().powf(1.0 / w);
            (*t, key)
        })
        .collect();
    items.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    items.into_iter().map(|(t, _)| t).collect()
}
```

**示例**：

```
targets:
  - Anthropic  weight=70
  - OpenAI     weight=30

流量分布:
  ~70% 请求先尝试 Anthropic，失败后尝试 OpenAI
  ~30% 请求先尝试 OpenAI，失败后尝试 Anthropic
```

### 8.4 priority 策略（主备分级）

按 priority 分级，始终优先主目标。weight 字段被忽略。

```rust
impl TargetSelector {
    fn priority_select<'a>(
        &self,
        targets: &'a [RouteTarget],
        health: &HealthRegistry,
    ) -> Vec<&'a RouteTarget> {
        // 按 priority 分组
        let mut groups: BTreeMap<i32, Vec<&RouteTarget>> = BTreeMap::new();
        for t in targets {
            groups.entry(t.priority).or_default().push(t);
        }

        let mut ordered = Vec::new();
        for (_priority, group) in groups {
            // 同 priority 内：健康的在前，unhealthy 在后
            let (healthy, sick): (Vec<_>, Vec<_>) = group.into_iter()
                .partition(|t| health.is_healthy(&t.id));
            ordered.extend(healthy);
            ordered.extend(sick);
        }
        ordered
    }
}
```

**示例**：

```
targets:
  - Anthropic  priority=1 (主)
  - OpenAI     priority=1 (主)
  - DeepSeek   priority=2 (备用)

正常:  [Anthropic, OpenAI] → DeepSeek 不会被用到
Anthropic 挂了: [OpenAI, Anthropic(探活)] → DeepSeek 仍不用
两个主都挂了: [Anthropic(探活), OpenAI(探活), DeepSeek] → 自动降级
```

### 8.5 设计要点

- 两种策略都返回**完整候选序列**，调用方遍历尝试，天然实现 failover。
- unhealthy target 不跳过，排到同组末尾给半开探活机会。
- 新增策略只需在 `selector.rs` 加方法 + 枚举变体，其他模块不动。

---

## 九、健康检查与熔断

### 9.1 HealthRegistry

```rust
// router/health.rs（新增）

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

const DEFAULT_FAILURE_THRESHOLD: u32 = 3;
const DEFAULT_RECOVERY_SECS: u64 = 30;

pub struct HealthRegistry {
    states: RwLock<HashMap<String, TargetHealth>>,
    failure_threshold: u32,
    recovery_after: Duration,
}

struct TargetHealth {
    consecutive_failures: u32,
    last_failure_at: Option<Instant>,
}

impl HealthRegistry {
    pub fn new() -> Self {
        Self {
            states: RwLock::new(HashMap::new()),
            failure_threshold: DEFAULT_FAILURE_THRESHOLD,
            recovery_after: Duration::from_secs(DEFAULT_RECOVERY_SECS),
        }
    }

    pub fn is_healthy(&self, target_id: &str) -> bool {
        let states = self.states.read().unwrap();
        match states.get(target_id) {
            None => true,
            Some(h) => {
                if h.consecutive_failures < self.failure_threshold {
                    return true;
                }
                // 半开: 超时后允许一次试探
                h.last_failure_at
                    .map(|t| t.elapsed() >= self.recovery_after)
                    .unwrap_or(true)
            }
        }
    }

    pub fn record_success(&self, target_id: &str) {
        let mut states = self.states.write().unwrap();
        if let Some(entry) = states.get_mut(target_id) {
            entry.consecutive_failures = 0;
        }
    }

    pub fn record_failure(&self, target_id: &str) {
        let mut states = self.states.write().unwrap();
        let entry = states.entry(target_id.to_string()).or_insert(TargetHealth {
            consecutive_failures: 0,
            last_failure_at: None,
        });
        entry.consecutive_failures += 1;
        entry.last_failure_at = Some(Instant::now());
    }
}
```

### 9.2 设计要点

- **纯内存**，不持久化。进程重启后所有目标视为健康。
- **被动检测**：只在请求失败时更新状态，不做定时 ping。
- **半开恢复**：连续失败 3 次标记 unhealthy，30 秒后允许一个请求探活，成功则恢复。
- 粒度是 `target_id`，不是 `provider_id`。

---

## 十、Handler 重构（核心路径）

### 10.1 当前流程

```
请求 → 解码 → match_route → 取唯一 provider → 编码 → 转发 → 返回
```

### 10.2 新流程

```
请求 → 解码 → match_route(返回 RouteWithTargets)
                 ↓
         select_ordered(targets, health)  ← 按策略排序
                 ↓
         ┌── for target in ordered ──┐
         │  取 provider               │
         │  编码(target.model)        │
         │  转发                      │
         │  成功 → record_success ────│──→ 返回响应
         │  可重试? → record_failure  │
         │  继续下一个 target         │
         │  不可重试 → 直接返回错误   │
         └────────────────────────────┘
                 ↓ (所有 target 失败)
         返回最后一个错误
```

### 10.3 伪代码

```rust
async fn proxy_pipeline(/* ... */) -> Response {
    // 1. 解码、提取 model
    let internal = decoder.decode_request(body)?;
    let request_model = &internal.model;

    // 2. 路由匹配（返回 RouteWithTargets）
    let route_entry = {
        let cache = gw.route_cache.read().await;
        cache.match_route(route_protocol, request_model).cloned()
    };
    let route_entry = match route_entry {
        Some(r) => r,
        None => return error_response(404, "no route for model"),
    };

    // 3. 鉴权（不变）
    if route_entry.route.access_control {
        authorize_route_access(/* ... */)?;
    }

    // 4. 按策略获取候选目标序列
    let candidates = gw.target_selector
        .select_ordered(&route_entry, &gw.health_registry);

    if candidates.is_empty() {
        return error_response(503, "no active targets for route");
    }

    // 5. 遍历尝试
    let mut last_error: Option<Response> = None;

    for target in &candidates {
        let provider = match get_provider(&access_store, &target.provider_id).await {
            Ok(p) => p,
            Err(_) => continue,
        };

        let actual_model = if target.model.is_empty() || target.model == "*" {
            request_model.clone()
        } else {
            target.model.clone()
        };

        match try_upstream(&gw, &provider, &actual_model, &internal, stream).await {
            Ok(response) => {
                gw.health_registry.record_success(&target.id);
                return response;
            }
            Err(e) if is_retryable(e.status()) => {
                gw.health_registry.record_failure(&target.id);
                last_error = Some(e.into_response());
                continue;  // 换下一个 target
            }
            Err(e) => {
                // 不可重试错误，直接返回
                return e.into_response();
            }
        }
    }

    last_error.unwrap_or_else(|| error_response(502, "all targets exhausted"))
}
```

### 10.4 可重试判定

```rust
fn is_retryable(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 529)
}
```

不可重试（直接返回客户端，不换 target）：
- `400` — 参数错误（换 target 也会报同样的错）
- `401` — 认证失败
- `403` — 权限拒绝
- `404` — 模型不存在

### 10.5 流式请求

**一旦开始向客户端发送 SSE 数据，就无法切换 target**（HTTP 语义限制）。

Failover 只在**建立连接阶段**生效：
- 连接超时 / 连接被拒 → 可重试，换 target
- 已开始接收流数据后中断 → 无法恢复，返回错误

---

## 十一、Gateway 结构体变更

```rust
pub struct Gateway {
    pub route_cache: Arc<RwLock<RouteCache>>,
    pub target_selector: TargetSelector,      // 新增
    pub health_registry: Arc<HealthRegistry>, // 新增
    // ... 其余不变
}
```

---

## 十二、Admin API 变更

### 12.1 路由 CRUD

| 端点 | 变化 |
|------|------|
| `POST /api/routes` | Body 增加 `strategy`、`targets[]` |
| `PUT /api/routes/:id` | Body 可选 `strategy`、`targets[]`（targets 提供时全量替换） |
| `GET /api/routes` | 响应每条 route 附带 `strategy`、`targets[]` |
| `GET /api/routes/:id` | 响应附带 `strategy`、`targets[]` |
| `DELETE /api/routes/:id` | 不变（`ON DELETE CASCADE` 自动清理 targets） |

### 12.2 请求示例

**weighted 模式**：

```json
{
  "name": "GPT-4o 分流",
  "ingress_protocol": "openai",
  "virtual_model": "gpt-4o",
  "strategy": "weighted",
  "targets": [
    { "provider_id": "pid-anthropic", "model": "claude-sonnet-4-5", "weight": 70 },
    { "provider_id": "pid-openai",    "model": "gpt-4o",            "weight": 30 }
  ]
}
```

**priority 模式**：

```json
{
  "name": "GPT-4o 高可用",
  "ingress_protocol": "openai",
  "virtual_model": "gpt-4o",
  "strategy": "priority",
  "targets": [
    { "provider_id": "pid-anthropic", "model": "claude-sonnet-4-5", "priority": 1 },
    { "provider_id": "pid-openai",    "model": "gpt-4o",            "priority": 1 },
    { "provider_id": "pid-deepseek",  "model": "deepseek-chat",     "priority": 2 }
  ]
}
```

### 12.3 响应示例

```json
{
  "id": "route-xxx",
  "name": "GPT-4o 高可用",
  "ingress_protocol": "openai",
  "virtual_model": "gpt-4o",
  "strategy": "priority",
  "access_control": false,
  "is_active": true,
  "created_at": "2026-03-24T12:00:00Z",
  "targets": [
    {
      "id": "rt-001",
      "provider_id": "pid-anthropic",
      "provider_name": "Anthropic",
      "model": "claude-sonnet-4-5",
      "weight": 100,
      "priority": 1
    },
    {
      "id": "rt-002",
      "provider_id": "pid-openai",
      "provider_name": "OpenAI",
      "model": "gpt-4o",
      "weight": 100,
      "priority": 1
    },
    {
      "id": "rt-003",
      "provider_id": "pid-deepseek",
      "provider_name": "DeepSeek",
      "model": "deepseek-chat",
      "weight": 100,
      "priority": 2
    }
  ]
}
```

### 12.4 校验规则

| 规则 | 说明 |
|------|------|
| `targets` 至少 1 条 | 不允许空目标路由 |
| `strategy` ∈ {"weighted", "priority"} | 非法值拒绝 |
| `provider_id` 必须存在 | 引用完整性 |
| `weight` ≥ 1 | 0 权重无意义 |
| `priority` ∈ {1, 2} | 当前仅两级 |
| 同 route 内不允许 `(provider_id, model)` 重复 | 避免误配 |

### 12.5 向后兼容

| 场景 | 处理 |
|------|------|
| 旧客户端发来 `target_provider` + `target_model` | Admin 层自动包装为 `targets: [{ provider_id, model }]`，strategy 默认 `weighted` |
| 旧路由数据 | 迁移脚本自动创建 `route_targets` 记录（§4.3） |
| API 响应 | `target_provider` / `target_model` 继续返回（取第一个 target），两个版本后废弃 |
| 导入/导出 | `ExportRoute` 保留旧字段 + 新增 `strategy` / `targets`；导入时两种格式均支持 |

---

## 十三、前端 UI 变更

### 13.1 路由表单

```
路由名称:    [GPT-4o 高可用              ]
接入协议:    [OpenAI ▼]
虚拟模型:    [gpt-4o                     ]
负载策略:    [主备分级 ▼]

── 目标列表 ──────────────────────────────────────────
┌───┬──────────────┬──────────────────┬────────┐
│ # │ Provider     │ 模型             │ 级别   │     ← priority 模式显示级别
├───┼──────────────┼──────────────────┼────────┤
│ 1 │ Anthropic ▼  │ claude-sonnet ▼  │ 主 ▼   │
│ 2 │ OpenAI ▼     │ gpt-4o ▼         │ 主 ▼   │
│ 3 │ DeepSeek ▼   │ deepseek-chat ▼  │ 备用 ▼ │
└───┴──────────────┴──────────────────┴────────┘
                                      [+ 添加目标]
```

策略切换后：

```
负载策略:    [加权轮询 ▼]

── 目标列表 ──────────────────────────────────────────
┌───┬──────────────┬──────────────────┬──────┐
│ # │ Provider     │ 模型             │ 权重 │     ← weighted 模式显示权重
├───┼──────────────┼──────────────────┼──────┤
│ 1 │ Anthropic ▼  │ claude-sonnet ▼  │  70  │
│ 2 │ OpenAI ▼     │ gpt-4o ▼         │  30  │
└───┴──────────────┴──────────────────┴──────┘
                                      [+ 添加目标]
```

### 13.2 TypeScript 类型

```typescript
// lib/types.ts

type RouteStrategy = 'weighted' | 'priority';

interface Route {
  id: string;
  name: string;
  ingress_protocol: string;
  virtual_model: string;
  strategy: RouteStrategy;
  access_control: boolean;
  is_active: boolean;
  created_at: string;
  targets: RouteTarget[];
  // 向后兼容（deprecated）
  target_provider?: string;
  target_model?: string;
}

interface RouteTarget {
  id: string;
  provider_id: string;
  provider_name?: string;
  model: string;
  weight: number;
  priority: number;
}

interface CreateRoute {
  name: string;
  ingress_protocol: string;
  virtual_model: string;
  strategy?: RouteStrategy;
  access_control?: boolean;
  targets: CreateRouteTarget[];
}

interface CreateRouteTarget {
  provider_id: string;
  model: string;
  weight?: number;
  priority?: number;
}
```

### 13.3 交互细节

| 策略 | 显示列 | 隐藏列 | 备注 |
|------|--------|--------|------|
| `weighted` | 权重 | 级别 | 权重旁显示百分比（如 "70" → "~70%"） |
| `priority` | 级别 | 权重 | 备用行用浅色/虚线区分 |

- 只有一个 target 时，权重和级别列均隐藏
- 策略切换时，目标列表保持不变，仅切换显示列

---

## 十四、日志记录

现有 `RequestLog` 增加字段：

```sql
ALTER TABLE request_logs ADD COLUMN target_id TEXT;
ALTER TABLE request_logs ADD COLUMN attempts INTEGER DEFAULT 1;
```

可分析：
- 每个 target 的命中率和成功率
- fallback 触发频率
- 平均尝试次数

---

## 十五、影响范围汇总

| 模块 | 文件 | 变更类型 |
|------|------|----------|
| **移除 MongoDB** | `storage/mongo/*`, `config.rs`, `lib.rs`, `main.rs`, `Cargo.toml` | 删除 |
| 数据模型 | `db/models.rs` | 修改 Route，新增 RouteTarget、RouteWithTargets 等 |
| 存储 trait | `storage/traits.rs` 或 `storage/mod.rs` | 新增 RouteTargetStore，修改 RouteSnapshotStore |
| SQLite | `db/mod.rs`, `storage/sqlite/mod.rs` | 建表、迁移、CRUD |
| PostgreSQL | `storage/postgres/mod.rs` | 建表、迁移、CRUD |
| 路由缓存 | `router/matcher.rs`, `router/mod.rs` | RouteCache 改用 RouteWithTargets |
| 目标选择 | `router/selector.rs`（新增） | TargetSelector + weighted/priority 实现 |
| 健康检查 | `router/health.rs`（新增） | HealthRegistry |
| 代理 handler | `proxy/handler.rs` | 核心路径从单 target 改为循环尝试 |
| Gateway | `proxy/mod.rs` 或 `proxy/server.rs` | 新增 TargetSelector、HealthRegistry |
| Admin | `admin/mod.rs` | 路由 CRUD 编排、校验、迁移、向后兼容 |
| 前端类型 | `webui/src/lib/types.ts` | Route + RouteTarget 类型 |
| 前端 API | `webui/src/lib/backend.ts` | 请求/响应格式 |
| 前端页面 | `webui/src/pages/routes.tsx` | 策略选择 + 目标列表表单 |
| 文档 | `README.md`, `README_CN.md` | 移除 MongoDB，更新路由功能 |

---

## 十六、实施阶段

```
Phase 0: 移除 MongoDB (1 天)
├── 删除 storage/mongo/ 目录
├── 清理 config / lib / main / Cargo.toml 中的 Mongo 引用
└── 更新 README 和 smoke 脚本

Phase 1: 数据层 (2-3 天)
├── RouteTarget / RouteWithTargets / RouteStrategy 模型
├── Route 表加 strategy 列
├── route_targets 建表 + 索引（3 个后端）
├── 数据迁移脚本（幂等）
├── RouteTargetStore trait + 3 个实现
└── RouteSnapshotStore 改为返回 RouteWithTargets

Phase 2: 路由引擎 (1-2 天)
├── router/selector.rs — TargetSelector
│   ├── weighted_select + weighted_shuffle
│   └── priority_select
├── router/health.rs — HealthRegistry
├── RouteCache 重构
└── Gateway 结构体集成

Phase 3: Handler 重构 (2-3 天)
├── proxy_pipeline 循环尝试逻辑
├── 可重试错误判定
├── 流式请求 failover 边界
├── RequestLog 扩展 target_id / attempts
└── 端到端测试

Phase 4: Admin + 前端 (2-3 天)
├── Admin CRUD 编排 + 校验
├── API 向后兼容层
├── 前端 Route 类型 + 策略选择
├── 前端目标列表表单（按策略切换显示列）
├── 导入/导出兼容
└── UI 交互细节

总计: 8-12 天
```

---

## 十七、开放问题

| # | 问题 | 倾向 |
|---|------|------|
| 1 | weight=0 是否合法？ | **否**，最小为 1 |
| 2 | priority 是否需要超过 2 级？ | **不需要**，两级覆盖绝大多数场景 |
| 3 | 流式请求中途失败是否 fallback？ | **否**，已开始发送 SSE 就无法切换 |
| 4 | 同 provider 不同 model 是否允许同时出现？ | **允许** |
| 5 | 旧 `target_provider` / `target_model` 列何时物理删除？ | 两个版本后 |
| 6 | 是否需要暴露 target 健康状态？ | V1 不暴露 |
| 7 | weighted 模式切到 priority 模式，已有 target 的 weight/priority 怎么处理？ | 保留原值不清零，策略只决定"看哪个字段" |
