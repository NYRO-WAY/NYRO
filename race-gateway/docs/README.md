# Race Gateway

`race-gateway` 是从旧生产 Python 并发竞速逻辑迁移出来的独立 Rust 服务。

设计与任务文档位于仓库根目录：

- `docs/design/race-gateway-architecture.md`
- `docs/design/race-gateway-interface-draft.md`
- `docs/design/race-gateway-development-tasks.md`

当前目录只保留服务自身的补充说明与本地开发入口。

本地开发最小流程：

1. 进入 `race-gateway/`
2. 设置 `RACE_GATEWAY_PROXY_BIND_ADDR`
3. 设置 `RACE_GATEWAY_ADMIN_BIND_ADDR`
4. 设置 `RACE_GATEWAY_DATABASE_URL`
5. 可选设置 `RACE_GATEWAY_BOOTSTRAP_JSON_PATH`
6. 运行 `cargo run`

当前内嵌管理页源文件位于：

- `race-gateway/webui/src/`

内嵌到 Rust 服务的静态资源位于：

- `race-gateway/src/web/assets/`

