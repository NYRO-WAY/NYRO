# Nyro Server

Nyro Server 是 Nyro AI Gateway 的独立服务端二进制，支持两种运行模式：

- **完整模式**（默认）：数据库存储 + Admin API + WebUI
- **Standalone 模式**：YAML 配置驱动，无数据库依赖，仅运行代理服务

## 快速开始

### 完整模式

```bash
nyro-server
```

默认使用 SQLite，数据目录 `~/.nyro`，代理端口 `19530`，管理端口 `19531`。

### Standalone 模式

```bash
nyro-server --config config.yaml
```

从 YAML 文件加载所有配置，不启动 Admin API 和 WebUI，仅运行 Proxy。

## 命令行参数

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--config, -c` | 无 | YAML 配置文件路径，启用 standalone 模式 |
| `--proxy-host` | `127.0.0.1` | 代理监听地址 |
| `--proxy-port` | `19530` | 代理监听端口 |
| `--admin-host` | `127.0.0.1` | Admin API 监听地址（完整模式） |
| `--admin-port` | `19531` | Admin API 监听端口（完整模式） |
| `--data-dir` | `~/.nyro` | 数据存储目录 |
| `--admin-key` | 无 | Admin API 认证 Bearer Token |
| `--admin-cors-origin` | 自动 | Admin API CORS 源（可重复） |
| `--proxy-cors-origin` | 自动 | 代理 CORS 源（可重复） |
| `--webui-dir` | `./webui/dist` | WebUI 静态文件目录 |
| `--storage-backend` | `sqlite` | 存储后端：`sqlite` / `postgres` |
| `--storage-dsn-env` | `NYRO_STORAGE_DSN` | 读取存储 DSN 的环境变量名 |
| `--sqlite-migrate-on-start` | `true` | 启动时自动运行 SQLite 迁移 |
| `--storage-max-connections` | `10` | 连接池最大连接数 |
| `--storage-min-connections` | `1` | 连接池最小连接数 |
| `--storage-acquire-timeout-secs` | `10` | 连接获取超时（秒） |
| `--storage-idle-timeout-secs` | 无 | 空闲连接超时（秒） |
| `--storage-max-lifetime-secs` | 无 | 连接最大生命周期（秒） |

## 完整模式

### 存储后端

#### SQLite（默认）

无需额外配置，数据存储在 `--data-dir` 下的 `gateway.db`。

```bash
nyro-server --data-dir ~/.nyro
```

#### PostgreSQL

```bash
export NYRO_STORAGE_DSN="postgres://user:pass@localhost:5432/nyro"
nyro-server --storage-backend postgres
```

### Admin API 安全

当 `--admin-host` 不是回环地址时，**必须**设置 `--admin-key`：

```bash
nyro-server \
  --admin-host 0.0.0.0 \
  --admin-key "your-secret-token"
```

客户端请求 Admin API 时需携带 `Authorization: Bearer your-secret-token`。

### WebUI

完整模式下 WebUI 自动挂载在 Admin 端口，访问 `http://localhost:19531` 即可打开管理界面。

通过 `--webui-dir` 指定自定义 WebUI 静态文件路径：

```bash
nyro-server --webui-dir /opt/nyro/webui
```

## Standalone 模式

Standalone 模式适用于以下场景：

- 容器化部署，配置通过 ConfigMap/Volume 挂载
- CI/CD 管道中的临时代理
- 嵌入式或边缘场景，资源受限
- Config-as-Code 工作流

### YAML 配置结构

```yaml
server:
  proxy_host: "0.0.0.0"
  proxy_port: 19530

providers:
  - name: openai
    default_protocol: openai
    endpoints:
      openai:
        base_url: https://api.openai.com/v1
    api_key: sk-xxx
    models_source: https://api.openai.com/v1/models      # 可选

  - name: deepseek
    default_protocol: openai
    endpoints:
      openai:
        base_url: https://api.deepseek.com/v1
      anthropic:
        base_url: https://api.deepseek.com/anthropic
    api_key: sk-xxx

routes:
  - name: gpt-4o
    virtual_model: gpt-4o
    targets:
      - provider: openai
        model: gpt-4o

  - name: ds-chat
    virtual_model: deepseek-chat
    strategy: priority                  # weighted（默认）或 priority
    targets:
      - provider: deepseek
        model: deepseek-chat
        priority: 1
      - provider: openai
        model: gpt-4o-mini
        priority: 2

settings:
  proxy_enabled: "false"
```

