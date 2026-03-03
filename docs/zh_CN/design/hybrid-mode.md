# NYRO Hybrid Mode 设计文档

> 版本: v3.1
> 日期: 2026-02-12
> 状态: **Phase 3.1 已实施，Phase 3.2 设计完成待实施**

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

| 模块 | 说明 |
|------|------|
| 全局插件执行 | 增强 `run_plugin`，支持 config.yaml 顶层 `plugins` 作为全局插件执行 |
| `local-metrics` | `http_log` 阶段写入 `plugin_local_metrics` shared_dict，三维度 (route/service/consumer) |
| `local-logs` | `http_log` 阶段赋值 `ngx.var` 自定义变量，nginx `log_format` 输出独立 JSON 日志，API 尾读 |

**v1 范围决策:**

| 决策项 | v1 方案 | 后续优化 |
|--------|---------|---------|
| 全局插件 | `run_plugin` 在 route/service 插件之后追加全局插件执行 | — |
| 指标维度 | route + service + consumer 三维度 | — |
| QPS | 不计算，Console 前端按轮询间隔自行算 | Timer 采样或 Rust FFI |
| AI Token 统计 | ai-proxy 从上游 `usage` 提取，local-metrics/local-logs 聚合输出 | — |
| active_connections | nginx 原生 `connections_active` | — |
| shared_dict | `plugin_local_metrics` 独立 | — |
| 日志文件 | 独立 `logs/access.json`，保留原有 `access.log` 不变 | — |
| 日志 Lua 字段 | 通过 `ngx.var` 自定义变量 ($nyro_route 等) 注入 log_format | — |

### Phase 3.3: Console 控制台

| 项目 | 说明 |
|------|------|
| 技术栈 | Vite + React 19 + TypeScript |
| UI | Tailwind CSS v4 + Shadcn UI (Radix) + Lucide React |
| 数据层 | TanStack Query v5 (API 请求/缓存) + Zustand (全局状态) + React Router v7 |
| 图表 | Recharts |
| 数据源 | Admin API `/nyro/admin/*` (配置 CRUD) + `/nyro/local/metrics` + `/nyro/local/logs` |
| 源码目录 | `webui/` (构建产物 `webui/dist/`) |
| 部署 | Admin Server (11080) `/` 路径，nginx alias `admin.console.ui/` |
| API 代理 | Admin Server 增加 `/nyro/local/` 反向代理到 proxy server，前端统一对接 11080 |
| 视觉风格 | Apple 风格 Light 主题 + 毛玻璃效果，Inter 字体 |

**开发步骤：**

| 步骤 | 内容 | 说明 |
|------|------|------|
| Step 1 | 脚手架 + 基础配置 | Vite 项目初始化、Tailwind/Shadcn 配置、API Client 封装 |
| Step 2 | 布局骨架 + 主题 | 毛玻璃侧边栏、顶栏、路由框架，确认视觉效果 |
| Step 3 | Dashboard 页 | 对接 `/status` + `/local/metrics`，KPI 卡片 + 图表 |
| Step 4 | 资源管理页 | Routes / Services / Backends / Consumers / Plugins / Certificates CRUD |
| Step 5 | 可观测页 | Traffic Logs (日志查看) + Traffic Stats (API/AI 双视图指标看板) |
| Step 6 | Nginx 模板更新 | Admin Server 路径调整 + API 反向代理 |

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
│                                                             │
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

### 6.0 前置依赖: 全局插件执行机制

**现状:**

`nyro.yaml` 中的 `plugins` 列表负责**加载**插件 handler 模块。
`config.yaml` 中的顶层 `plugins` 为**全局插件实例**（与 services、routes 同级）。

当前 `run_plugin()` 只执行 route 级和 service 级插件，**不执行全局插件**。
`local-metrics` 和 `local-logs` 作为全局插件，需要对所有请求生效。

**改造方案:**

在 `run_plugin()` 中追加第三轮遍历——全局插件执行（在 route/service 插件之后）:

```lua
-- nyro/nyro.lua run_plugin() 伪码
function run_plugin(phase, oak_ctx)
    -- 1. 执行 route 级插件
    for _, p in ipairs(router_plugins) do ... end

    -- 2. 执行 service 级插件 (跳过已在 route 执行过的)
    for _, p in ipairs(service_plugins) do ... end

    -- 3. 执行全局插件 (从 store.get_plugins() 获取)
    local global_plugins = store.get_plugins()
    for _, gp in ipairs(global_plugins) do
        local handler = plugin_objects[gp.id or gp.name]
        if handler and handler.handler[phase] then
            handler.handler[phase](oak_ctx, gp.config or {})
        end
    end
end
```

