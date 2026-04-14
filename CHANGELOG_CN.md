# 更新日志

Nyro 的所有重要变更均记录在此文件中。

---

## v1.6.1

> 发布于 2026-04-14

#### 功能

- **流式缓存重放 TPS 限速**：在 `ExactCacheConfig` 和 `SemanticCacheConfig` 中新增 `stream_replay_tps`（默认 100）；设为 `0` 可禁用限速并恢复即时输出行为；实现 `split_text_deltas` 辅助函数，将较大的 `TextDelta`/`ReasoningDelta` 切分为约 1 Token 的小块以平滑逐 Token 输出节奏；首个 SSE chunk 始终立即下发，保证 TTFT 为零
- **独立缓存响应头控制**：在两种缓存配置中新增 `expose_headers`（默认 `true`），可分别控制 exact 和 semantic 缓存命中时是否下发 `X-NYRO-CACHE-*` 响应头；响应头统一改为全大写：`X-NYRO-CACHE` / `X-NYRO-CACHE-KEY` / `X-NYRO-CACHE-SCORE`
- **WebUI 内嵌至服务端二进制**：移除 `--webui-dir` CLI 参数，通过 `rust-embed` 将 `webui/dist` 直接内嵌到二进制文件中；新增 `--log-level` 参数（环境变量 `NYRO_LOG_LEVEL`）替代硬编码的 tracing 过滤规则；关键参数支持环境变量（`NYRO_PROXY_HOST`、`NYRO_ADMIN_TOKEN` 等）
- **浏览器 Token 鉴权**：新增 `/login` 页面及浏览器 WebUI 的 Token 鉴权流程；管理 Token 生效时 WebUI 顶栏显示登出图标（Tauri IPC 路径不受影响）
- **资源启用/禁用切换**：在 WebUI 的 Provider、路由、API Key 列表页新增启用/禁用切换按钮；仅在资源被禁用时显示危险徽标

#### 改进

- **服务端 CLI 精简**：CLI 参数从 27 个减少至 18 个；`--admin-key` 重命名为 `--admin-token`，`--storage-dsn-env` 重命名为 `--postgres-dsn`；PostgreSQL 连接池参数统一加 `postgres-` 前缀；`--sqlite-migrate-on-start` 重命名为 `--migrate-on-start`；移除 9 个缓存相关 CLI 参数（现通过 Admin API / WebUI + DB 管理）
- **状态字段统一**：将 `providers.is_active`、`routes.is_active`、`api_keys.status` 统一重命名为 `is_enabled`（BOOLEAN 类型），覆盖所有存储后端、SQL 查询、Rust 模型与 WebUI；同时为 SQLite 和 PostgreSQL 提供非破坏性 schema 迁移

#### 修复

- 修复 feat #45 引入的两个新字段 `stream_replay_tps` 和 `expose_headers` 未在 nyro-server 缓存配置初始化代码（`main.rs` 和 `yaml_config.rs`）中添加，导致 CI 编译失败
- 修复 standalone 模式下代理 host/port 优先级 bug：CLI 传入值被默认值静默覆盖
- 修复 `backend.ts` 中 null-data bug：当 `data` 为 `null` 时，`json.data ?? json` 返回完整响应对象，导致 Provider 与 Settings 页面调用 `.trim()` 时崩溃

#### 文档

- 将 8 个过时设计文档合并为单一 `docs/design/architecture.md`
- 新增 `docs/standalone/` 目录，包含完整的 Standalone 模式使用指南及缓存章节
- 删除 `examples/` 目录（内容已整合至 standalone 文档）
- 修复 `docs/server/README.md`、`README.md`、`README_CN.md` 中的过时 CLI 参数说明

---

## v1.6.0

> 发布于 2026-04-12

#### 功能

- **端到端缓存系统**：实现模块化 exact/semantic 缓存后端，支持流式响应 SSE 缓存重放与 singleflight 请求合并，防止并发场景下的缓存穿透
- **路由入口别名**：新增无版本前缀的路由别名（`/chat/completions`、`/messages`、`/responses`、`/models/:model_action`），提升客户端兼容性
- **OpenAI 兼容模型列表接口**：新增 `/v1/models` 和 `/models` 接口，支持按路由返回模型列表，可按 API Key 过滤已绑定模型，并在 Key 无效时优雅降级为公开列表
- **语义向量维度生命周期管理**：Embedding 维度变更时自动重建向量表，持久化当前维度到配置，PostgreSQL 后端支持事务性重建并提供权限不足时的清晰引导说明

#### 改进

