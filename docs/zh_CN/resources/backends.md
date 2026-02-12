# Backends（后端）

## 作用

`backends` 定义上游服务器集群，对应 Nginx 的 `upstream` 概念。用于配置负载均衡、健康检查、超时等参数。

## 配置说明

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | 是 | 后端名称，唯一标识 |
| `algorithm` | string | 否 | 负载均衡算法：`roundrobin`（默认）、`chash` |
| `endpoints` | array | 是 | 端点列表 |
| `timeout` | object | 否 | 超时配置 |
| `retries` | number | 否 | 重试次数 |

### endpoints 配置

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `address` | string | 是 | 地址，格式：`IP:PORT` 或 `IP` |
| `port` | number | 否 | 端口（如 address 不含端口则必填） |
| `weight` | number | 否 | 权重，默认 1 |

### timeout 配置

| 字段 | 类型 | 说明 |
|------|------|------|
| `connect` | number | 连接超时（毫秒） |
| `send` | number | 发送超时（毫秒） |
| `read` | number | 读取超时（毫秒） |

## 使用示例

### 基础配置

```yaml
backends:
  - name: user-backend
    endpoints:
      - address: 192.168.1.10:8080
      - address: 192.168.1.11:8080
```

### 完整配置

```yaml
backends:
  - name: order-backend
    algorithm: roundrobin
    endpoints:
      - address: 192.168.1.20:8080
        weight: 100
      - address: 192.168.1.21:8080
        weight: 50
    timeout:
      connect: 5000
      send: 60000
      read: 60000
    retries: 3
```

### 一致性哈希

```yaml
backends:
  - name: session-backend
    algorithm: chash
    endpoints:
      - address: 192.168.1.30:8080
      - address: 192.168.1.31:8080
```

## Admin API

> 端点前缀 `http://127.0.0.1:11080/nyro/admin`

### 列表

```bash
curl http://127.0.0.1:11080/nyro/admin/backends
```

### 查询

```bash
curl http://127.0.0.1:11080/nyro/admin/backends/user-backend
```

### 创建

```bash
curl -X POST http://127.0.0.1:11080/nyro/admin/backends \
  -H "Content-Type: application/json" \
  -d '{
    "name": "user-backend",
    "algorithm": "roundrobin",
    "endpoints": [
      {"address": "192.168.1.10", "port": 8080, "weight": 100},
      {"address": "192.168.1.11", "port": 8080, "weight": 50}
    ],
    "timeout": {"connect": 5000, "send": 60000, "read": 60000}
  }'
```

### 更新

```bash
curl -X PUT http://127.0.0.1:11080/nyro/admin/backends/user-backend \
  -H "Content-Type: application/json" \
  -d '{
    "name": "user-backend",
    "endpoints": [
      {"address": "192.168.1.10", "port": 8080}
    ]
  }'
```

### 删除

```bash
curl -X DELETE http://127.0.0.1:11080/nyro/admin/backends/user-backend
```

> 如果有 service 引用了该 backend，删除将被拒绝（返回 400）。

## 关联资源

- 被 `services` 通过 `backend` 字段引用
