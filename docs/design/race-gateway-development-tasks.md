# Race Gateway 开发任务清单

## 1. 目的

本文把以下设计文档拆成可直接执行的开发清单：

- [race-gateway-architecture.md](./race-gateway-architecture.md)
- [race-gateway-interface-draft.md](./race-gateway-interface-draft.md)

目标：

- 严格对齐旧生产并发竞速逻辑
- 拆清后端 Rust、存储、WebUI 的实际开发顺序
- 每项任务都能独立验收

## 2. 执行规则

1. 先固化核心竞速语义，再补管理面和 UI。
2. Phase 1 不主动引入“比旧生产更通用”的默认行为。
3. 任何看似合理的抽象，只要会改变 winner、惩罚、key 选择语义，都必须后置。
4. 每完成一阶段都要补回归测试。

## 3. 状态图例

- `[todo]` 未开始
- `[doing]` 进行中
- `[done]` 已完成
- `[blocked]` 受前置阻塞

## 3.1 当前实现状态

截至 `2026-05-08`，`race-gateway/` 已按设计完成可启动、可配置、可观测的独立服务实现，当前状态如下：

- `[done]` Phase 0 文档与工程骨架
- `[done]` Phase 1 配置态模型、校验与 SQLite 存储
- `[done]` Phase 2 旧生产核心竞速逻辑迁移
- `[done]` Phase 3 共享 key 池与下游请求装配
- `[done]` Phase 4 OpenAI / Anthropic / Google adapter
- `[done]` Phase 5 双端口 data plane 与 `RaceRunner`
- `[done]` Phase 6 Admin CRUD / validate / runtime / settings API
- `[done]` Phase 7 嵌入式 WebUI 管理面
- `[done]` Phase 8 单元测试、集成测试与启动烟测

补充说明：

- WebUI 最终采用“`webui/src` 源文件 + `sync-assets.ps1` 同步到 `src/web/assets`”的嵌入式静态资源方案，而不是额外引入 `Vite + React` 构建链。
- 运行时设置 `race_settings` 已接入真实热路径：
  - `enable_race_diagnostics_header`
  - `max_buffer_events`
  - `buffer_backpressure_timeout_ms`
- 当前已覆盖的关键验证包括：
  - 共享权重跨协议生效
  - all-failed 协议级收尾
  - Admin API CRUD 与 key 脱敏
  - 双端口启动与管理页静态资源可访问

## 4. 推荐分阶段顺序

### Phase 0. 文档与骨架

- `[todo]` RG-T0.1 创建 `race-gateway/` 目录骨架
  - 设计关联：
    - 架构文档 4, 15
    - 接口草案 1, 6
  - 交付：
    - `race-gateway/Cargo.toml`
    - `race-gateway/src/main.rs`
    - `race-gateway/src/lib.rs`
    - `race-gateway/docs/README.md` 或引用现有 `docs/design/`
  - 验收：
    - `cargo check` 可过

- `[todo]` RG-T0.2 固化文档引用关系
  - 设计关联：
    - 架构文档 1
  - 交付：
    - `docs/design/race-gateway-architecture.md`
    - `docs/design/race-gateway-interface-draft.md`
    - 本任务文档
  - 验收：
    - 文档内链有效

### Phase 1. 配置态模型与存储

- `[todo]` RG-T1.1 实现基础配置模块
  - 设计关联：
    - 架构文档 4
    - 接口草案 3, 4
  - 文件：
    - `race-gateway/src/config/app.rs`
    - `race-gateway/src/config/mod.rs`
  - 交付：
    - `RACE_GATEWAY_PROXY_BIND_ADDR`
    - `RACE_GATEWAY_ADMIN_BIND_ADDR`
    - `RACE_GATEWAY_DATABASE_URL`
    - `RACE_GATEWAY_BOOTSTRAP_JSON_PATH`
  - 验收：
    - 默认值可启动