- **缓存系统统一**：统一 exact/semantic 缓存运行时配置与热重载行为；强制 chat/embedding 路由类型隔离，通过 OpenAI 端点校验保障路由安全；同步更新 WebUI 路由与设置流程
- **设置页保存体验升级**：重构设置模块为显式保存操作，增加未保存变更提示；将 API Key 状态拆分为管理状态与有效性标识；对齐 SQLite 语义缓存相似度评分与余弦距离预期
- **全局缓存/代理联动完善**：路由列表徽标与 Provider 列表代理徽标现根据全局开关联动；路由表单在全局缓存关闭时隐藏缓存控件，Provider 表单在全局代理关闭时隐藏代理开关；已保存配置在重新开启时自动恢复
- **语义缓存配置联动**：关闭语义缓存开关时自动清空 `embedding_route` 字段；被语义缓存引用的 embedding 路由禁止删除，并弹出错误提示引导用户先解除依赖

#### 修复

- 修复缓存命中日志中模型名显示不一致：在缓存条目中持久化 `actual_model`，保证命中日志上报真实的上游模型
- 修复全局缓存/代理开关与路由列表徽标及 Provider 代理徽标缺乏联动的问题
- 修复全局代理关闭后，启用了 `use_proxy=true` 的路由返回 502 的问题；改为自动降级为直连 HTTP 客户端
- 统一 UI 与文档中的缓存术语表述，不改变现有配置 Key 名称

#### 重构与清理

- **移除 MySQL 后端**：下线 MySQL 存储实现、配置/CLI 路径及 sqlx mysql feature；当前支持后端为 SQLite / PostgreSQL / Memory
- **GitHub 组织名更新**：将所有 `NYRO-WAY` 引用统一更新为 `nyroway`，涵盖配置、脚本、文档、安装脚本与前端代码
- 更新 Zai Provider 默认能力来源配置

---

## v1.5.0

> 发布于 2026-04-02

#### 功能

- **存储后端能力扩展**：新增多后端存储抽象，并在服务端提供 SQLite / MySQL / PostgreSQL 后端配置能力
- **多目标路由能力演进**：引入多目标路由选择与 weighted/priority 策略链路，支持 `weight=0` 作为显式禁用值
- **网关协议架构升级**：支持多协议 Provider、协议无关的路由行为，以及 standalone YAML 路由/Provider 加载
- **代理扩展性增强**：抽离 `ProviderAdapter`，并对齐 Provider 级代理控制逻辑，便于后续接入扩展

#### 改进

- **废弃字段统一清理**：移除路由/Provider/日志/存储中的历史遗留字段，简化现行路由链路相关 schema 与查询逻辑
- **网关错误类型标准化**：统一 proxy/auth 返回中的错误 `type` 为 `NYRO_*` 命名，便于客户端稳定识别与处理
- **CLI 接入体验优化**：改进 Web CLI 配置预览，并在 Claude Code 同步配置中加入 `CLAUDE_CODE_NO_FLICKER=1`
- **仓库迁移一致性完善**：将项目与发版引用统一迁移到 `NYRO-WAY` 组织，并同步 updater/发版脚本路径
- **构建与运行时结构整理**：拆分 Docker runtime 镜像与开发容器结构，降低 CI/CD 维护复杂度

#### 测试与文档

- 更新 smoke 测试与文档内容，使其与协议无关路由及最新 route/provider 数据模型保持一致

## v1.4.0

> 发布于 2026-03-21

#### 功能

- **协议归一化层升级**：新增语义级内部响应归一化，并在 Responses API 链路输出 item 级 reasoning / function-call 结果
- **Provider 预设能力统一**：统一 Provider 预设与能力源解析逻辑，并内置 models.dev 快照用于离线元数据
- **Connect CLI 流程增强**：Codex/OpenCode 同步输出与运行时默认配置对齐，优化路由状态锚定与配置动作体验

#### 改进

- **WebUI 配置交互优化**：细化 Provider 预设行为与路由编辑模型交互，提升管理流可预期性
- **管理端错误一致性增强**：后端返回结构化 provider/route 冲突信息，前端同步完成冲突错误本地化
- **CLI 面板布局打磨**：调整 API Key 与更新配置区块顺序，保持动作区半宽布局，并统一预览提示对齐与间距
- **本地体验默认值优化**：初始语言默认 `en-US`，日志页面请求时间按本地时区显示

#### 修复

- 修复跨协议工具调用语义问题：加强 tool-call/result 关联，并统一各适配器的 thinking/text delta 处理
- 修复 Google 模型发现鉴权路径与模型归一化问题，恢复管理端发现链路稳定性

#### 测试与文档

