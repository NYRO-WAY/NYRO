# 更新日志

Nyro 的所有重要变更均记录在此文件中。

---

## v1.1.0

> 发布于 2026-03-13

#### 功能

- **路由匹配重构**：从模糊 `match_pattern` 切换为 `(ingress_protocol, virtual_model)` 精确匹配，支持 OpenAI / Anthropic / Gemini 接入
- **全新 API Key 体系**：新增 `api_keys` + `api_key_routes` 数据模型及完整 CRUD，默认密钥格式为 `sk-nyro-xxxx`
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
