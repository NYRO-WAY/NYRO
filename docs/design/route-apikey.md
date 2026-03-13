# Route 与 API Key 设计

## 一、核心概念

Route 和 API Key 是**独立管理、多对多绑定**的关系：

- 一个 Key 可以访问多个 Route（应用需要调用不同模型）
- 一个 Route 可以被多个 Key 访问（多应用/团队共用同一路由）
- Key 未绑定任何 Route 时默认全局生效

```
API Key ──── (授权绑定) ──── Route
  │                            │
  ├── 配额: RPM / TPM / TPD     ├── 接入协议 (openai / anthropic / gemini)
  ├── 过期时间                  ├── 虚拟模型名 (精确匹配)
  ├── 状态: active / revoked   ├── 目标提供商 + 目标模型
  └── 名称                      └── 访问控制开关
```

## 二、Route

### 路由匹配

路由唯一键为 `(ingress_protocol, virtual_model)`，匹配流程：

```
请求进入
  → match(ingress_protocol, virtual_model) → 找到 Route
  → if route.access_control == true:
      → 验证 API Key
      → if key 绑定了路由: 检查当前 Route 是否在绑定列表中
      → if key 未绑定任何路由: 全局通过
  → 执行路由转发
```

### 字段定义

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | TEXT PK | UUID |
| `name` | TEXT | 人类可读名称，如 "Production Claude" |
| `ingress_protocol` | TEXT | 接入协议：`openai` / `anthropic` / `gemini` |
| `virtual_model` | TEXT | 客户端传入的 model 值，精确匹配 |
| `target_provider` | TEXT FK | 目标模型提供商 |
| `target_model` | TEXT | 实际调用的模型 |
| `access_control` | BOOLEAN | 是否启用访问控制，默认 false |
| `is_active` | BOOLEAN | 路由启用状态，默认 true |
| `created_at` | TEXT | 创建时间 |

唯一约束：`UNIQUE(ingress_protocol, virtual_model)`

### 虚拟模型名继承规则

选择目标模型时自动填入虚拟模型名，用户可覆盖：

```
虚拟模型名 = 用户填写 ?? 目标模型名
```

两种使用场景：

- **直接继承（透明代理）**：虚拟模型名 = 真实模型名。客户端代码与直连模型提供商完全一致，迁移零成本。
- **自定义虚拟名（抽象层）**：虚拟模型名 ≠ 真实模型名。客户端代码固定写 `gpt-4o`，后端随时切换模型，客户端无感知。

### 路由示例

| 路由名 | 接入协议 | 虚拟模型 | 目标提供商 | 目标模型 | 访问控制 |
|---|---|---|---|---|---|
| dev-cheap | openai | gpt-4o-mini | DeepSeek | deepseek-chat | 关 |
| prod-smart | openai | gpt-4o | Anthropic | claude-sonnet-4-5-20250514 | 开 |
| prod-heavy | anthropic | claude-opus | Anthropic | claude-opus-4-0-20250514 | 开 |
| team-coding | openai | codex-mini | OpenAI | o4-mini | 开 |

## 三、API Key

### 字段定义

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | TEXT PK | UUID |
| `key` | TEXT UNIQUE | 格式 `sk-nyro-xxxx`，自动生成 |
| `name` | TEXT | 可读名称，如 "Frontend App" |
| `rpm` | INTEGER NULL | Requests Per Minute 限额，NULL = 不限 |
| `tpm` | INTEGER NULL | Tokens Per Minute 限额，NULL = 不限 |
| `tpd` | INTEGER NULL | Tokens Per Day 限额，NULL = 不限 |
| `status` | TEXT | `active` / `revoked`，默认 `active` |
| `expires_at` | TEXT NULL | 过期时间，NULL = 永不过期 |
| `created_at` | TEXT | 创建时间 |
| `updated_at` | TEXT | 更新时间 |

### 过期时间预设

创建 Key 时提供以下选项：

| 选项 | expires_at |
|---|---|
| 永不过期 | NULL |
| 1 天 | now + 1d |
| 7 天 | now + 7d |
| 30 天 | now + 30d |
| 90 天 | now + 90d |
| 180 天 | now + 180d |
| 1 年 | now + 365d |

### Key 有效性判定

```
key_valid = (status == 'active') AND (expires_at IS NULL OR now < expires_at)
```

`status` 和 `expires_at` 是独立的两个维度：

- `status` 是人为意志（手动吊销/恢复）
- `expires_at` 是时间策略（自动失效）

保留 `status` 字段的原因：Key 泄露时可一键 revoke 而不丢失配置；临时停用后可恢复。

### 配额单位

| 单位 | 含义 | 适用场景 |
|---|---|---|
| **RPM** | Requests Per Minute，每分钟请求数 | 防止高频调用、保护上游接口 |
| **TPM** | Tokens Per Minute，每分钟 Token 限额 | 控制瞬时计算资源消耗 |
| **TPD** | Tokens Per Day，每天 Token 限额 | 控制每日总成本 |

> 三者互补：RPM 防突发流量，TPM 控瞬时资源，TPD 控总成本。任一维度超限即触发 429。

### Key 使用示例

```
Key: sk-nyro-frontend-app
  ├── 绑定路由: [prod-smart, dev-cheap]  ← 仅能访问这两条路由
  └── 配额: 60 RPM / 100K TPD

Key: sk-nyro-backend-svc
  ├── 绑定路由: []                        ← 未绑定，全局生效
  └── 配额: 不限 RPM / 500K TPD

Key: sk-nyro-team-a
  ├── 绑定路由: [team-coding]             ← 仅 team-coding
  └── 配额: 30 RPM / 50K TPD
```

## 四、绑定关系

### 数据模型