- `[todo]` RG-T1.2 定义配置态 Rust 类型
  - 设计关联：
    - 架构文档 6
    - 接口草案 3, 4
  - 前置：
    - RG-T1.1
  - 文件：
    - `race-gateway/src/domain/types.rs`
    - `race-gateway/src/domain/mod.rs`
  - 交付：
    - `ProtocolFamily`
    - `AuthStrategy`
    - `KeySelectionStrategy`
    - `RaceTargetEndpoint`
    - `RaceModelDescriptor`
    - `RaceCandidate`
    - `RaceGroup`
    - `RaceKeyPool`
    - `RaceKey`
    - `RaceSettings`
    - `RaceModelSummary`
    - `RaceGroupSummary`
    - `RaceKeyPoolSummary`
    - `ProtocolStatsSnapshot`
    - `ValidationIssue`
    - `ValidationErrorResponse`
    - `ResolvedCandidateTarget`
  - 验收：
    - serde round-trip 测试通过

- `[todo]` RG-T1.3 实现配置校验器
  - 设计关联：
    - 架构文档 3.1, 6, 13
    - 接口草案 2, 12
  - 前置：
    - RG-T1.2
  - 文件：
    - `race-gateway/src/domain/validate.rs`
  - 交付：
    - candidate 数量 `1..=8`
    - `response_protection_timeout_ms` 范围校验
    - 组内 candidate 名称唯一
    - group 至少存在一个 enabled candidate
    - model endpoint 至少一条 enabled endpoint
    - 同一协议 endpoint 不允许重复 enabled
    - key pool 至少一把 enabled key
  - 验收：
    - 字段级错误返回结构稳定

- `[todo]` RG-T1.4 定义 SQLite schema 与 store trait
  - 设计关联：
    - 架构文档 13
    - 接口草案 6.1, 7
  - 文件：
    - `race-gateway/src/storage/mod.rs`
    - `race-gateway/src/storage/sqlite.rs`
  - 交付：
    - `RaceConfigStore` trait
    - `SqliteRaceConfigStore`
    - 表：
      - `race_models`
      - `race_model_endpoints`
      - `race_groups`
      - `race_group_candidates`
      - `race_key_pools`
      - `race_keys`
      - `race_settings`
  - 验收：
    - 新库自动建表
    - round-trip 测试通过

- `[todo]` RG-T1.5 实现 bootstrap 导入
  - 设计关联：
    - 接口草案 4, 7
  - 文件：
    - `race-gateway/src/storage/bootstrap.rs`
  - 交付：
    - 从 JSON seed models / groups / key_pools / settings
  - 验收：
    - 空库导入成功
    - 非空库默认不覆盖

### Phase 2. 旧生产核心竞速逻辑迁移

- `[todo]` RG-T2.1 迁移 `scheduler` 相对延迟逻辑
  - 设计关联：
    - 架构文档 7
    - 接口草案 12
  - 来源参考：
    - `free-claude-code-codex/providers/model_group/scheduler.py`
  - 文件：
    - `race-gateway/src/group/scheduler.rs`
  - 交付：
    - `compute_schedule(candidates, fallback_ratio, decay_factor)`
  - 验收：
    - 相对延迟测试与旧公式一致

- `[todo]` RG-T2.2 迁移 `WeightTracker`
  - 设计关联：
    - 架构文档 8
    - 接口草案 5.1, 6.3
  - 来源参考：
    - `free-claude-code-codex/providers/model_group/weight_tracker.py`
  - 文件：
    - `race-gateway/src/group/weight_tracker.rs`
  - 交付：
    - `tick`
    - `reconfigure`
    - `apply_penalty`
    - `snapshot`
    - `reset`
  - 验收：
    - lazy recovery 测试通过
    - 相等权重不惩罚测试通过

- `[todo]` RG-T2.3 迁移 `RaceStats`
  - 设计关联：
    - 架构文档 12
    - 接口草案 5.3, 5.4
  - 来源参考：
    - `free-claude-code-codex/providers/model_group/stats.py`
  - 文件：
    - `race-gateway/src/group/stats.rs`
  - 交付：
    - `RaceRecord`
    - `RaceStatsSnapshot`
    - `by_protocol`
  - 验收：
    - 统计聚合测试通过

