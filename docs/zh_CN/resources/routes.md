# Routes（路由）

## 作用

`routes` 定义请求匹配规则，将客户端请求路由到对应的服务。支持多种匹配模式，必须引用一个 `service`。

## 配置说明

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | 是 | 路由名称，唯一标识 |
| `service` | string | 是 | 引用的 service 名称 |
| `paths` | array | 是 | 路径匹配规则 |
| `methods` | array | 否 | HTTP 方法，默认全部 |
| `match_type` | string | 否 | 匹配类型，覆盖自动推导 |
| `hosts` | array | 否 | 域名匹配 |
| `headers` | object | 否 | 请求头匹配 |
| `priority` | number | 否 | 优先级，越大越优先 |
| `plugins` | array | 否 | 路由级别插件 |

## 路径匹配模式

### 自动推导（推荐）

系统根据路径格式自动推导匹配类型：

| 模式 | 格式 | 示例 |
|------|------|------|
| 精确匹配 | 普通路径 | `/api/v1/users` |
| 前缀匹配 | 以 `/*` 结尾 | `/api/v1/articles/*` |
| 参数匹配 | 包含 `{param}` | `/api/v1/users/{id}` |

### 手动指定 match_type

当自动推导不满足需求时，使用 `match_type` 显式指定：

| 值 | 说明 |
|------|------|
| `exact` | 精确匹配 |
| `prefix` | 前缀匹配 |
| `param` | 参数匹配 |

```yaml
routes:
  # 强制精确匹配（即使路径看起来像前缀）
  - name: exact-api
    service: api-service
    paths:
      - /api/v1/users
    match_type: exact

  # 强制前缀匹配（不使用 /* 后缀）
  - name: prefix-api
    service: api-service
    paths:
      - /api/v1
    match_type: prefix
```

## 使用示例

### 精确匹配

```yaml
routes:
  - name: user-list
    service: user-service
    paths:
      - /api/v1/users
```

### 前缀匹配

```yaml
routes:
  - name: article-api
    service: article-service
    paths:
      - /api/v1/articles/*
```

### 参数匹配

```yaml
routes:
  - name: user-detail
    service: user-service
    paths:
      - /api/v1/users/{id}
      - /api/v1/users/{id}/profile
```

### 完整配置

```yaml
routes:
  - name: internal-api
    service: internal-service
    paths:
      - /internal/*
    methods: [GET, POST]
    match_type: prefix
    hosts:
      - api.example.com
    headers:
      X-Internal-Token: "secret-token"
    priority: 100
    plugins:
      - name: ip-restriction
        config:
          whitelist: ["10.0.0.0/8"]
```

## Admin API

> 端点前缀 `http://127.0.0.1:11080/nyro/admin`

### 列表

```bash
curl http://127.0.0.1:11080/nyro/admin/routes
```

### 查询

```bash
curl http://127.0.0.1:11080/nyro/admin/routes/user-api
```

### 创建

```bash
curl -X POST http://127.0.0.1:11080/nyro/admin/routes \
  -H "Content-Type: application/json" \
  -d '{
    "name": "user-api",
    "service": "user-service",
    "paths": ["/api/v1/users/*"],
    "methods": ["GET", "POST"]
  }'
```

> 创建时 `service` 引用的服务必须已存在，否则返回 400。

### 更新（全量替换）

```bash
curl -X PUT http://127.0.0.1:11080/nyro/admin/routes/user-api \
  -H "Content-Type: application/json" \
  -d '{
    "name": "user-api",
    "service": "user-service",
    "paths": ["/api/v1/users/*", "/api/v2/users/*"],
    "methods": ["GET", "POST", "PUT"]
  }'
```

### 删除

```bash
curl -X DELETE http://127.0.0.1:11080/nyro/admin/routes/user-api
```

## 关联资源

- 必须通过 `service` 引用 `services`
- 不直接与 `backends` 关联