```
api_key_routes (绑定关系表)
├── api_key_id  TEXT FK → api_keys.id  ON DELETE CASCADE
├── route_id    TEXT FK → routes.id    ON DELETE CASCADE
└── PRIMARY KEY(api_key_id, route_id)
```

纯关系表，无额外字段。

### 绑定语义

| Key 绑定状态 | 行为 |
|---|---|
| 未绑定任何路由 | 全局生效：可访问所有开启了访问控制的路由 |
| 绑定了特定路由 | 精准生效：仅可访问绑定列表中的路由 |

### 管理入口

- **Route 页面**：仅显示"访问控制"开关（开/关）
- **API Key 页面**：管理 Key 信息 + 绑定哪些路由

## 五、鉴权流程

### 代理请求鉴权

```
1. 解析请求 → 提取 ingress_protocol, model, api_key
2. match(ingress_protocol, model) → Route
   └── 未匹配 → 404 no route
3. if route.access_control == false:
   └── 直接放行，跳到步骤 6
4. if api_key 为空:
   └── 401 unauthorized
5. 验证 api_key:
   a. 查找 key → 不存在 → 401 invalid key
   b. status != 'active' → 403 key revoked
   c. expires_at < now → 403 key expired
   d. key 绑定了路由 AND 当前 route 不在绑定列表 → 403 forbidden
   e. 配额检查 (rpm/tpm/tpd) → 任一超限 → 429 rate limited
6. 执行路由转发 → target_provider + target_model
```

### API Key 提取位置

按以下顺序尝试提取，兼容不同协议的客户端习惯：

| 优先级 | 来源 | 说明 |
|---|---|---|
| 1 | `Authorization: Bearer sk-nyro-xxxx` | OpenAI / Gemini 客户端习惯 |
| 2 | `x-api-key: sk-nyro-xxxx` | Anthropic 客户端习惯 |

## 六、数据库 DDL

```sql
CREATE TABLE IF NOT EXISTS routes (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    ingress_protocol  TEXT NOT NULL,
    virtual_model     TEXT NOT NULL,
    target_provider   TEXT NOT NULL REFERENCES providers(id),
    target_model      TEXT NOT NULL,
    access_control    INTEGER NOT NULL DEFAULT 0,
    is_active         INTEGER NOT NULL DEFAULT 1,
    created_at        TEXT DEFAULT (datetime('now')),
    UNIQUE(ingress_protocol, virtual_model)
);

CREATE TABLE IF NOT EXISTS api_keys (
    id          TEXT PRIMARY KEY,
    key         TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    rpm         INTEGER,
    tpm         INTEGER,
    tpd         INTEGER,
    status      TEXT NOT NULL DEFAULT 'active',
    expires_at  TEXT,
    created_at  TEXT DEFAULT (datetime('now')),
    updated_at  TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS api_key_routes (
    api_key_id  TEXT NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    route_id    TEXT NOT NULL REFERENCES routes(id) ON DELETE CASCADE,
    PRIMARY KEY (api_key_id, route_id)
);
```

## 七、UI 交互

### 创建路由

```
┌─ 创建路由 ──────────────────────────────────┐
│                                              │
│  名称          [Production Claude        ]  │
│  接入协议      [OpenAI ▼]                   │
│  目标提供商    [Anthropic ▼]                │
│  目标模型      [claude-sonnet-4-5 ▼]       │
│  虚拟模型      [claude-sonnet-4-5        ]  │  ← 自动继承，可修改
│                                              │
│  访问控制      [○ 关闭]                      │
│                                              │
│                      [取消]  [创建路由]      │
└──────────────────────────────────────────────┘
```

### 创建 API Key

```
┌─ 创建 API Key ──────────────────────────────┐
│                                              │
│  名称          [Frontend App             ]  │
│  过期时间      [30 天 ▼]                     │
│  RPM 限额      [60         ] (留空=不限)    │
│  TPM 限额      [           ] (留空=不限)    │
│  TPD 限额      [100000     ]                │
│                                              │
│  绑定路由 (不绑定则全局生效)                │
│  ┌──────────────────────────────────────┐   │
│  │  ☑ prod-smart  (openai / gpt-4o)    │   │
│  │  ☑ dev-cheap   (openai / gpt-4o-mini)│   │
│  │  ☐ prod-heavy  (anthropic / claude)  │   │
│  └──────────────────────────────────────┘   │
│                                              │
│                    [取消]  [创建 Key]        │
└──────────────────────────────────────────────┘
```

### API Key 详情

```
┌─ API Key 详情 ──────────────────────────────┐
│ 名称:   Frontend App                        │
│ Key:    sk-nyro-•••••••  [复制] [吊销]      │
│ 状态:   ● active                            │
│ 配额:   60 RPM / 100K TPD                   │
│ 过期:   2026-04-03                          │
│                                              │
│ 绑定路由:                                   │
│  ✓ prod-smart  (openai / gpt-4o)            │
│  ✓ dev-cheap   (openai / gpt-4o-mini)       │
│                            [管理绑定]        │
└──────────────────────────────────────────────┘
```

## 八、与现有设计的变更点

| 变更项 | 旧设计 | 新设计 |
|---|---|---|
| 路由匹配键 | `match_pattern`（支持 glob/通配符） | `(ingress_protocol, virtual_model)` 精确匹配 |
| Fallback | `fallback_provider` + `fallback_model` | 移除，后续迭代 |
| Provider 优先级 | `priority` 字段 | 移除，后续迭代 |
| 代理鉴权 | Settings 中的全局 `bearer_token` | `sk-nyro-xxxx` Key 体系，按路由粒度控制 |
| 管理 API 鉴权 | 无 | 桌面模式仅监听 127.0.0.1 无需鉴权；Server 模式后续补充 |