- `[todo]` RG-T2.4 迁移 diagnostics payload
  - 设计关联：
    - 架构文档 12
    - 接口草案 5.5
  - 来源参考：
    - `free-claude-code-codex/core/race_diagnostics.py`
  - 文件：
    - `race-gateway/src/group/diagnostics.rs`
  - 交付：
    - header payload
    - key masking
    - error truncation
    - one-shot diagnostics sink
  - 验收：
    - header 序列化测试通过

- `[todo]` RG-T2.5 迁移 `RaceCore` 与 `CandidateState`
  - 设计关联：
    - 架构文档 9
    - 接口草案 5.2, 6.4
  - 来源参考：
    - `free-claude-code-codex/providers/model_group/race_core.py`
  - 文件：
    - `race-gateway/src/group/race_core.rs`
  - 交付：
    - bounded queue
    - `_launch`
    - `_drain`
    - `_find_winner`
    - `_can_win`
    - usable-content fallback
    - loser cancel cleanup
  - 验收：
    - “failed 但已有内容仍可胜出”测试通过
    - “race_max_wait_time 后 usable-content fallback”测试通过
    - “winner 后失败只做协议收尾、不重选”测试通过

### Phase 3. 共享 key 池与下游请求

- `[todo]` RG-T3.1 实现共享 key 池选择器
  - 设计关联：
    - 架构文档 3.1, 6.4
    - 接口草案 4.5, 6.2
  - 来源参考：
    - `free-claude-code-codex/providers/nvidia_nim/key_pool.py`
  - 文件：
    - `race-gateway/src/key_pool/mod.rs`
  - 交付：
    - `RandomKeyPoolSelector`
    - masked key snapshot
  - 验收：
    - 空池拒绝
    - disabled key 跳过

- `[todo]` RG-T3.2 实现下游 endpoint 解析
  - 设计关联：
    - 接口草案 4.1, 4.2, 4.3
  - 前置：
    - RG-T1.2
    - RG-T1.3
  - 文件：
    - `race-gateway/src/domain/resolve.rs`
  - 交付：
    - 按请求 `ProtocolFamily` 从 candidate/model 解析 endpoint
    - fallback 顺序：
      - candidate inline endpoint
      - model endpoint
    - 返回 `ResolvedCandidateTarget[]`
    - 过滤当前协议下不可参赛 candidate
  - 验收：
    - 缺失协议 endpoint 返回明确错误
    - 不可参赛 candidate 不进入本轮竞速

- `[todo]` RG-T3.3 实现下游请求构造器
  - 设计关联：
    - 接口草案 6.5
  - 文件：
    - `race-gateway/src/downstream/request.rs`
  - 交付：
    - auth 注入
    - extra headers / query
    - route_kind 拼接
  - 验收：
    - OpenAI / Anthropic / Google URL 构造测试通过

- `[todo]` RG-T3.4 实现 `reqwest` 下游 dispatcher
  - 设计关联：
    - 接口草案 6.5
  - 文件：
    - `race-gateway/src/downstream/mod.rs`
  - 交付：
    - `dispatch_stream`
    - 请求超时使用 `request_timeout_ms`
  - 验收：
    - mock downstream 测试通过

### Phase 4. 协议 adapter 迁移

- `[todo]` RG-T4.1 实现 Anthropic adapter
  - 设计关联：
    - 架构文档 10.2
  - 来源参考：
    - `free-claude-code-codex/providers/model_group/race.py`
  - 文件：
    - `race-gateway/src/adapters/anthropic.rs`
  - 交付：
    - `is_content_event`
    - `feed_event`
    - `all_failed_stream`
    - `fallback_close_stream`
  - 验收：
    - 旧 SSE 语义测试通过