- 新增协议回归覆盖：tool id、finish reason、schema 映射与 provider-policy 移除相关行为
- 新增协议架构加固设计文档，并更新 README 与控制台 UI 截图

## v1.3.0

> 发布于 2026-03-18

#### 功能

- **新增 OpenAI Responses 转换链路**：支持 `/v1/responses` 请求/响应转换，提升与新一代 OpenAI 风格客户端的兼容性
- **Provider 模型测试流程升级**：引入分阶段测试流程，并统一 Provider 管理页动作反馈
- **新增 Ollama 能力探测**：按供应商与模型能力动态检查，自动处理工具调用支持差异
- **Gemini cURL 示例优化**：Connect 页生成 Gemini 示例时保留模型名中的 `:`（如 `gemma3:1b`）

#### 改进

- **Provider 交互一致性优化**：改进供应商/渠道同步逻辑，稳定路由编辑态重置行为
- **Route 模型发现行为优化**：仅在 Provider 配置了可用模型端点时启用发现下拉
- **管理页错误体验优化**：统一后端错误本地化与失败弹窗展示策略

#### 修复

- 修复 MiniMax + Codex 互通问题：针对 Responses API 入口规范化指令消息，避免上游因 `system` 角色拒绝请求
- 修复 OpenRouter 模型发现行为，并恢复 Provider 创建后的自动测试流程
- 修复 Windows 桌面端下拉/搜索选择异常：解决标题栏拖拽捕获与下拉点击事件冲突

## v1.2.0

> 发布于 2026-03-15

#### 功能

- **新增「接入」模块**：增加 `Connect` 页面与 `代码接入` / `CLI 接入` 双标签，支持按协议选择路由并生成 Python / TypeScript / cURL 示例
- **桌面端 CLI 集成**：支持 Claude Code、Codex CLI、Gemini CLI、OpenCode 的就绪检测与配置同步/恢复
- **CLI 配置预览与复制优化**：按文件展示将更新的片段，并在预览区域内置复制能力
- **API Key 权限模型升级**：受控路由改为默认拒绝（需显式绑定路由才可访问），并统一为 `sk-<32位hex>` 密钥格式
- **配额能力扩展**：新增 `RPD`（每天请求数），打通 API Key 数据模型、管理接口、前端表单与代理鉴权限流

#### 改进

- **API Key 页面重构**：创建/编辑表单按三段式重排（基本信息、访问权限、访问限额），统一宽度策略，编辑态下有效期与 Key 不可修改
- **Provider 表单优化**：API Key 输入支持显示/隐藏，修复编辑态 API Key 回显，并对齐创建/编辑布局行为
- **Route 表单一致性**：编辑布局与创建布局对齐，单行输入/下拉保持半宽展示
- **统计时间范围统一生效**：小时筛选覆盖概览、模型、提供商统计，并在 WebUI + 后端 + Tauri 命令链路保持一致

#### 修复

- 修复 `build-and-smoke` CI 对新鉴权流程的不兼容（移除过时 `--proxy-key`，改为创建并绑定 smoke API Key）
- 修复 CLI 同步参数命名不一致（`toolId` / `apiKey`），并增强前端错误信息解析
- 回退 Codex `wire_api` 为 `responses` 以兼容最新 CLI 行为
- 优化表单下拉/搜索面板视觉一致性与访问控制开关布局细节

#### CI 与发版

- 桌面端发版流程支持自动计算并回写 Homebrew Cask 校验和
- 更新路由/API Key 设计文档与安装说明，保持文档与实现一致

---

## v1.1.0

> 发布于 2026-03-13

#### 功能

- **路由匹配重构**：从模糊 `match_pattern` 切换为 `(ingress_protocol, virtual_model)` 精确匹配，支持 OpenAI / Anthropic / Gemini 接入
- **全新 API Key 体系**：新增 `api_keys` + `api_key_routes` 数据模型及完整 CRUD，默认密钥格式为 `sk-<32位hex>`
- **路由级访问控制**：先匹配路由，再在 `access_control` 开启时校验 API Key；支持按路由绑定或全局生效
- **API Key 配额能力**：在代理鉴权链路中新增 `RPM`、`TPM`、`TPD`、状态与过期时间校验

#### 改进

- **后端迁移与兼容处理**：
  - 新增并回填路由/Provider/日志字段（`ingress_protocol`、`virtual_model`、`access_control`、`channel`、`api_key_id`）
  - 现行流程移除旧的路由/Provider fallback 与 priority 机制
- **管理接口扩展**：服务端与 Tauri 管理 API/命令新增 API Key 管理能力
- **WebUI 路由与密钥体验升级**：
  - 新增 API Keys 页面，支持可搜索多选绑定路由
  - 创建路由时将提供商/模型同排展示，并自动将目标模型回填到虚拟模型
  - Provider 创建/编辑流程持久化并自动锚定供应商与渠道标识