### 最小配置

```yaml
providers:
  - name: openai
    default_protocol: openai
    endpoints:
      openai:
        base_url: https://api.openai.com/v1
    api_key: sk-xxx

routes:
  - name: gpt-4o
    virtual_model: gpt-4o
    targets:
      - provider: openai
        model: gpt-4o
```

### 配置字段说明

#### server

| 字段 | 默认值 | 说明 |
|------|--------|------|
| `proxy_host` | `127.0.0.1` | 代理监听地址 |
| `proxy_port` | `19530` | 代理监听端口 |

CLI 参数 `--proxy-host` / `--proxy-port` 显式指定时会覆盖 YAML 值。

#### providers[]

| 字段 | 必填 | 说明 |
|------|------|------|
| `name` | 是 | Provider 名称，路由中通过此名称引用 |
| `default_protocol` | 是 | 默认出口协议，必须在 `endpoints` 中有对应条目 |
| `endpoints` | 是 | 协议 → 端点映射，key 为协议名（`openai`/`anthropic`/`gemini`） |
| `api_key` | 是 | API 密钥 |
| `use_proxy` | 否 | 是否使用本地代理（默认 `false`） |
| `models_source` | 否 | 模型发现 URL |
| `capabilities_source` | 否 | 模型能力发现 URL |
| `static_models` | 否 | 静态模型列表 |

#### routes[]

| 字段 | 必填 | 说明 |
|------|------|------|
| `name` | 是 | 路由名称 |
| `virtual_model` | 是 | 客户端请求的模型 ID（精确匹配） |
| `strategy` | 否 | 负载策略：`weighted`（默认）或 `priority` |
| `targets` | 是 | 目标列表（至少一个） |
| `access_control` | 否 | 是否启用访问控制（默认 `false`） |

#### routes[].targets[]

| 字段 | 必填 | 说明 |
|------|------|------|
| `provider` | 是 | Provider 名称（需与 `providers[].name` 匹配） |
| `model` | 是 | 实际模型 ID |
| `weight` | 否 | 权重，`weighted` 策略下使用（默认 `100`） |
| `priority` | 否 | 优先级，`priority` 策略下使用（默认 `1`，数字越小优先级越高） |

### 多协议转发

Standalone 模式完整支持多协议自动转发。当 Provider 声明了多个协议端点时：

```yaml
providers:
  - name: deepseek
    default_protocol: openai
    endpoints:
      openai:
        base_url: https://api.deepseek.com/v1
      anthropic:
        base_url: https://api.deepseek.com/anthropic
```

- OpenAI 客户端请求 → 直接转发到 `openai` 端点
- Anthropic 客户端请求 → 直接转发到 `anthropic` 端点
- Gemini 客户端请求 → Provider 不支持，自动转换为 `default_protocol`（openai）后转发

### 与完整模式的差异

| 能力 | 完整模式 | Standalone |
|------|----------|------------|
| Provider/Route 管理 | Admin API 实时读写 | 编辑 YAML + 重启 |
| Admin API | 完整 | 不启动 |
| WebUI | 完整 | 不启动 |
| 请求日志 | DB 存储 + WebUI 查看 | 仅 stdout |
| 数据持久化 | SQLite/Postgres | 无（进程重启从 YAML 恢复） |
| 部署依赖 | 二进制 + 存储目录 | 二进制 + YAML 文件 |

## 使用示例

### Docker 部署（Standalone）

```dockerfile
FROM rust:1.83 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p nyro-server

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/nyro-server /usr/local/bin/
COPY config.yaml /etc/nyro/config.yaml
EXPOSE 19530
CMD ["nyro-server", "--config", "/etc/nyro/config.yaml", "--proxy-host", "0.0.0.0"]
```

### 客户端调用

启动后，所有协议客户端均可通过代理端口访问路由配置的模型：

```bash
# OpenAI 协议
curl http://localhost:19530/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4o", "messages": [{"role": "user", "content": "hello"}]}'

# Anthropic 协议
curl http://localhost:19530/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: any" \
  -H "anthropic-version: 2023-06-01" \
  -d '{"model": "gpt-4o", "max_tokens": 1024, "messages": [{"role": "user", "content": "hello"}]}'
```