全局插件**始终执行**，不受路由匹配影响。如果某请求未命中任何路由（404），
全局插件仍会在 `http_log` 阶段执行（前提: 404 也走 log 阶段）。

**config.yaml 全局插件配置示例:**

```yaml
# config.yaml
plugins:
  - id: local-metrics
  - id: local-logs

routes:
  - name: chat-openai
    service: openai
    paths: [/v1/chat/completions]
    plugins:           # route 级插件
      - id: key-auth
      - id: ai-proxy
```

### 6.1 local-metrics 插件

**作用:** 在 `http_log` 阶段聚合请求指标到 `lua_shared_dict plugin_local_metrics`，
通过 JSON API 暴露给 Console。面向个人用户 / 单节点本地观测，不计算 QPS。

**共享内存 Key 设计 (三维度: route / service / consumer):**

```
# ── 全局 ──
m:total_requests                    → counter
m:uptime_start                      → timestamp (init_worker 时写入)

# ── 按 route ──
m:rt:{name}:requests                → counter
m:rt:{name}:latency_sum             → counter (累计延迟 ms)
m:rt:{name}:latency_count           → counter
m:rt:{name}:status:2xx              → counter
m:rt:{name}:status:4xx              → counter
m:rt:{name}:status:5xx              → counter

# ── 按 service ──
m:svc:{name}:requests               → counter
m:svc:{name}:latency_sum            → counter
m:svc:{name}:latency_count          → counter
m:svc:{name}:status:2xx             → counter
m:svc:{name}:status:4xx             → counter
m:svc:{name}:status:5xx             → counter

# ── 按 consumer ──
m:cs:{name}:requests                → counter
m:cs:{name}:status:2xx              → counter
m:cs:{name}:status:4xx              → counter
m:cs:{name}:status:5xx              → counter
```

> **说明:** `active_connections` 从 nginx 内置变量 `ngx.var.connections_active` 读取，不写入 shared_dict。

**handler.lua http_log 阶段逻辑:**

```lua
function _M.http_log(oak_ctx, plugin_config)
    local dict = ngx.shared.plugin_local_metrics
    local status = ngx.status
    local request_time = tonumber(ngx.var.request_time) or 0
    local latency_ms = request_time * 1000

    -- 状态码分桶
    local status_bucket
    if status >= 200 and status < 300 then
        status_bucket = "2xx"
    elseif status >= 400 and status < 500 then
        status_bucket = "4xx"
    else
        status_bucket = "5xx"
    end

    -- 全局
    dict:incr("m:total_requests", 1, 0)

    -- route 维度
    local route_name = oak_ctx.config and oak_ctx.config.route and oak_ctx.config.route.name
    if route_name then
        dict:incr("m:rt:" .. route_name .. ":requests", 1, 0)
        dict:incr("m:rt:" .. route_name .. ":latency_sum", latency_ms, 0)
        dict:incr("m:rt:" .. route_name .. ":latency_count", 1, 0)
        dict:incr("m:rt:" .. route_name .. ":status:" .. status_bucket, 1, 0)
    end

    -- service 维度
    local service_name = oak_ctx.config and oak_ctx.config.service and oak_ctx.config.service.name
    if service_name then
        dict:incr("m:svc:" .. service_name .. ":requests", 1, 0)
        dict:incr("m:svc:" .. service_name .. ":latency_sum", latency_ms, 0)
        dict:incr("m:svc:" .. service_name .. ":latency_count", 1, 0)
        dict:incr("m:svc:" .. service_name .. ":status:" .. status_bucket, 1, 0)
    end

    -- consumer 维度
    local consumer_name = oak_ctx._consumer and oak_ctx._consumer.name or "anonymous"
    dict:incr("m:cs:" .. consumer_name .. ":requests", 1, 0)
    dict:incr("m:cs:" .. consumer_name .. ":status:" .. status_bucket, 1, 0)
end
```

**API 端点:** `GET /nyro/local/metrics`