- `[todo]` RG-T4.2 实现 OpenAI adapter
  - 设计关联：
    - 架构文档 10.1
  - 来源参考：
    - `free-claude-code-codex/providers/model_group/openai_chat_race.py`
  - 文件：
    - `race-gateway/src/adapters/openai.rs`
  - 交付：
    - role-only 首包不触发 winner
    - content / reasoning / tool_calls 触发 winner
    - `[DONE]` 收尾
  - 验收：
    - OpenAI chunk 识别测试通过

- `[todo]` RG-T4.3 实现 Google adapter
  - 设计关联：
    - 架构文档 10.3
  - 文件：
    - `race-gateway/src/adapters/google.rs`
  - 交付：
    - Gemini 有效内容判定
    - all-failed / fallback close 逻辑
  - 验收：
    - Google 流式 chunk 测试通过

### Phase 5. Data plane

- `[todo]` RG-T5.1 实现 app state 与双端口 router
  - 设计关联：
    - 架构文档 4, 5
  - 文件：
    - `race-gateway/src/app/mod.rs`
    - `race-gateway/src/main.rs`
  - 交付：
    - `build_proxy_router`
    - `build_admin_router`
  - 验收：
    - proxy/admin 路由隔离测试通过

- `[todo]` RG-T5.2 注册 proxy 路由
  - 设计关联：
    - 接口草案 8
  - 文件：
    - `race-gateway/src/data_plane/mod.rs`
  - 交付：
    - `/groups/:group_id/openai/v1/chat/completions`
    - `/groups/:group_id/openai/v1/responses`
    - `/groups/:group_id/anthropic/v1/messages`
    - `/groups/:group_id/google/v1beta/models/:model_action`
    - `/groups/:group_id/google/models/:model_action`
  - 验收：
    - 路由 smoke test 通过

- `[todo]` RG-T5.3 实现请求上下文解析
  - 设计关联：
    - 接口草案 6.4
  - 文件：
    - `race-gateway/src/data_plane/request.rs`
  - 交付：
    - `group_id`
    - `protocol_family`
    - `route_kind`
    - 原始 `HeaderMap`
    - 原始 `Bytes`
  - 验收：
    - 每种协议 family 的解析测试通过

- `[todo]` RG-T5.4 实现 `RaceRunner` 装配
  - 设计关联：
    - 架构文档 9, 10
    - 接口草案 6.4
  - 前置：
    - RG-T2.1
    - RG-T2.2
    - RG-T2.3
    - RG-T2.4
    - RG-T2.5
    - RG-T3.1
    - RG-T3.2
    - RG-T3.3
    - RG-T3.4
    - RG-T4.1
    - RG-T4.2
    - RG-T4.3
  - 文件：
    - `race-gateway/src/data_plane/runner.rs`
  - 交付：
    - 从 store 读取 group
    - 预加载 `model_id -> RaceModelDescriptor`
    - 建立本轮权重快照
    - 为每个可参赛 candidate 解析 endpoint + 共享 key 池
    - 选择 adapter 并进入 `RaceCore`
  - 验收：
    - 同一 group 的 Anthropic/OpenAI 请求共享权重测试通过
    - 当前协议无 endpoint 的 candidate 不阻塞 winner

### Phase 6. Control plane API

- `[todo]` RG-T6.1 实现 models CRUD
  - 设计关联：
    - 接口草案 9.2
  - 文件：
    - `race-gateway/src/control_plane/models.rs`
  - 验收：
    - PUT/GET/DELETE 全通过

- `[todo]` RG-T6.2 实现 groups CRUD
  - 设计关联：
    - 接口草案 9.3
  - 文件：
    - `race-gateway/src/control_plane/groups.rs`
  - 验收：
    - 非法 group 配置返回结构化错误

- `[todo]` RG-T6.3 实现 key pools CRUD
  - 设计关联：
    - 接口草案 9.4
  - 文件：
    - `race-gateway/src/control_plane/key_pools.rs`
  - 验收：
    - 保存 secret
    - 返回时默认脱敏

