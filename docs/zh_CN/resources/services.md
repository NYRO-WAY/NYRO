# Services（服务）

## 作用

`services` 是上游的逻辑抽象，作为 `routes` 和 `backends` 之间的中间层。支持两种模式：
1. **引用 backend**：通过 `backend` 字段关联已定义的后端
2. **URL 直接代理**：通过 `url` 字段直接代理到外部 API

## 配置说明

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | 是 | 服务名称，唯一标识 |
| `backend` | string | 否* | 引用的 backend 名称 |
| `url` | string | 否* | 直接代理的 URL |
| `plugins` | array | 否 | 服务级别插件 |

> *`backend` 和 `url` 二选一

## 使用示例

### 模式一：引用 Backend

```yaml
backends:
  - name: user-backend
    endpoints:
      - address: 192.168.1.10:8080

services:
  - name: user-service
    backend: user-backend
```

### 模式二：URL 直接代理

无需定义 backend，直接代理到外部 API。系统自动从 URL 解析 `protocol`、`host`、`port`、`path`。

```yaml
services:
  # HTTP
  - name: httpbin-service
    url: http://httpbin.org

  # HTTPS
  - name: openai-service
    url: https://api.openai.com/v1

  # 带端口
  - name: internal-api
    url: http://10.0.0.1:9000/api
```

### 带插件的服务

```yaml
services:
  - name: protected-service
    backend: api-backend
    plugins:
      - name: rate-limiting
        config:
          rate: 100
          burst: 50
```

## Admin API

> 端点前缀 `http://127.0.0.1:11080/nyro/admin`

### 列表

```bash
curl http://127.0.0.1:11080/nyro/admin/services
```

### 查询

```bash
curl http://127.0.0.1:11080/nyro/admin/services/httpbin-service
```

### 创建

```bash
curl -X POST http://127.0.0.1:11080/nyro/admin/services \
  -H "Content-Type: application/json" \
  -d '{
    "name": "httpbin-service",
    "url": "http://httpbin.org"
  }'
```

> `backend` 和 `url` 二选一。引用 `backend` 时该 backend 必须已存在。

### 更新

```bash
curl -X PUT http://127.0.0.1:11080/nyro/admin/services/httpbin-service \
  -H "Content-Type: application/json" \
  -d '{
    "name": "httpbin-service",
    "url": "https://httpbin.org"
  }'
```

### 删除

```bash
curl -X DELETE http://127.0.0.1:11080/nyro/admin/services/httpbin-service
```

> 如果有 route 引用了该 service，删除将被拒绝（返回 400）。

## 关联资源

- 通过 `backend` 引用 `backends`
- 被 `routes` 通过 `service` 字段引用