放置在主 server block 中，始终可用，无需 Admin 开启。

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
      "latency_avg_ms": 85,
      "status": { "2xx": 49500, "4xx": 400, "5xx": 100 }
    }
  ],
  "services": [
    {
      "name": "openai",
      "requests": 80000,
      "latency_avg_ms": 90,
      "status": { "2xx": 79000, "4xx": 700, "5xx": 300 }
    }
  ],
  "consumers": [
    {
      "name": "ai-app",
      "requests": 60000,
      "status": { "2xx": 59500, "4xx": 300, "5xx": 200 }
    },
    {
      "name": "anonymous",
      "requests": 65000,
      "status": { "2xx": 64000, "4xx": 800, "5xx": 200 }
    }
  ]
}
```

> Console 前端可按轮询间隔对 `total_requests` 做差值计算实时 QPS。

**实现文件:**

```
nyro/plugin/local-metrics/
├── handler.lua   -- http_log 阶段写入 shared_dict + init_worker 写入 uptime_start
├── schema.lua    -- 配置 schema (预留扩展)
└── api.lua       -- /nyro/local/metrics 读取 shared_dict 构造 JSON 响应
```

local-metrics 使用独立的 shared_dict `plugin_local_metrics` (JSON 格式，给 Console)。

### 6.2 local-logs 插件

**作用:** 在 `http_log` 阶段将 Lua 层面的上下文信息（route/service/consumer）
写入 `ngx.var` 自定义变量，由 nginx `log_format nyro_json` 输出到独立的 JSON 日志文件
`logs/access.json`。API 端点尾读该文件返回最近 N 条日志。

**原有 `logs/access.log`（text 格式）保持不变。**

**设计思路:**

```
请求流经 location / 的各阶段:
  access_by_lua  → route match, plugin 执行
  ...
  log_by_lua     → local-logs handler 将 oak_ctx 信息写入 ngx.var
                 → nginx 用 log_format nyro_json 输出到 logs/access.json
                 → 原有 log_format main 继续输出到 logs/access.log
```

**handler.lua http_log 阶段逻辑:**

```lua
function _M.http_log(oak_ctx, plugin_config)
    -- 将 Lua 层面的上下文信息写入 ngx.var，供 log_format 引用
    local route_name = oak_ctx.config and oak_ctx.config.route and oak_ctx.config.route.name
    local service_name = oak_ctx.config and oak_ctx.config.service and oak_ctx.config.service.name
    local consumer_name = oak_ctx._consumer and oak_ctx._consumer.name

    ngx.var.nyro_route    = route_name or ""
    ngx.var.nyro_service  = service_name or ""
    ngx.var.nyro_consumer = consumer_name or ""
end
```

**nginx 模板新增:**

```nginx
# ── 自定义变量 (在 location / 中声明) ──
set $nyro_route    '';
set $nyro_service  '';
set $nyro_consumer '';

# ── JSON 日志格式 (在 http 块中声明) ──
log_format nyro_json escape=json '{'
    '"timestamp":"$time_iso8601",'
    '"client_ip":"$remote_addr",'
    '"method":"$request_method",'
    '"uri":"$uri",'
    '"status":$status,'
    '"latency_ms":$request_time,'
    '"request_length":$request_length,'
    '"response_length":$bytes_sent,'
    '"upstream_addr":"$upstream_addr",'
    '"upstream_status":"$upstream_status",'
    '"request_id":"$request_id",'
    '"route":"$nyro_route",'
    '"service":"$nyro_service",'
    '"consumer":"$nyro_consumer"'
'}';

# ── 双日志输出 (在 location / 中) ──
access_log logs/access.log main;                      # 原有不变
access_log logs/access.json nyro_json;       # local-logs 专用
```

**日志条目格式:**

```json
{
  "timestamp": "2026-02-11T16:20:00+08:00",
  "client_ip": "127.0.0.1",
  "method": "POST",
  "uri": "/v1/chat/completions",
  "status": 200,
  "latency_ms": 0.085,
  "request_length": 256,
  "response_length": 1024,
  "upstream_addr": "api.deepseek.com:443",
  "upstream_status": "200",
  "request_id": "abc123",
  "route": "chat-openai",
  "service": "openai",
  "consumer": "ai-app"
}
```

**API 端点:** `GET /nyro/local/logs?limit=50`

放置在主 server block 中，始终可用。

**尾读实现 (api.lua):**

```lua
-- 从文件末尾反向读取最近 N 行
-- 1. io.open(log_path, "r")
-- 2. file:seek("end", -read_size)  -- 从末尾回退 read_size 字节
-- 3. 读取内容，按 \n 分割
-- 4. 取最后 limit 行，每行 json.decode 后返回
```

**响应:**

```json
{
  "total": 50,
  "items": [
    { "timestamp": "...", "method": "POST", "uri": "...", ... },
    ...
  ]
}
```

**实现文件:**

```
nyro/plugin/local-logs/
├── handler.lua   -- http_log 阶段赋值 ngx.var.nyro_*
├── schema.lua    -- 配置 schema (log_path, 预留扩展)
└── api.lua       -- /nyro/local/logs 尾读 JSON 日志文件
```

### 6.3 端点注册

两个插件的 API 端点放置在**主 server block** 中，
始终可用，不依赖 `admin.enabled` 配置:

```nginx
# nginx_conf.lua 模板 — 主 server block 内
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

