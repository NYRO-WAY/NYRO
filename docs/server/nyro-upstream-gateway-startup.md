# Nyro Gateway Stack 启动与部署

本文档是当前仓库的统一运行文档，覆盖三类服务：

- `nyro-server`
  - 协议转换、主代理入口、主 WebUI / Admin API
- `upstream-gateway`
  - 上游 key 池、RPM / RPD / TPM 限流、provider 运行时观测
- `race-gateway`
  - 同协议多 candidate 并发竞速、权重惩罚恢复、竞速运行时观测

推荐拓扑：

```text
client / SDK / CLI
  -> nyro
  -> upstream-gateway        # 可选，负责 provider-native 上游配额
  -> race-gateway            # 可选，负责同协议竞速
  -> real upstream provider
```

常见组合：

- `nyro -> upstream-gateway -> real provider`
  - 适合限流 / key 池治理
- `nyro -> race-gateway -> real provider`
  - 适合多候选竞速
- `nyro -> race-gateway -> upstream-gateway -> real provider`
  - 适合先竞速，再由各 candidate 继续走上游配额治理

## 1. 端口与职责

建议默认端口：

| 服务 | 代理端口 | 管理端口 | 说明 |
|------|----------|----------|------|
| `nyro-server` | `127.0.0.1:19530` | `127.0.0.1:19531` | 代理与主 WebUI / Admin |
| `upstream-gateway` | `127.0.0.1:2080` | `127.0.0.1:2081` | provider-native 代理与限流管理 |
| `race-gateway` | `127.0.0.1:2090` | `127.0.0.1:2091` | 竞速代理与竞速管理 |

当前实现差异：

- `nyro-server`
  - 固定双端口
- `upstream-gateway`
  - `UPSTREAM_GATEWAY_ADMIN_BIND_ADDR` 可选
  - 未配置时只启动代理端口
- `race-gateway`
  - 当前实现强制双端口

## 2. 前置依赖

### 2.1 Windows 开发机

建议准备：

- Rust stable
- Node.js 22+ / npm
- `cmake`
- Visual Studio 2022 Build Tools，包含 C++ 构建工具
- Git

检查：

```powershell
rustc -V
cargo -V
node -v
npm -v
cmake --version
```

如果 `cmake` 不在 `PATH`：

```powershell
$env:CMAKE = "C:\Program Files\CMake\bin\cmake.exe"
```

### 2.2 Linux ARM64 生产机

建议准备：

- `build-essential`
- `cmake`
- `pkg-config`
- `git`
- `ca-certificates`
- Node.js 22+ / npm
  - 仅当你要重建 `upstream-gateway/webui`

Ubuntu / Debian 示例：

```bash
sudo apt-get update
sudo apt-get install -y build-essential cmake pkg-config git ca-certificates
curl https://sh.rustup.rs -sSf | sh
source "$HOME/.cargo/env"
rustup default stable
```

## 3. 仓库内三个服务的构建入口

| 服务 | 目录 | 构建命令 | 产物 |
|------|------|----------|------|
| `nyro-server` | 仓库根 | `cargo build --release -p nyro-server` | `target/release/nyro-server` |
| `upstream-gateway` | `upstream-gateway/` | `cargo build --release` | `upstream-gateway/target/release/upstream-gateway` |
| `race-gateway` | `race-gateway/` | `cargo build --release` | `race-gateway/target/release/race-gateway` |

前端资源说明：

- `nyro-server`
  - WebUI 已内嵌
- `upstream-gateway`
  - `webui/` 是 React + Vite 管理台
  - 先 `npm run build`，再编译 Rust
- `race-gateway`
  - `webui/src/` 是嵌入式静态管理页源码
  - 先执行 `powershell -ExecutionPolicy Bypass -File webui\sync-assets.ps1`
  - 再编译 Rust

## 4. 本地开发启动

如果你想把三套服务作为一个可重复启停的本地开发栈跑起来，仓库里已经补了脚本：

```powershell
Set-Location scripts\dev-stack
.\start-dev-stack.ps1
.\status-dev-stack.ps1
.\stop-dev-stack.ps1
```

