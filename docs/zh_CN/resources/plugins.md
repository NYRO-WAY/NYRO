# Plugins（插件）

## 作用

`plugins` 提供可插拔的功能扩展，可在不同层级配置，用于实现认证、限流、日志等功能。

## 插件层级

插件可配置在三个层级，执行顺序：全局 → 服务 → 路由

| 层级 | 位置 | 作用范围 |
|------|------|----------|
| 全局 | 顶层 `plugins` | 所有请求 |
| 服务 | `services[].plugins` | 该服务的所有路由 |
| 路由 | `routes[].plugins` | 仅该路由 |

## 配置格式

```yaml
plugins:
  - name: 插件名称
    config:
      配置项: 值
```

## 使用示例

### 全局插件

```yaml
plugins:
  - name: cors
    config:
      allow_origins: "*"
      allow_methods: ["GET", "POST", "PUT", "DELETE"]

  - name: request-id
    config:
      header_name: X-Request-ID
```

### 服务级插件

```yaml
services:
  - name: api-service
    backend: api-backend
    plugins:
      - name: rate-limiting
        config:
          rate: 1000
          burst: 100
```

### 路由级插件

```yaml
routes:
  - name: protected-api
    service: api-service
    paths:
      - /api/v1/admin/*
    plugins:
      - name: key-auth
      - name: ip-restriction
        config:
          whitelist: ["10.0.0.0/8"]
```

## 常用插件

### 认证类

| 插件 | 说明 |
|------|------|
| `key-auth` | API Key 认证 |
| `basic-auth` | HTTP Basic 认证 |
| `jwt-auth` | JWT 认证 |

### 安全类

| 插件 | 说明 |
|------|------|
| `ip-restriction` | IP 黑白名单 |
| `cors` | 跨域资源共享 |

### 流量控制

| 插件 | 说明 |
|------|------|
| `rate-limiting` | 请求限流 |

### 可观测性

| 插件 | 说明 |
|------|------|
| `request-id` | 请求 ID 注入 |

## Admin API

全局插件通过 Admin API 的 `plugins` 资源管理。

> 端点前缀 `http://127.0.0.1:11080/nyro/admin`

### 列表

```bash
curl http://127.0.0.1:11080/nyro/admin/plugins
```

### 查询

```bash
curl http://127.0.0.1:11080/nyro/admin/plugins/global-cors
```

### 创建

```bash
curl -X POST http://127.0.0.1:11080/nyro/admin/plugins \
  -H "Content-Type: application/json" \
  -d '{
    "name": "global-cors",
    "id": "cors",
    "config": {
      "allow_origins": "*",
      "allow_methods": ["GET", "POST", "PUT", "DELETE"]
    }
  }'
```

### 更新

```bash
curl -X PUT http://127.0.0.1:11080/nyro/admin/plugins/global-cors \
  -H "Content-Type: application/json" \
  -d '{
    "name": "global-cors",
    "id": "cors",
    "config": {
      "allow_origins": "https://example.com"
    }
  }'
```

### 删除

```bash
curl -X DELETE http://127.0.0.1:11080/nyro/admin/plugins/global-cors
```

## 完整示例

```yaml
# 全局插件
plugins:
  - name: cors
    config:
      allow_origins: "*"

# 服务插件
services:
  - name: user-service
    backend: user-backend
    plugins:
      - name: rate-limiting
        config:
          rate: 500

# 路由插件
routes:
  - name: user-api
    service: user-service
    paths:
      - /api/v1/users/*
    plugins:
      - name: key-auth
```