`nyro.lua` 新增对应入口函数:

```lua
function NYRO.http_local_metrics()
    local api = require("nyro.plugin.local-metrics.api")
    api.serve()
end

function NYRO.http_local_logs()
    local api = require("nyro.plugin.local-logs.api")
    api.serve()
end
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

### 技术栈

| 模块 | 选型 | 说明 |
|------|------|------|
| 构建 | Vite + React 19 + TypeScript | 纯 SPA，无 SSR 需求 |
| 样式 | Tailwind CSS v4 | 最新版本 |
| UI 组件 | Shadcn UI (Radix Primitives) + Lucide React | 无头组件 + 专业图标库 |
| 图表 | Recharts | 声明式图表库 |
| 数据请求 | TanStack Query v5 | API 缓存、轮询、乐观更新 |
| 路由 | React Router v7 | SPA 路由 |
| 状态 | Zustand | 轻量全局状态 (主题、侧边栏折叠等) |

### 数据源

| 功能 | 数据来源 | 端口 |
|------|---------|------|
| 配置管理 (CRUD) | `Admin API /nyro/admin/*` | 11080 |
| 实时指标 | `/nyro/local/metrics` (JSON, 轮询) | 11080 (代理) |
| 请求日志 | `/nyro/local/logs` (JSON, 轮询) | 11080 (代理) |
| 节点状态 | `/nyro/admin/status` | 11080 |
| 集群视图 (CP) | `/nyro/admin/status` (含所有 DP 信息) | 11080 |

> Admin Server 增加 `/nyro/local/` 反向代理到 proxy server（动态取 `nginx.http_listen` 第一项端口），前端统一对接 11080 端口，避免跨域。

### 部署

Console 为静态前端资源 (HTML/JS/CSS)，源码位于 `webui/`，构建产物目录由 `admin.console.ui` 配置（开发推荐 `webui/dist`，发布可切换为 `console`）。

```nginx
# Admin Server (11080)
server {
    listen 11080;

    # Admin API
    location /nyro/admin { ... }

    # 可观测 API 反向代理 (解决跨端口问题，端口来自 nginx.http_listen[1])
    location /nyro/local/ {
        proxy_pass http://127.0.0.1:{proxy_port};
    }

    # Console SPA
    location / {
        index index.html;
        alias {admin.console.ui}/;
        try_files $uri $uri/ /index.html;
    }
}
```

### 视觉风格

| 项目 | 说明 |
|------|------|
| 基调 | Apple 风格 Light 模式，纯白 + 柔和灰色背景 |
| 毛玻璃 | 侧边栏 `backdrop-filter: blur(15px)` + `bg-white/80`，模态框/顶栏同理 |
| 字体 | Inter (300-700 weight) |
| 主色 | 蓝色强调 (`#2563EB`)，中性灰色系 |
| Logo | `webui/assets/logos/NYRO-logo.png` |
| AI 厂商图标 | `webui/assets/icons/*.svg` |

### 页面结构

```
Console
├── Dashboard          — 概览 (运行时长 / 总请求 / 活跃连接 / Token 总量 / 状态码分布)
├── Routes             — 路由管理 (列表 / 创建 / 编辑 / 删除)
├── Services           — 服务管理
├── Backends           — 后端管理
├── Consumers          — 消费者管理
├── Plugins            — 全局插件管理
├── Certificates       — SSL 证书管理
├── Traffic Logs       — 日志查看器 (筛选 / 搜索)
└── Traffic Stats      — 指标看板
     ├── API View      — route / service / consumer 维度 (请求量 / 延迟 / 状态码)
     └── AI View       — model 维度 (Token 用量 / 请求量 / 延迟)
```

### 开发步骤

| 步骤 | 内容 | 说明 |
|------|------|------|
| Step 1 | 脚手架 + 基础配置 | Vite 项目初始化、Tailwind/Shadcn 配置、API Client 封装 |
| Step 2 | 布局骨架 + 主题 | 毛玻璃侧边栏、顶栏、路由框架，确认视觉效果 |
| Step 3 | Dashboard 页 | 对接 `/status` + `/local/metrics`，KPI 卡片 + 图表 |
| Step 4 | 资源管理页 | Routes / Services / Backends / Consumers / Plugins / Certificates CRUD |
| Step 5 | 可观测页 | Traffic Logs (日志查看) + Traffic Stats (API/AI 双视图指标看板) |
| Step 6 | Nginx 模板更新 | Admin Server 路径调整 + API 反向代理 |

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
  console:
    ui: webui/dist               # Console 静态目录 (开发: webui/dist, 发布: console)
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
# 插件模块加载列表 (nyro.yaml — 声明要加载哪些插件 handler)
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
  - local-metrics                # 本地指标聚合 (全局插件)
  - local-logs                   # 本地日志上下文注入 (全局插件)
```

### nginx 模板新增

**1. http 块 — JSON 日志格式 (local-logs 专用):**

```nginx
log_format nyro_json escape=json '{'
    '"timestamp":"$time_iso8601",'
    '"client_ip":"$remote_addr",'
    '"method":"$request_method",'
    '"uri":"$uri",'
    '"status":$status,'
    '"latency_ms":$request_time,'
    '"request_length":$request_length,'
    '"response_length":$bytes_sent,'
    '"upstream_addr":"$upstream_addr",'
    '"upstream_status":"$upstream_status",'
    '"request_id":"$request_id",'
    '"route":"$nyro_route",'
    '"service":"$nyro_service",'
    '"consumer":"$nyro_consumer"'
'}';
```

**2. location / 块 — 自定义变量 + 双日志:**

```nginx
# 自定义变量声明 (供 local-logs handler 赋值)
set $nyro_route    '';
set $nyro_service  '';
set $nyro_consumer '';

# 双日志输出
access_log logs/access.log main;                 # 原有 text 格式不变
access_log logs/access.json nyro_json;  # local-logs JSON 格式
```

**3. 主 server block — 可观测端点:**

```nginx
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

### lua_shared_dict 变更

```yaml
nginx:
  shared_dict:
    # ... 已有 ...
    plugin_local_metrics: 10m          # local-metrics 插件
    # local_logs 不需要 shared_dict (使用文件存储)
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

| 文件 | 操作 | 说明 |
|------|------|------|
| `nyro/nyro.lua` | 修改 | `run_plugin` 增加全局插件执行；新增 `http_local_metrics` / `http_local_logs` |
| `nyro/plugin/local-metrics/handler.lua` | 新增 | `http_log` 阶段写入 shared_dict (三维度) |
| `nyro/plugin/local-metrics/schema.lua` | 新增 | 配置 schema |
| `nyro/plugin/local-metrics/api.lua` | 新增 | `/nyro/local/metrics` API handler |
| `nyro/plugin/local-logs/handler.lua` | 新增 | `http_log` 阶段赋值 `ngx.var.nyro_*` |
| `nyro/plugin/local-logs/schema.lua` | 新增 | 配置 schema |
| `nyro/plugin/local-logs/api.lua` | 新增 | `/nyro/local/logs` 尾读 JSON 日志 |
| `nyro/cli/templates/nginx_conf.lua` | 修改 | `log_format nyro_json`、`set $nyro_*`、双 `access_log`、两个 location |
| `conf/nyro.yaml` | 修改 | `shared_dict` 新增 `plugin_local_metrics`；`plugins` 列表新增 |

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
| 11 | **AI Token 统计** | ✅ 已实现 | ai-proxy body_filter 按 target_proto 提取上游 usage，local-metrics 聚合，local-logs 记录 | — |
| 12 | **QPS 计算** | 不实现，Console 前端差值计算 | Timer 采样或 Rust FFI 实时计算 | 低 |

### 参考

- [Kong Hybrid Mode](https://docs.konghq.com/gateway/latest/production/deployment-topologies/hybrid-mode/) — CP/DP 分离参考
- [Apache APISIX Admin API](https://apisix.apache.org/docs/apisix/admin-api/) — Admin API 设计参考
- [lua-resty-websocket](https://github.com/openresty/lua-resty-websocket) — WebSocket 库

---

*文档结束*