默认地址：

- `nyro proxy`
  - [http://127.0.0.1:19530/health](http://127.0.0.1:19530/health)
- `nyro WebUI`
  - [http://127.0.0.1:19531/](http://127.0.0.1:19531/)
- `upstream-gateway proxy`
  - [http://127.0.0.1:2080/healthz](http://127.0.0.1:2080/healthz)
- `upstream-gateway WebUI`
  - [http://127.0.0.1:2081/admin](http://127.0.0.1:2081/admin)
- `race-gateway proxy`
  - [http://127.0.0.1:2090/healthz](http://127.0.0.1:2090/healthz)
- `race-gateway WebUI`
  - [http://127.0.0.1:2091/admin](http://127.0.0.1:2091/admin)

### 4.1 只跑 `nyro + upstream-gateway`

这是当前最常用的限流治理组合。

第一步：准备 bootstrap：

```powershell
Copy-Item `
  upstream-gateway\examples\gemini-first\bootstrap.template.json `
  upstream-gateway\examples\gemini-first\bootstrap.local.json
```

把里面的真实 key 填好。

第二步：启动 `upstream-gateway`：

```powershell
$env:UPSTREAM_GATEWAY_PROXY_BIND_ADDR = "127.0.0.1:2080"
$env:UPSTREAM_GATEWAY_ADMIN_BIND_ADDR = "127.0.0.1:2081"
$env:UPSTREAM_GATEWAY_DATABASE_URL = "sqlite://upstream-gateway.db"
$env:UPSTREAM_GATEWAY_BOOTSTRAP_JSON_PATH = "examples/gemini-first/bootstrap.local.json"
$env:UPSTREAM_GATEWAY_REQUEST_TIMEOUT_SECS = "300"

Set-Location upstream-gateway
cargo run
```

第三步：在另一个终端启动 `nyro` standalone：

```powershell
Set-Location ..
cargo run -p nyro-server -- `
  --config upstream-gateway/examples/gemini-first/nyro.standalone.yaml `
  --data-dir .nyro-dev
```

验证：

- `upstream-gateway proxy`
  - [http://127.0.0.1:2080/healthz](http://127.0.0.1:2080/healthz)
- `upstream-gateway admin`
  - [http://127.0.0.1:2081/admin](http://127.0.0.1:2081/admin)
- `nyro proxy`
  - `http://127.0.0.1:19530`

### 4.2 跑 `race-gateway`

进入 `race-gateway/`：

```powershell
$env:RACE_GATEWAY_PROXY_BIND_ADDR = "127.0.0.1:2090"
$env:RACE_GATEWAY_ADMIN_BIND_ADDR = "127.0.0.1:2091"
$env:RACE_GATEWAY_DATABASE_URL = "sqlite://race-gateway.db"
Remove-Item Env:RACE_GATEWAY_BOOTSTRAP_JSON_PATH -ErrorAction SilentlyContinue

Set-Location race-gateway
cargo run --bin race-gateway
```

验证：

- proxy 健康检查
  - [http://127.0.0.1:2090/healthz](http://127.0.0.1:2090/healthz)
- admin 健康检查
  - [http://127.0.0.1:2091/admin/healthz](http://127.0.0.1:2091/admin/healthz)
- admin UI
  - [http://127.0.0.1:2091/admin](http://127.0.0.1:2091/admin)
- Prometheus 指标
  - [http://127.0.0.1:2091/admin/metrics](http://127.0.0.1:2091/admin/metrics)

### 4.3 三服务串联联调

如果你要同时验证协议转换、竞速和上游限流，推荐链路：

```text
client -> nyro -> race-gateway -> upstream-gateway -> real provider
```

推荐顺序：

1. 启动 `upstream-gateway`
2. 启动 `race-gateway`
3. 启动 `nyro`
4. 客户端只打 `nyro` 代理口

路由配置建议：

- `nyro`
  - provider 指向 `race-gateway`
- `race-gateway`
  - model endpoint / candidate endpoint 指向 `upstream-gateway`
- `upstream-gateway`
  - provider 指向真实 OpenAI / Anthropic / Gemini

## 5. 各服务常用配置

### 5.1 `nyro-server`

完整模式常用命令：

```bash
nyro-server \
  --proxy-host 127.0.0.1 \
  --proxy-port 19530 \
  --admin-host 127.0.0.1 \
  --admin-port 19531 \
  --data-dir ~/.nyro
```

重要说明：

- `--config <yaml>` 为 standalone 模式
- standalone 模式不启动 `nyro` 自己的主 Admin API / WebUI
- 当 `--admin-host` 不是回环地址时，必须配置 `--admin-token`

### 5.2 `upstream-gateway`

环境变量：

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `UPSTREAM_GATEWAY_PROXY_BIND_ADDR` | `127.0.0.1:2080` | 代理监听地址 |
| `UPSTREAM_GATEWAY_ADMIN_BIND_ADDR` | 空 | 管理监听地址；不配则不启用 |
| `UPSTREAM_GATEWAY_BIND_ADDR` | 空 | 旧兼容变量，仅作为 proxy 地址回退 |
| `UPSTREAM_GATEWAY_REQUEST_TIMEOUT_SECS` | `300` | 上游请求超时 |
| `UPSTREAM_GATEWAY_DATABASE_URL` | `sqlite://upstream-gateway.db` | SQLite 地址 |
| `UPSTREAM_GATEWAY_BOOTSTRAP_JSON_PATH` | 空 | 空库初始化 bootstrap |

### 5.3 `race-gateway`

环境变量：

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `RACE_GATEWAY_PROXY_BIND_ADDR` | `127.0.0.1:2090` | 竞速代理监听地址 |
| `RACE_GATEWAY_ADMIN_BIND_ADDR` | `127.0.0.1:2091` | 竞速管理监听地址 |
| `RACE_GATEWAY_DATABASE_URL` | `sqlite://race-gateway.db` | SQLite 地址 |
| `RACE_GATEWAY_BOOTSTRAP_JSON_PATH` | 空 | 空库时导入初始配置 |

补充：

- `RUST_LOG=info`
  - 推荐在三类服务上统一开启

## 6. 端点总览

### 6.1 `nyro-server`

- proxy
  - `POST /v1/chat/completions`
  - `POST /v1/messages`
  - 以及 Nyro 当前支持的其他兼容入口
- admin
  - `GET /`
  - `GET /api/*`

### 6.2 `upstream-gateway`

- proxy
  - `GET /healthz`
  - `POST /providers/:provider_id/openai/v1/chat/completions`
  - `POST /providers/:provider_id/openai/v1/responses`
  - `POST /providers/:provider_id/openai/v1/embeddings`
  - `POST /providers/:provider_id/anthropic/v1/messages`
  - `POST /providers/:provider_id/google/v1beta/models/:model_action`
  - `POST /providers/:provider_id/google/models/:model_action`
- admin
  - `GET /healthz`
  - `GET /admin`
  - `GET /admin/healthz`
  - `GET /admin/providers`
  - `GET /admin/providers/:provider_id`
  - `PUT /admin/providers/:provider_id`
  - `DELETE /admin/providers/:provider_id`
  - `PUT /admin/providers/:provider_id/keys/:key_id`
  - `DELETE /admin/providers/:provider_id/keys/:key_id`
  - `PUT /admin/providers/:provider_id/model-rules/*model`
  - `DELETE /admin/providers/:provider_id/model-rules/*model`
  - `GET /admin/providers/:provider_id/runtime`
  - `GET /admin/runtime/providers`

### 6.3 `race-gateway`

- proxy
  - `GET /healthz`
  - `POST /groups/:group_id/openai/v1/chat/completions`
  - `POST /groups/:group_id/openai/v1/responses`
  - `POST /groups/:group_id/anthropic/v1/messages`
  - `POST /groups/:group_id/google/v1beta/models/:model_action`
  - `POST /groups/:group_id/google/models/:model_action`
- admin
  - `GET /admin`
  - `GET /admin/healthz`
  - `GET /admin/metrics`
  - `GET /admin/models`
  - `GET /admin/models/:model_id`
  - `PUT /admin/models/:model_id`
  - `DELETE /admin/models/:model_id`
  - `GET /admin/groups`
  - `GET /admin/groups/:group_id`
  - `PUT /admin/groups/:group_id`
  - `DELETE /admin/groups/:group_id`
  - `GET /admin/key-pools`
  - `GET /admin/key-pools/:key_pool_id`
  - `PUT /admin/key-pools/:key_pool_id`
  - `DELETE /admin/key-pools/:key_pool_id`
  - `GET /admin/runtime/groups`
  - `GET /admin/runtime/groups/:group_id`
  - `GET /admin/settings`
  - `PUT /admin/settings`
  - `POST /admin/validate/model`
  - `POST /admin/validate/group`
  - `POST /admin/validate/key-pool`

## 7. 前端开发与静态资源同步

### 7.1 `upstream-gateway`

改了 `upstream-gateway/webui/` 后：

```powershell
Set-Location upstream-gateway\webui
npm install
npm run build

Set-Location ..
cargo build
```

说明：

- Rust 会嵌入 `webui/dist`
- 只改前端源码、不重新构建前端，页面不会更新

### 7.2 `race-gateway`

改了 `race-gateway/webui/src/` 后：

```powershell
Set-Location race-gateway
powershell -ExecutionPolicy Bypass -File webui\sync-assets.ps1
cargo build
```

说明：

- `race-gateway` 管理页不是 React / Vite
- 当前是嵌入式 `HTML + CSS + JS`
- `sync-assets.ps1` 会把 `webui/src/*` 同步到 `src/web/assets/*`

## 8. Linux ARM64 生产环境编译

推荐直接在 ARM64 主机原生编译：

```bash
cargo build --release -p nyro-server

cd upstream-gateway
cargo build --release

cd ../race-gateway
cargo build --release
```

如果你修改了 `upstream-gateway/webui/`，先：

```bash
cd upstream-gateway/webui
npm install
npm run build
```

如果你修改了 `race-gateway/webui/src/`，先：

```bash
cd race-gateway
powershell -ExecutionPolicy Bypass -File webui/sync-assets.ps1
```

Linux 上等价做法是手动把 `webui/src/admin.*` 复制到 `src/web/assets/`。

## 9. Linux ARM64 生产环境启动

推荐目录：

```text
/opt/nyro/
  nyro-server
  nyro.standalone.yaml
  data/

/opt/upstream-gateway/
  upstream-gateway
  bootstrap.json
  upstream-gateway.db

/opt/race-gateway/
  race-gateway
  race-gateway.db
```

### 9.1 启动 `upstream-gateway`

```bash
cd /opt/upstream-gateway

export UPSTREAM_GATEWAY_PROXY_BIND_ADDR=0.0.0.0:2080
export UPSTREAM_GATEWAY_ADMIN_BIND_ADDR=0.0.0.0:2081
export UPSTREAM_GATEWAY_DATABASE_URL=sqlite:///opt/upstream-gateway/upstream-gateway.db
export UPSTREAM_GATEWAY_BOOTSTRAP_JSON_PATH=/opt/upstream-gateway/bootstrap.json
export UPSTREAM_GATEWAY_REQUEST_TIMEOUT_SECS=300
export RUST_LOG=info

./upstream-gateway
```

### 9.2 启动 `race-gateway`

```bash
cd /opt/race-gateway

export RACE_GATEWAY_PROXY_BIND_ADDR=0.0.0.0:2090
export RACE_GATEWAY_ADMIN_BIND_ADDR=0.0.0.0:2091
export RACE_GATEWAY_DATABASE_URL=sqlite:///opt/race-gateway/race-gateway.db
export RACE_GATEWAY_BOOTSTRAP_JSON_PATH=/opt/race-gateway/bootstrap.json
export RUST_LOG=info

./race-gateway
```

### 9.3 启动 `nyro`

推荐 standalone 接网关：

```bash
cd /opt/nyro

./nyro-server \
  --config /opt/nyro/nyro.standalone.yaml \
  --data-dir /opt/nyro/data
```

## 10. systemd 示例

### 10.1 `upstream-gateway.service`

```ini
[Unit]
Description=upstream-gateway
After=network.target

[Service]
WorkingDirectory=/opt/upstream-gateway
Environment=UPSTREAM_GATEWAY_PROXY_BIND_ADDR=0.0.0.0:2080
Environment=UPSTREAM_GATEWAY_ADMIN_BIND_ADDR=0.0.0.0:2081
Environment=UPSTREAM_GATEWAY_DATABASE_URL=sqlite:///opt/upstream-gateway/upstream-gateway.db
Environment=UPSTREAM_GATEWAY_BOOTSTRAP_JSON_PATH=/opt/upstream-gateway/bootstrap.json
Environment=UPSTREAM_GATEWAY_REQUEST_TIMEOUT_SECS=300
Environment=RUST_LOG=info
ExecStart=/opt/upstream-gateway/upstream-gateway
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
```

### 10.2 `race-gateway.service`

```ini
[Unit]
Description=race-gateway
After=network.target upstream-gateway.service

[Service]
WorkingDirectory=/opt/race-gateway
Environment=RACE_GATEWAY_PROXY_BIND_ADDR=0.0.0.0:2090
Environment=RACE_GATEWAY_ADMIN_BIND_ADDR=0.0.0.0:2091
Environment=RACE_GATEWAY_DATABASE_URL=sqlite:///opt/race-gateway/race-gateway.db
Environment=RACE_GATEWAY_BOOTSTRAP_JSON_PATH=/opt/race-gateway/bootstrap.json
Environment=RUST_LOG=info
ExecStart=/opt/race-gateway/race-gateway
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
```

### 10.3 `nyro.service`

```ini
[Unit]
Description=nyro-server
After=network.target upstream-gateway.service race-gateway.service

[Service]
WorkingDirectory=/opt/nyro
ExecStart=/opt/nyro/nyro-server --config /opt/nyro/nyro.standalone.yaml --data-dir /opt/nyro/data
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
```

## 11. 验证命令

### 11.1 `upstream-gateway`

```bash
curl http://127.0.0.1:2080/healthz
curl http://127.0.0.1:2081/admin/healthz
curl http://127.0.0.1:2081/admin/providers
```

### 11.2 `race-gateway`

```bash
curl http://127.0.0.1:2090/healthz
curl http://127.0.0.1:2091/admin/healthz
curl http://127.0.0.1:2091/admin/metrics
curl http://127.0.0.1:2091/admin/runtime/groups
```

内置压测工具：

```bash
cd race-gateway
cargo run --bin race-loadtest -- \
  --url http://127.0.0.1:2090/groups/demo/openai/v1/chat/completions \
  --requests 50 \
  --concurrency 10 \
  --header content-type:application/json \
  --header x-api-key:test \
  --body-file examples/loadtest/openai-chat.json
```

### 11.3 `nyro`

```bash
curl http://127.0.0.1:19531/healthz
```

客户端从 `19530` 打代理流量即可。

## 12. 常见问题

### 12.1 为什么看不到 `upstream-gateway /admin`

因为 `UPSTREAM_GATEWAY_ADMIN_BIND_ADDR` 是可选的；不设置就只启 proxy。

### 12.2 为什么 `race-gateway` 没有页面热更新

因为当前管理页是嵌入式静态资源，不是单独 dev server。改完需要先执行 `webui\sync-assets.ps1`，再重新编译 / 启动。

### 12.3 为什么推荐把复杂能力拆到独立网关

因为这样 `nyro` 仍专注协议转换，后续同步上游分支时冲突最少。

## 13. 相关文档

- [Nyro Server 说明](./README.md)
- [Race Gateway 架构设计](../design/race-gateway-architecture.md)
- [Race Gateway 管理手册](./gateway-admin-manual.md)
- [Race Gateway 压测与指标说明](../../race-gateway/docs/loadtest-and-metrics.md)
