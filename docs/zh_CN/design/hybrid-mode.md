# NYRO Hybrid Mode 设计文档

> 版本: v3.0
> 日期: 2026-02-11
> 状态: **设计完成，待实施**

---

## 目录

1. [概述](#1-概述)
2. [实施路线图](#2-实施路线图)
3. [部署模式](#3-部署模式)
4. [Admin API 设计](#4-admin-api-设计)
5. [YAML Adapter 写入支持](#5-yaml-adapter-写入支持)
6. [可观测插件](#6-可观测插件)
7. [CP/DP 架构](#7-cpdp-架构)
8. [Console 控制台](#8-console-控制台)
9. [配置变更](#9-配置变更)
10. [附录](#10-附录)

---

## 1. 概述

### 项目演进

| 阶段 | 内容 | 状态 |
|------|------|------|
| **Phase 1: DB-less** | 声明式 YAML 配置、Rust 路由引擎 (matchit)、Store 抽象层 | ✅ 已完成 |
| **Phase 2: AI Proxy** | LLM 协议转换 (3x3)、key-auth 增强、Rust FFI llm-converter | ✅ 已完成 |
| **Phase 3: Hybrid** | Admin API、可观测插件、Console、CP/DP 分布式 | 📋 本文档 |

### 设计原则

- **同一二进制** — Nyro 只有一个可执行文件，通过配置决定角色 (standalone / cp / dp)
- **插件化可观测** — 指标和日志通过全局插件收集，不与部署模式耦合
- **零外部依赖** — 个人用户单节点部署无需任何数据库，`nyro start` 即可使用全部功能
- **渐进式增强** — standalone → standalone + Admin API → CP/DP 分布式，逐步升级

---

## 2. 实施路线图

```
Phase 3.1: Admin API + YAML R/W + 热更新
    │
    ▼
Phase 3.2: local-metrics / local-logs 可观测插件
    │
    ▼
Phase 3.3: Console 控制台前端
    │
    ▼
Phase 3.4: CP/DP + MongoDB + WebSocket 分布式
```

### Phase 3.1: Admin API + YAML 读写 + 热更新

| 模块 | 说明 |
|------|------|
| Admin API Router | RESTful CRUD，覆盖全部 6 种资源 |
| YAML Adapter 写入 | Admin API 写入后回写 config.yaml |
| 热更新机制 | version 递增 → coordinator_sync 检测 → worker events 广播 → 各子系统 rebuild |
| nginx 模板 | admin server 块已预留，启用 `admin.enabled: true` 即可 |

**v1 范围决策:**

| 决策项 | v1 方案 | 后续优化 |
|--------|---------|---------|
| API 鉴权 | 不实现，Admin 端口仅内网暴露 | v2 加 `auth_token` 校验 |
| HTTP 方法 | GET / POST / PUT / DELETE | v2 加 PATCH (部分更新) |
| 引用校验 | 写入时校验依赖资源是否存在，不存在则拒绝 | — |
| 热更新粒度 | 全量 rebuild (route/backend/consumer/plugin 全部重建) | v2 按资源类型精细化 rebuild |
| 手动热加载 | 保留 `POST /config/reload` 端点 (用户手动编辑 YAML 后触发) | — |
| 写入串行化 | privileged agent (worker 0) 唯一写入者 | — |

### Phase 3.2: 可观测插件

| 插件 | 端点 | 数据源 |
|------|------|--------|
| `local-metrics` | `/nyro/local/metrics` (JSON) | `lua_shared_dict` 聚合 |
| `local-logs` | `/nyro/local/logs` (JSON) | `access.log` 尾读 + 环形缓冲 |

与已有 `/nyro/prometheus/metrics` (Prometheus text) 共存，读同一份 shared_dict。

### Phase 3.3: Console 控制台

| 项目 | 说明 |
|------|------|
| 技术栈 | React / Vue (待定) |
| 数据源 | Admin API (配置 CRUD) + `/nyro/local/metrics` + `/nyro/local/logs` |
| 部署 | 静态文件内嵌 Nyro，通过 `location /nyro/console` 提供 |

### Phase 3.4: CP/DP 分布式

| 模块 | 说明 |
|------|------|
| MongoDB Adapter | CP 存储后端，实现 Store 接口 |
| WebSocket Push | CP 维护 DP 连接，配置变更时推送 |
| Sync Adapter | DP 端，接收 CP 推送并应用到本地内存 |
| 配置版本协议 | v1 全量快照，v2 增量 delta |

---

## 3. 部署模式

### 模式矩阵

| 模式 | 配置存储 | Admin API | 适用场景 |
|------|---------|-----------|---------|
| **standalone** (默认) | YAML 只读 | 无 | 纯 DB-less，GitOps |
| **standalone + Admin** | YAML 读写 | 有 | 个人用户，单节点动态管理 |
| **CP** | MongoDB | 有 | 企业，集中管控 + 推送 DP |
| **DP** | 内存 (从 CP 同步) | 无 | 企业，接收 CP 配置的代理节点 |

### 配置示例

```yaml
# nyro.yaml

# ── 个人模式 (standalone + Admin API) ──
store:
  mode: standalone
  standalone:
    config_file: conf/config.yaml
admin:
  enabled: true
  listen:
    - 11080

# ── 企业模式 CP ──
# store:
#   mode: hybrid
#   hybrid:
#     role: cp
#     mongodb:
#       uri: mongodb://localhost:27017
#       database: nyro
# admin:
#   enabled: true
#   listen:
#     - 11080

# ── 企业模式 DP ──
# store:
#   mode: hybrid
#   hybrid:
#     role: dp
#     control_plane:
#       endpoints:
#         - ws://cp-1.nyro.local:11080/nyro/sync
#       auth_token: "dp-secret-token"
#       reconnect_interval: 5
# admin:
#   enabled: false
```

### 架构图

```
┌─────────────────────────────────────────────────────────────┐
│              Personal Mode (standalone + Admin)              │
│                                                             │
│   Console ──▶ Admin API ──▶ YAML Adapter (R/W)           │
│                     │              │                         │
│                     │         config.yaml                    │
│                     │              │                         │
│                     ▼              ▼                         │
│              version bump → coordinator_sync                │
│                     → worker events → rebuild all           │
│                                                             │
│   local-metrics ──▶ /nyro/local/metrics (JSON)             │
│   local-logs   ──▶ /nyro/local/logs   (JSON)              │
│   prometheus   ──▶ /nyro/prometheus/metrics (text)         │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│              Enterprise Mode (CP/DP)                         │
│                                                             │
│   Console ──▶ Admin API (CP) ──▶ MongoDB Adapter          │
│                     │                    │                   │
│                     │           MongoDB ──┤                  │
│                     │                    │                   │
│                     ▼                    ▼                   │
│              WebSocket Push Service                         │
│                  │         │         │                       │
│                  ▼         ▼         ▼                       │
│               DP-1      DP-2      DP-N                      │
│             (Sync      (Sync      (Sync                     │
│              Adapter)   Adapter)   Adapter)                 │
└─────────────────────────────────────────────────────────────┘
```

---

## 4. Admin API 设计

### 端点总览

所有端点前缀: `/nyro/admin`

| 方法 | 路径 | v1 | 说明 |
|------|------|:--:|------|
| GET | `/routes` | ✅ | 列出所有路由 |
| GET | `/routes/{name}` | ✅ | 获取单条路由 |
| POST | `/routes` | ✅ | 创建路由 (校验 service 引用) |
| PUT | `/routes/{name}` | ✅ | 更新路由 (全量替换) |
| PATCH | `/routes/{name}` | ⏳ | 部分更新 (v2) |
| DELETE | `/routes/{name}` | ✅ | 删除路由 |
| GET | `/services` | ✅ | 列出所有服务 |
| GET | `/services/{name}` | ✅ | 获取单个服务 |
| POST | `/services` | ✅ | 创建服务 |
| PUT | `/services/{name}` | ✅ | 更新服务 |
| PATCH | `/services/{name}` | ⏳ | 部分更新 (v2) |
| DELETE | `/services/{name}` | ✅ | 删除服务 (校验无 route 引用) |
| GET | `/backends` | ✅ | 列出所有后端 |
| GET | `/backends/{name}` | ✅ | 获取单个后端 |
| POST | `/backends` | ✅ | 创建后端 |
| PUT | `/backends/{name}` | ✅ | 更新后端 |
| PATCH | `/backends/{name}` | ⏳ | 部分更新 (v2) |
| DELETE | `/backends/{name}` | ✅ | 删除后端 (校验无 service 引用) |
| GET | `/consumers` | ✅ | 列出所有消费者 |
| GET | `/consumers/{name}` | ✅ | 获取单个消费者 |
| POST | `/consumers` | ✅ | 创建消费者 |
| PUT | `/consumers/{name}` | ✅ | 更新消费者 |
| PATCH | `/consumers/{name}` | ⏳ | 部分更新 (v2) |
| DELETE | `/consumers/{name}` | ✅ | 删除消费者 |
| GET | `/plugins` | ✅ | 列出全局插件 |
| POST | `/plugins` | ✅ | 添加全局插件 |
| PUT | `/plugins/{name}` | ✅ | 更新全局插件 |
| DELETE | `/plugins/{name}` | ✅ | 删除全局插件 |
| GET | `/certificates` | ✅ | 列出所有证书 |
| GET | `/certificates/{name}` | ✅ | 获取单个证书 |
| POST | `/certificates` | ✅ | 创建证书 |
| PUT | `/certificates/{name}` | ✅ | 更新证书 |
| DELETE | `/certificates/{name}` | ✅ | 删除证书 |
| POST | `/config/reload` | ✅ | 手动触发热加载 (用户编辑 YAML 后) |
| GET | `/config/version` | ✅ | 获取当前配置版本号 |
| GET | `/status` | ✅ | 节点状态 (uptime, version, mode, connections) |

#### 引用校验规则 (v1)

| 操作 | 校验 |
|------|------|
| POST/PUT route | `service` 字段引用的 service 必须存在 |
| DELETE service | 不能存在引用该 service 的 route |
| DELETE backend | 不能存在引用该 backend 的 service |

### 请求/响应格式

**创建路由 — 请求:**

```http
POST /nyro/admin/routes HTTP/1.1
Content-Type: application/json

{
  "name": "chat-openai",
  "service": "openai",
  "paths": ["/v1/chat/completions"],
  "methods": ["POST"],
  "plugins": [
    {
      "id": "key-auth"
    },
    {
      "id": "ai-proxy",
      "config": {
        "api_key": "sk-xxxxx"
      }
    }
  ]
}
```

**响应 (成功):**

```json
{
  "code": 0,
  "message": "created",
  "data": {
    "name": "chat-openai",
    "service": "openai",
    "paths": ["/v1/chat/completions"],
    "methods": ["POST"],
    "plugins": [...]
  }
}
```

**响应 (错误):**

```json
{
  "code": 400,
  "message": "validation failed: name is required"
}
```

**列表响应:**

```json
{
  "code": 0,
  "data": {
    "total": 3,
    "items": [...]
  }
}
```

### 实现结构

```
nyro/admin/
├── init.lua          -- 路由分发 (解析 method + path → handler)
├── routes.lua        -- routes 资源 CRUD handler
├── services.lua      -- services 资源 CRUD handler
├── backends.lua      -- backends 资源 CRUD handler
├── consumers.lua     -- consumers 资源 CRUD handler
├── plugins.lua       -- plugins 资源 CRUD handler
├── certificates.lua  -- certificates 资源 CRUD handler
├── config.lua        -- reload / version 端点
└── status.lua        -- 节点状态
```

所有 handler 调用 `store` 统一接口，不直接操作底层适配器。

---

## 5. YAML Adapter 写入支持

### 现状

`store/adapter/yaml.lua` 当前只有读取能力 (load_config → parse_yaml → build_index)。

### 改造

增加写入接口，实现 Admin API → YAML 文件双向同步:

```lua
-- 新增 Store 写入接口 (store/init.lua)
function _M.put_route(name, data)     -- 创建/更新
function _M.delete_route(name)        -- 删除
function _M.put_service(name, data)
function _M.delete_service(name)
-- ... 其他 4 种资源同理

-- YAML Adapter 写入实现 (store/adapter/yaml.lua)
function _M.put_route(name, data)
    -- 1. 更新内存中 config_data.routes
    -- 2. 重建 _index
    -- 3. 递增 config_version
    -- 4. 回写 config.yaml (serialize → write file)
    -- 5. notify_watchers("update", ...)
end
```

### 热更新流程

```
Admin API PUT /routes/chat
    │
    ▼
store.put_route("chat", data)
    │
    ├── 1. 更新 config_data (内存)
    ├── 2. config_version++
    ├── 3. write config.yaml (持久化)
    └── 4. notify_watchers
              │
              ▼
    coordinator_sync 检测到 version 变化
              │
              ▼
    events.post → 广播到所有 worker
              │
              ▼
    各子系统 rebuild:
    ├── route.rebuild_router()
    ├── backend.rebuild_backends()
    ├── consumer.rebuild_consumers()
    ├── plugin.rebuild_plugins()
    └── certificate.rebuild_certificates()
```

**注意:** YAML 写入操作需要加锁 (使用 `lua_shared_dict` 实现分布式锁或 `resty.lock`)，
防止多 worker 并发写入导致文件损坏。实际上只需要在 privileged agent 中执行写入。

### 写入安全

- **原子写入**: 先写临时文件 `config.yaml.tmp`，然后 `os.rename()` 原子替换
- **备份**: 写入前备份 `config.yaml.bak`，写入失败可回滚
- **校验**: 写入前对完整配置做 `validate_config()`，校验失败拒绝写入

---

## 6. 可观测插件

### 6.1 local-metrics 插件

**作用:** 在 `log_by_lua` 阶段聚合请求指标到 `lua_shared_dict`，通过 JSON API 暴露。

**共享内存 Key 设计:**

```
metrics:total_requests           → counter
metrics:active_connections       → gauge
metrics:route:{name}:requests    → counter
metrics:route:{name}:latency_sum → counter (累计延迟 ms)
metrics:route:{name}:latency_count → counter
metrics:route:{name}:status:2xx  → counter
metrics:route:{name}:status:4xx  → counter
metrics:route:{name}:status:5xx  → counter
metrics:uptime_start             → timestamp
```

**API 端点:** `GET /nyro/local/metrics`

**响应示例:**

```json
{
  "uptime_seconds": 86400,
  "total_requests": 125000,
  "active_connections": 15,
  "routes": [
    {
      "name": "chat-openai",
      "requests": 50000,
      "qps": 12.5,
      "latency_avg_ms": 85,
      "status": {
        "2xx": 49500,
        "4xx": 400,
        "5xx": 100
      }
    }
  ]
}
```

**实现文件:**

```
nyro/plugin/local-metrics/
├── handler.lua   -- http_log 阶段写入 shared_dict
└── schema.lua    -- 配置 (shared_dict name, 保留时长等)
```

**与 Prometheus 的关系:**

两者读同一份 `lua_shared_dict` 数据:
- `/nyro/prometheus/metrics` → Prometheus text 格式 (给 Prometheus/Grafana)
- `/nyro/local/metrics` → JSON 格式 (给 Console)

### 6.2 local-logs 插件

**作用:** 维护最近 N 条请求日志的环形缓冲，通过 JSON API 暴露。

**存储方案:** `lua_shared_dict` 环形缓冲

```
logs:head   → 最新写入位置 (number)
logs:tail   → 最旧有效位置 (number)
logs:size   → 缓冲区容量 (默认 1000)
logs:{idx}  → JSON string (单条日志)
```

**日志条目格式:**

```json
{
  "timestamp": "2026-02-11T16:20:00+08:00",
  "client_ip": "127.0.0.1",
  "method": "POST",
  "uri": "/v1/chat/completions",
  "status": 200,
  "latency_ms": 85,
  "request_length": 256,
  "response_length": 1024,
  "route": "chat-openai",
  "service": "openai",
  "consumer": "app-1"
}
```

**API 端点:** `GET /nyro/local/logs?limit=50&offset=0`

**响应:**

```json
{
  "total": 1000,
  "items": [
    { "timestamp": "...", "method": "POST", ... },
    ...
  ]
}
```

**实现文件:**

```
nyro/plugin/local-logs/
├── handler.lua   -- http_log 阶段写入环形缓冲
└── schema.lua    -- 配置 (buffer_size 默认 1000)
```

### 端点注册

两个插件的 API 端点 (`/nyro/local/metrics`、`/nyro/local/logs`) 需要在 nginx 模板中注册，
与 Admin API、Prometheus 端点同级:

```nginx
# nginx_conf.lua 模板中新增
location /nyro/local/metrics {
    content_by_lua_block {
        nyro.http_local_metrics()
    }
}

location /nyro/local/logs {
    content_by_lua_block {
        nyro.http_local_logs()
    }
}
```

---

## 7. CP/DP 架构

### 7.1 整体流程

```
┌──────────────┐                    ┌──────────────┐
│     DP-1     │◄───── WebSocket ──►│              │
├──────────────┤                    │     CP       │
│     DP-2     │◄───── WebSocket ──►│              │
├──────────────┤                    │  ┌────────┐  │
│     DP-N     │◄───── WebSocket ──►│  │MongoDB │  │
└──────────────┘                    │  └────────┘  │
                                    │  ┌────────┐  │
       Console ──── HTTP ────────►│  │Admin   │  │
                                    │  │API     │  │
                                    │  └────────┘  │
                                    └──────────────┘
```

### 7.2 CP 端

**MongoDB Adapter** (`store/adapter/mongo.lua`):

实现与 YAML Adapter 相同的 Store 接口，后端替换为 MongoDB:

```lua
-- 集合映射
-- nyro.routes       → routes 资源
-- nyro.services     → services 资源
-- nyro.backends     → backends 资源
-- nyro.consumers    → consumers 资源
-- nyro.plugins      → plugins 资源
-- nyro.certificates → certificates 资源
-- nyro.meta         → { key: "version", value: N }
```

**WebSocket Push Service:**

CP 在 Admin server 上暴露 WebSocket 端点: `/nyro/sync`

```
# DP 连接时
1. DP → CP: WebSocket connect + auth_token
2. CP 验证 token
3. CP → DP: full config snapshot { type: "snapshot", version: N, data: {...} }

# 配置变更时
4. Admin API 写入 MongoDB
5. MongoDB version 递增
6. CP 检测到 version 变化
7. CP → 所有 DP: { type: "snapshot", version: N+1, data: {...} }

# 心跳
8. DP → CP: { type: "ping" }   (每 10s)
9. CP → DP: { type: "pong" }

# DP 断线重连
10. DP 检测连接断开
11. 等待 reconnect_interval 秒
12. 回到步骤 1
```

### 7.3 DP 端

**Sync Adapter** (`store/adapter/sync.lua`):

```lua
-- init: 连接 CP WebSocket
-- on_message: 收到 snapshot → 更新内存 config_data → version++
-- 各子系统的 coordinator_sync 检测到 version 变化 → rebuild

-- DP 本地不持久化，纯内存模式
-- 如果与 CP 断开，继续使用最后一次的配置
-- 可选: 落盘为 config_dump.yaml 做容灾 (DP 重启时如果 CP 不可用，从 dump 恢复)
```

### 7.4 配置推送协议

**v1: 全量快照 (Phase 3.4 实现)**

```json
{
  "type": "snapshot",
  "version": 42,
  "timestamp": "2026-02-11T16:20:00Z",
  "data": {
    "routes": [...],
    "services": [...],
    "backends": [...],
    "consumers": [...],
    "plugins": [...],
    "certificates": [...]
  }
}
```

**v2: 增量 delta (未来优化)**

```json
{
  "type": "delta",
  "version": 43,
  "base_version": 42,
  "timestamp": "2026-02-11T16:20:05Z",
  "changes": [
    { "op": "add",    "resource": "route",   "name": "new-route", "data": {...} },
    { "op": "update", "resource": "service", "name": "openai",    "data": {...} },
    { "op": "delete", "resource": "route",   "name": "old-route" }
  ]
}
```

DP 收到 delta 后:
- 检查 `base_version` 是否等于本地 version
- 匹配 → 应用增量
- 不匹配 → 请求 CP 发送全量 snapshot

### 7.5 DP 指标上报

v1 仅上报心跳和节点基本状态:

```json
{
  "type": "ping",
  "node": "dp-1",
  "version": 42,
  "uptime": 86400,
  "active_connections": 15,
  "total_requests": 125000
}
```

CP 汇总所有 DP 状态，通过 `/nyro/admin/status` 返回集群视图。

---

## 8. Console 控制台

### 数据源

| 功能 | 数据来源 |
|------|---------|
| 配置管理 (CRUD) | `Admin API /nyro/admin/*` |
| 实时指标 | `/nyro/local/metrics` (JSON, 轮询) |
| 请求日志 | `/nyro/local/logs` (JSON, 轮询) |
| Prometheus 集成 | `/nyro/prometheus/metrics` (用户自行接 Grafana) |
| 节点状态 | `/nyro/admin/status` |
| 集群视图 (CP) | `/nyro/admin/status` (含所有 DP 信息) |

### 部署

Console 为静态前端资源 (HTML/JS/CSS)，内嵌在 Nyro 中:

```nginx
location /nyro/console {
    index index.html;
    alias console/;
    try_files $uri $uri/ /index.html;
}
```

已在 `nyro/cli/generator.lua` 的 `build_admin_server` 中预留。

### 页面结构 (参考)

```
Console
├── 概览 (QPS / 延迟 / 错误率 / 活跃连接)
├── 路由管理 (列表 / 创建 / 编辑 / 删除)
├── 服务管理
├── 后端管理
├── 消费者管理
├── 插件管理
├── 证书管理
├── 请求日志 (实时滚动)
└── 节点状态 (单节点或集群视图)
```

---

## 9. 配置变更

### nyro.yaml 新增/修改项

```yaml
# ============================================================
# Admin API 配置
# ============================================================
admin:
  enabled: true                  # 启用 Admin API
  listen:
    - 11080                      # Admin 监听端口
  # auth_token: "admin-secret"   # 可选: Admin API 访问令牌

# ============================================================
# 存储模式配置
# ============================================================
store:
  mode: standalone               # standalone | hybrid

  standalone:
    config_file: conf/config.yaml
    reload_method: admin_api     # signal | admin_api

  # hybrid:
  #   role: cp                   # cp | dp
  #
  #   # CP 专用配置
  #   mongodb:
  #     uri: mongodb://localhost:27017
  #     database: nyro
  #
  #   # DP 专用配置
  #   control_plane:
  #     endpoints:
  #       - ws://cp-1.nyro.local:11080/nyro/sync
  #     auth_token: "dp-secret-token"
  #     reconnect_interval: 5    # 断线重连间隔 (秒)
  #     config_dump: conf/config_dump.yaml  # 可选: 本地容灾备份

# ============================================================
# 插件列表 (新增可观测插件)
# ============================================================
plugins:
  - cors
  - mock
  - key-auth
  - jwt-auth
  - limit-req
  - limit-conn
  - limit-count
  - ai-proxy
  - local-metrics                # 本地指标聚合
  - local-logs                   # 本地日志缓冲
```

### nginx 模板新增

```nginx
# 可观测插件端点 (与 admin server 独立，始终可用)
location /nyro/local/metrics {
    content_by_lua_block {
        nyro.http_local_metrics()
    }
}

location /nyro/local/logs {
    content_by_lua_block {
        nyro.http_local_logs()
    }
}
```

### lua_shared_dict 新增

```yaml
nginx:
  shared_dict:
    # ... 已有 ...
    local_metrics: 10m           # local-metrics 插件
    local_logs: 20m              # local-logs 插件 (环形缓冲)
```

---

## 10. 附录

### Store Adapter 接口规范

所有 adapter (yaml / mongo / sync) 必须实现以下接口:

```lua
-- 读取接口 (已有)
adapter.init(config)
adapter.get_plugins()
adapter.get_backends()
adapter.get_services()
adapter.get_routes()
adapter.get_consumers()
adapter.get_certificates()
adapter.get_version()
adapter.reload()

-- 写入接口 (新增, yaml 和 mongo 实现, sync 不实现)
adapter.put_route(name, data)
adapter.delete_route(name)
adapter.put_service(name, data)
adapter.delete_service(name)
adapter.put_backend(name, data)
adapter.delete_backend(name)
adapter.put_consumer(name, data)
adapter.delete_consumer(name)
adapter.put_plugin(name, data)
adapter.delete_plugin(name)
adapter.put_certificate(name, data)
adapter.delete_certificate(name)
```

### 文件变更清单 (预估)

**Phase 3.1: Admin API**

| 文件 | 操作 |
|------|------|
| `nyro/admin/init.lua` | 新增 — API 路由分发 |
| `nyro/admin/routes.lua` | 新增 — routes CRUD |
| `nyro/admin/services.lua` | 新增 — services CRUD |
| `nyro/admin/backends.lua` | 新增 — backends CRUD |
| `nyro/admin/consumers.lua` | 新增 — consumers CRUD |
| `nyro/admin/plugins.lua` | 新增 — plugins CRUD |
| `nyro/admin/certificates.lua` | 新增 — certificates CRUD |
| `nyro/admin/config.lua` | 新增 — reload / version |
| `nyro/admin/status.lua` | 新增 — 节点状态 |
| `nyro/store/init.lua` | 修改 — 增加写入接口 |
| `nyro/store/adapter/yaml.lua` | 修改 — 实现写入 + 原子落盘 |
| `nyro/nyro.lua` | 修改 — http_admin 调用 admin 模块 |

**Phase 3.2: 可观测插件**

| 文件 | 操作 |
|------|------|
| `nyro/plugin/local-metrics/handler.lua` | 新增 |
| `nyro/plugin/local-metrics/schema.lua` | 新增 |
| `nyro/plugin/local-logs/handler.lua` | 新增 |
| `nyro/plugin/local-logs/schema.lua` | 新增 |
| `nyro/nyro.lua` | 修改 — 新增 http_local_metrics / http_local_logs |
| `nyro/cli/templates/nginx_conf.lua` | 修改 — 新增 location 块 |
| `conf/nyro.yaml` | 修改 — 插件列表、shared_dict |

**Phase 3.4: CP/DP**

| 文件 | 操作 |
|------|------|
| `nyro/store/adapter/mongo.lua` | 新增 — MongoDB adapter |
| `nyro/store/adapter/sync.lua` | 新增 — DP sync adapter |
| `nyro/sync/init.lua` | 新增 — CP 端 WebSocket push service |
| `nyro/sync/client.lua` | 新增 — DP 端 WebSocket client |
| `nyro/nyro.lua` | 修改 — init_worker 增加 sync 模块初始化 |

### 后续优化项 (Backlog)

以下为 v1 有意简化、留待后续版本升级的项目:

| # | 优化项 | 当前 (v1) | 目标 | 优先级 |
|---|--------|-----------|------|--------|
| 1 | **Admin API 鉴权** | 无鉴权，依赖网络隔离 | `auth_token` header 校验；可选 RBAC | 高 |
| 2 | **PATCH 部分更新** | 仅 PUT 全量替换 | 支持 JSON Merge Patch (RFC 7396) | 中 |
| 3 | **热更新精细化** | 全量 rebuild 所有子系统 | 按变更的资源类型选择性 rebuild (如只改 route 不 rebuild backend) | 中 |
| 4 | **增量推送 (CP/DP)** | v1 全量 snapshot | delta 增量推送 + base_version 校验 | 高 (Phase 3.4 后) |
| 5 | **YAML 写入并发优化** | privileged agent 串行写入 | 批量合并 (debounce) 多次写入为一次文件 I/O | 低 |
| 6 | **资源版本号** | 全局 config_version | 每个资源独立 version/updated_at 字段，支持乐观锁 | 中 |
| 7 | **分页与过滤** | GET list 返回全量 | 支持 `?page=1&size=20&filter=name:chat*` | 低 |
| 8 | **批量操作** | 单条 CRUD | `POST /batch` 批量创建/更新/删除 | 低 |
| 9 | **Audit Log** | 无 | 记录 Admin API 操作历史 (谁/何时/改了什么) | 中 |
| 10 | **Import/Export** | 无 | `GET /config/export` 导出完整配置 / `POST /config/import` 导入 | 中 |

### 参考

- [Kong Hybrid Mode](https://docs.konghq.com/gateway/latest/production/deployment-topologies/hybrid-mode/) — CP/DP 分离参考
- [Apache APISIX Admin API](https://apisix.apache.org/docs/apisix/admin-api/) — Admin API 设计参考
- [lua-resty-websocket](https://github.com/openresty/lua-resty-websocket) — WebSocket 库

---

*文档结束*