- **UI 组件标准化**：引入并统一使用 shadcn 风格 `Badge`、`Switch`、`Checkbox`、`Dialog`、`Combobox`、`Command`、`Popover`、`MultiSelect`、`Tabs` 等组件
- **Provider 图标策略优化**：Provider 列表主图标优先展示供应商图标（亮色彩色、暗色纯色），协议胶囊图标保持协议维度
- **版本展示自动化**：设置页版本改为构建时注入，不再写死

#### 修复

- 修复搜索下拉面板背景透明导致内容混叠的问题
- 修复自定义下拉搜索过滤与 hover/高亮反馈问题
- Homebrew 安装文档改为标准 `brew install --cask nyro` 流程

#### 文档

- 新增路由与 API Key 设计文档：`docs/design/route-apikey.md`
- 新增 Provider Base URL/渠道设计说明：`docs/design/provider-base-urls.md`
- 更新 `README.md` 与 `README_CN.md` 安装命令及相关说明

---

## v1.0.1

> 发布于 2026-03-10

#### 改进

- **全平台 ARM64 / aarch64 原生构建**：使用 GitHub Actions ARM runner（`ubuntu-24.04-arm`、`windows-11-arm`、`macos-latest`）原生构建，零交叉编译
  - 桌面端：Linux aarch64 AppImage、Windows ARM64 NSIS 安装包
  - 服务端：Linux aarch64、macOS aarch64、Windows ARM64 二进制
- **macOS Intel 原生构建**：使用 `macos-15-intel` runner 原生编译，不再依赖 ARM 交叉编译
- **Homebrew Cask 支持**：`brew tap shuaijinchao/nyro && brew install --cask nyro`（独立 `homebrew-nyro` tap 仓库，发版自动同步版本）
- **一键安装脚本**：macOS/Linux（`install.sh`）和 Windows（`install.ps1`），macOS 自动移除隔离属性
- **前端 chunk 拆分**：Vite `manualChunks` 拆分 react/query/charts，消除 >500kB 打包警告

#### 修复

- **CI**：`cargo check --workspace` 排除 `nyro-desktop`，避免 Linux CI 依赖 GTK
- **CI**：移除 `cargo tauri build` 不支持的 `--manifest-path` 参数
- **CI**：添加 `pkg-config` 和 `libssl-dev` 依赖

#### 清理

- 移除桌面发布中的 MSI 和 deb 包（仅保留 NSIS + AppImage）
- 移除桌面 SHA256SUMS.txt（updater `.sig` 文件已提供完整性校验）
- Homebrew Cask 迁移至独立 `homebrew-nyro` 仓库
- 修复安装脚本和 README 中 `main` → `master` 分支引用

---

## v1.0.0

> 发布于 2026-03-09

Nyro AI Gateway 首个公开版本 — 从原 OpenResty/Lua API Gateway 完整重构为纯 Rust 本地 AI 协议网关。

#### 功能

- **多协议入口**：支持 OpenAI（`/v1/chat/completions`）、Anthropic（`/v1/messages`）、Gemini（`/v1beta/models/*/generateContent`），全协议支持流式（SSE）和非流式响应
- **任意上游出口**：可路由到任意 OpenAI 兼容、Anthropic、Gemini Provider
- **Provider 管理**：创建、编辑、删除 Provider，含 base URL 和加密 API Key
- **路由规则管理**：基于优先级的路由规则，支持模型覆盖和 Fallback Provider
- **请求日志持久化**：SQLite 存储，含协议、模型、延迟、状态码、Token 用量
- **用量统计看板**：概览仪表盘，含按小时/天图表和 Provider/模型维度分布
- **API Key 加密存储**：AES-256-GCM 加密静态存储
- **Bearer Token 鉴权**：代理层和管理层支持独立鉴权配置
- **桌面应用**：基于 Tauri v2 的跨平台桌面应用（macOS / Windows / Linux）
  - 系统托盘及快捷菜单
  - 可选开机自启
  - 应用内自动更新（Tauri updater）
  - macOS 原生标题栏融合
  - 深色/浅色模式切换
  - 中文/英文语言切换
- **服务端二进制**：独立 `nyro-server` 二进制，支持服务器部署，通过 HTTP 访问 WebUI
  - 代理端口和管理端口独立绑定地址配置
  - CORS 来源白名单配置
  - 非本地绑定时强制要求鉴权 Key
- **CI/CD**：GitHub Actions 自动化构建，支持跨平台桌面安装包和服务端二进制发布
