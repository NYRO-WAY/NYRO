# Certificates（证书）

## 作用

`certificates` 定义 SSL/TLS 证书，用于 HTTPS 加密通信。支持通配符域名和多域名证书。

## 配置说明

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | 是 | 证书名称，唯一标识 |
| `snis` | array | 是 | SNI 域名列表 |
| `cert` | string | 否* | 证书内容（PEM 格式） |
| `key` | string | 否* | 私钥内容（PEM 格式） |
| `cert_file` | string | 否* | 证书文件路径 |
| `key_file` | string | 否* | 私钥文件路径 |

> *使用内容方式（`cert`/`key`）或文件方式（`cert_file`/`key_file`）二选一

## SNI 匹配规则

| 模式 | 示例 | 匹配 |
|------|------|------|
| 精确匹配 | `api.example.com` | 仅匹配 `api.example.com` |
| 通配符 | `*.example.com` | 匹配 `api.example.com`、`www.example.com` 等 |

## 使用示例

### 内容方式

```yaml
certificates:
  - name: example-cert
    snis:
      - "example.com"
      - "*.example.com"
    cert: |
      -----BEGIN CERTIFICATE-----
      MIIDXTCCAkWgAwIBAgIJAJC1HiIAZAiUMA0Gcz93F...
      -----END CERTIFICATE-----
    key: |
      -----BEGIN PRIVATE KEY-----
      MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSk...
      -----END PRIVATE KEY-----
```

### 文件方式

```yaml
certificates:
  - name: api-cert
    snis:
      - "api.example.com"
    cert_file: /etc/ssl/certs/api.example.com.crt
    key_file: /etc/ssl/private/api.example.com.key
```

### 多域名证书

```yaml
certificates:
  - name: multi-domain-cert
    snis:
      - "example.com"
      - "example.org"
      - "api.example.com"
    cert_file: /etc/ssl/certs/multi-domain.crt
    key_file: /etc/ssl/private/multi-domain.key
```

## Admin API

> 端点前缀 `http://127.0.0.1:11080/nyro/admin`

### 列表

```bash
curl http://127.0.0.1:11080/nyro/admin/certificates
```

### 查询

```bash
curl http://127.0.0.1:11080/nyro/admin/certificates/example-cert
```

### 创建

```bash
curl -X POST http://127.0.0.1:11080/nyro/admin/certificates \
  -H "Content-Type: application/json" \
  -d '{
    "name": "example-cert",
    "snis": ["example.com", "*.example.com"],
    "cert_file": "/etc/ssl/certs/example.com.crt",
    "key_file": "/etc/ssl/private/example.com.key"
  }'
```

### 更新

```bash
curl -X PUT http://127.0.0.1:11080/nyro/admin/certificates/example-cert \
  -H "Content-Type: application/json" \
  -d '{
    "name": "example-cert",
    "snis": ["example.com", "*.example.com", "api.example.com"],
    "cert_file": "/etc/ssl/certs/example.com.crt",
    "key_file": "/etc/ssl/private/example.com.key"
  }'
```

### 删除

```bash
curl -X DELETE http://127.0.0.1:11080/nyro/admin/certificates/example-cert
```

## 注意事项

1. 证书和私钥必须是 PEM 格式
2. 通配符证书只匹配一级子域名（`*.example.com` 不匹配 `a.b.example.com`）
3. 文件路径方式需确保 NYRO 进程有读取权限