- `[todo]` RG-T6.4 实现 settings API
  - 设计关联：
    - 接口草案 9.5
  - 文件：
    - `race-gateway/src/control_plane/settings.rs`
  - 验收：
    - settings 可持久化

- `[todo]` RG-T6.5 实现 validate API
  - 设计关联：
    - 接口草案 9.6
  - 文件：
    - `race-gateway/src/control_plane/validate.rs`
  - 验收：
    - 字段级错误可直接用于 WebUI

- `[todo]` RG-T6.6 实现 runtime API
  - 设计关联：
    - 接口草案 10
  - 文件：
    - `race-gateway/src/control_plane/runtime.rs`
  - 验收：
    - `by_protocol`
    - `effective_weights`
    - `recent_races`

### Phase 7. WebUI

- `[todo]` RG-T7.1 建立独立 WebUI 工程
  - 设计关联：
    - 架构文档 14
    - 接口草案 11
  - 文件：
    - `race-gateway/webui/`
  - 实现建议：
    - Vite + React + TypeScript
    - UI 结构参考 `nyro/webui`
  - 交付：
    - `src/components/layout/`
    - `src/components/ui/`
    - `src/pages/`
  - 验收：
    - 能本地启动并访问 admin shell

- `[todo]` RG-T7.2 实现整体布局与导航
  - 设计关联：
    - 架构文档 14
  - 页面：
    - `Groups`
    - `Models`
    - `Key Pools`
    - `Runtime`
    - `Diagnostics`
    - `Settings`
  - 文件：
    - `race-gateway/webui/src/components/layout/*`
    - `race-gateway/webui/src/router.tsx`
  - 验收：
    - 侧边栏与路由切换完成

- `[todo]` RG-T7.3 实现 Groups 页面
  - 设计关联：
    - 接口草案 9.3, 11
  - 文件：
    - `race-gateway/webui/src/pages/groups/*`
  - 功能：
    - 列表
    - 新建/编辑/删除
    - candidate 拖动排序或序号调整
    - `response_protection_timeout_ms` 编辑
  - 验收：
    - 能创建符合校验规则的 group

- `[todo]` RG-T7.4 实现 Models 页面
  - 设计关联：
    - 接口草案 9.2, 11
  - 文件：
    - `race-gateway/webui/src/pages/models/*`
  - 功能：
    - 模型元数据编辑
    - 各协议 endpoint 编辑
    - 绑定 key pool
  - 验收：
    - model endpoint 可新增/删除

- `[todo]` RG-T7.5 实现 Key Pools 页面
  - 设计关联：
    - 接口草案 9.4, 11
  - 文件：
    - `race-gateway/webui/src/pages/key-pools/*`
  - 功能：
    - key pool 列表
    - key 条目新增/禁用
    - secret 输入与脱敏展示
  - 验收：
    - 保存后列表只显示 masked key

- `[todo]` RG-T7.6 实现 Runtime 页面
  - 设计关联：
    - 接口草案 10, 11
  - 文件：
    - `race-gateway/webui/src/pages/runtime/*`
  - 功能：
    - 当前有效权重
    - 状态色
    - 最近 winner 分布
    - 按协议统计
    - recent races 表格
  - 验收：
    - 高亮 recovering / penalized 状态

- `[todo]` RG-T7.7 实现 Diagnostics 页面
  - 设计关联：
    - 架构文档 12, 14
  - 文件：
    - `race-gateway/webui/src/pages/diagnostics/*`
  - 功能：
    - all-failed 记录
    - candidate 错误概览
    - buffer overflow / timeout 过滤
  - 验收：
    - 能按 group 和 protocol 过滤

- `[todo]` RG-T7.8 实现 Settings 页面
  - 设计关联：
    - 接口草案 9.5
  - 文件：
    - `race-gateway/webui/src/pages/settings/*`
  - 功能：
    - diagnostics header 开关
    - buffer 参数编辑
  - 验收：
    - 修改后立即持久化

