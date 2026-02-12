# Consumers（消费者）

## 作用

`consumers` 定义 API 消费者，用于身份认证和访问控制。每个消费者可配置多种凭证类型和专属插件。

## 配置说明

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | 是 | 消费者名称，唯一标识 |
| `credentials` | object | 是 | 凭证配置 |
| `plugins` | array | 否 | 消费者级别插件 |

### credentials 配置

支持多种认证方式：

#### key-auth

```yaml
credentials:
  key-auth:
    key: your-api-key
```

#### basic-auth

```yaml
credentials:
  basic-auth:
    username: user
    password: secret
```

#### jwt-auth

```yaml
credentials:
  jwt-auth:
    key: jwt-key-id
    secret: jwt-secret
```

## 使用示例

### 基础配置

```yaml
consumers:
  - name: mobile-app
    credentials:
      key-auth:
        key: mobile-app-api-key-123
```

### 多种认证方式

```yaml
consumers:
  - name: web-app
    credentials:
      key-auth:
        key: web-app-api-key
      basic-auth:
        username: webapp
        password: webapp-secret
```

### 带插件配置

```yaml
consumers:
  - name: partner-api
    credentials:
      key-auth:
        key: partner-api-key
    plugins:
      - name: rate-limiting
        config:
          rate: 100
          burst: 10
      - name: ip-restriction
        config:
          whitelist: ["203.0.113.0/24"]
```

## 认证流程

1. 客户端请求携带凭证（如 API Key、JWT 等）
2. 认证插件（如 `key-auth`）从请求中提取凭证
3. 系统通过凭证查找对应的 consumer
4. 认证成功后，消费者的专属插件生效

## Admin API

> 端点前缀 `http://127.0.0.1:11080/nyro/admin`

### 列表

```bash
curl http://127.0.0.1:11080/nyro/admin/consumers
```

### 查询

```bash
curl http://127.0.0.1:11080/nyro/admin/consumers/mobile-app
```

### 创建

```bash
curl -X POST http://127.0.0.1:11080/nyro/admin/consumers \
  -H "Content-Type: application/json" \
  -d '{
    "name": "mobile-app",
    "credentials": {
      "key-auth": {"key": "mobile-app-api-key-123"}
    }
  }'
```

### 更新

```bash
curl -X PUT http://127.0.0.1:11080/nyro/admin/consumers/mobile-app \
  -H "Content-Type: application/json" \
  -d '{
    "name": "mobile-app",
    "credentials": {
      "key-auth": {"key": "new-api-key-456"}
    },
    "plugins": [
      {"name": "rate-limiting", "config": {"rate": 200}}
    ]
  }'
```

### 删除

```bash
curl -X DELETE http://127.0.0.1:11080/nyro/admin/consumers/mobile-app
```

## 关联资源

- 被认证类插件（`key-auth`、`jwt-auth` 等）使用
- 消费者级插件对该消费者的所有请求生效