- `[todo]` RG-T7.9 嵌入 admin 静态资源
  - 设计关联：
    - 架构文档 4
  - 文件：
    - `race-gateway/src/web/mod.rs`
    - `race-gateway/build.rs` 或静态资源嵌入方案
  - 验收：
    - admin 端口可直接访问 UI

### Phase 8. 回归与联调

- `[todo]` RG-T8.1 迁移旧生产核心回归测试
  - 设计关联：
    - 架构文档 16
    - 接口草案 12
  - 测试点：
    - 相对延迟
    - lazy recovery
    - 相等权重不惩罚
    - usable-content fallback
    - failed but usable can win
    - winner 后失败不重选
    - 共享 key 池随机选 key
    - 同一 group 跨协议共享权重

- `[todo]` RG-T8.2 做协议级集成测试
  - 设计关联：
    - 接口草案 8, 10
  - 测试点：
    - OpenAI stream
    - Anthropic stream
    - Google stream
    - all-failed 返回各自协议收尾

- `[todo]` RG-T8.3 做 admin API 集成测试
  - 设计关联：
    - 接口草案 9
  - 测试点：
    - CRUD
    - validate
    - runtime snapshot

- `[todo]` RG-T8.4 做 WebUI 联调
  - 设计关联：
    - 接口草案 11
  - 验收：
    - 从 UI 完整创建 key pool -> model -> group
    - 发一轮请求后 Runtime 页面能看到权重变化

## 5. WebUI 专项清单

这里单独列出前端同学可直接开工的任务顺序。

### UI-1 基础工程

- `[todo]` 建立 `race-gateway/webui` 工程
- `[todo]` 接入 `nyro` 风格的 layout / sidebar / header
- `[todo]` 定义 API client 和通用 error handler
- `[todo]` 定义 summary/detail/runtime 的 TypeScript DTO
- `[todo]` 接入 query cache 与轮询策略

### UI-2 配置页面

- `[todo]` `Models` 页面
- `[todo]` `Key Pools` 页面
- `[todo]` `Groups` 页面
- `[todo]` 表单校验与错误提示
- `[todo]` endpoint editor 支持按协议增删
- `[todo]` group candidate 顺序调整与协议可用性提示

### UI-3 运行时页面

- `[todo]` `Runtime` 页面
- `[todo]` `Diagnostics` 页面
- `[todo]` 统计图表或分布组件
- `[todo]` 按协议展示当前可参赛 candidate 数
- `[todo]` 展示 candidate 的 eligible protocol badges

### UI-4 系统设置

- `[todo]` `Settings` 页面
- `[todo]` diagnostics 开关
- `[todo]` buffer 参数设置

### UI-5 嵌入与回归

- `[todo]` 打包产物嵌入 Rust admin 服务
- `[todo]` 本地与 API 联调
- `[todo]` 页面级 smoke test

## 6. 建议 PR 切分

建议按下面顺序切 PR：

1. `RG-PR1` 骨架 + config + domain types + summary DTO + validation types
2. `RG-PR2` SQLite schema + store + bootstrap + validate API
3. `RG-PR3` scheduler + weight_tracker + stats + diagnostics
4. `RG-PR4` race_core + usable-content fallback 回归测试
5. `RG-PR5` key pool selector + candidate target resolver + downstream dispatcher
6. `RG-PR6` protocol adapters + proxy routes + RaceRunner 装配
7. `RG-PR7` admin CRUD + runtime API
8. `RG-PR8` WebUI shell + Groups/Models/Key Pools
9. `RG-PR9` Runtime/Diagnostics/Settings + 静态资源嵌入 + 联调收口

## 7. 最终验收标准

完成标准不是“服务能启动”，而是以下全部成立：

1. 同一 `RaceGroup` 在不同协议入口下共享同一份权重状态。
2. 旧生产核心竞速测试全部通过。
3. key 池默认行为是共享池随机选 key。
4. `race_max_wait_time_ms` 后 usable-content fallback 正常。
5. Admin API 可完整管理 models / groups / key pools / settings。
6. WebUI 可完整完成配置和运行时观测。
7. 双端口 proxy/admin 完整隔离。
