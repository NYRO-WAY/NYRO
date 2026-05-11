# Race Gateway 架构设计

## 1. 目标

配套文档：

- [race-gateway-interface-draft.md](./race-gateway-interface-draft.md)
- [race-gateway-development-tasks.md](./race-gateway-development-tasks.md)

`race-gateway` 是一个独立服务，职责只有一件事：

- 对同一协议族下的多个 candidate 做并发竞速、胜出裁决、惩罚恢复、运行时观测

它和 `upstream-gateway` 是**相互独立**的两个服务：

- `race-gateway` 内置自己的 candidate / target 级 key 池、认证配置、竞速运行时状态
- `race-gateway` 不继承 `upstream-gateway` 那套 `provider + key + model` 维度的 RPM / RPD / TPM 配额管理语义
- `upstream-gateway` 不内置并发竞速、权重惩罚、winner 裁决

如果未来需要串联部署，可以把 `race-gateway` 的下游指向任意 provider-native endpoint，包括 `upstream-gateway`，但这不是 `race-gateway` 设计成立的前提。

## 2. 迁移原则

本次迁移不是只保留“概念名”，而是要尽量保留原 Python 项目里已经验证过的**行为语义**和**实现逻辑**。

必须保留的内容：

- `ModelGroup` / `Candidate` / `WeightTracker` / `RaceCore` 的职责划分
- 相对延迟调度公式
- 基于“本轮当前权重快照”的排序、阻塞判断、惩罚判断
- 低权重胜出后的单次惩罚与懒恢复
- bounded queue + backpressure 的缓冲策略
- 流式首个有效内容驱动的 winner 判定
- 已有可用内容后的 winner fallback 语义
- 协议适配器分层，而不是在核心调度里写死 Anthropic/OpenAI/Gemini 语义
- 运行时统计、诊断头、脱敏输出

不迁移的内容：

- 原 Python 单体服务里的 Claude 兼容映射入口
- 原 Python 管理页的视觉和交互
- 原 Python 项目里与 provider、鉴权、消息平台混在一起的组织方式

## 3. 边界

`race-gateway` 不是第二个 `nyro`，也不是第二个 `upstream-gateway`。

它的边界非常明确：

- 输入是已经完成协议转换的 provider-native 请求
- 输出是同协议族的 provider-native 响应
- 不做 OpenAI / Anthropic / Gemini 之间的跨协议转换
- 管理自己的 target / candidate 级 key 池与认证材料
- 不以 `provider + key + model` 维度实现 `upstream-gateway` 风格的 RPM / RPD / TPM 配额控制

因此保留下来的，是原项目的“模型组竞速内核”，不是原项目的整站 API 外壳。

## 3.1 设计取舍结论

为了避免“看起来更通用、实际偏离旧生产逻辑”，本设计明确采用以下取舍：

1. 同一个 `RaceGroup` 共享一份权重状态和竞速经验值，不按协议拆成多份。
2. 协议差异只放在 adapter 层，不放在 group 身份和权重域里。
3. `race-gateway` 自己持有 key 池，但 Phase 1 默认采用旧实现的“共享池 + 每请求随机选 key”。
4. candidate 级独立 key 池只作为后续可选扩展，不作为 Phase 1 默认语义。
5. 已经产出可用内容的 candidate，即使后续流失败，仍然可能胜出。
6. 整轮达到最大等待时间后，先尝试按排序选择“已有可用内容”的 candidate，再决定 all-failed。
7. 旧生产配置 guardrail 保留到 Phase 1：
   - 每组 candidate 数量 `<= 8`
   - `response_protection_timeout` 范围 `1..120s`

## 4. 服务形态

`race-gateway` 强制双端口：

- `RACE_GATEWAY_PROXY_BIND_ADDR`
- `RACE_GATEWAY_ADMIN_BIND_ADDR`

推荐默认值：

- proxy: `127.0.0.1:2090`
- admin: `127.0.0.1:2091`

proxy 端口职责：

- 接收 group 级 provider-native 请求
- 执行调度、并发发起、胜出裁决
- 流式透传 winner
- 提供 proxy 健康检查

admin 端口职责：

- group / candidate / model catalog CRUD
- runtime snapshot
- diagnostics / stats 查询
- 管理界面
- admin 健康检查

## 5. 路由边界

第一阶段采用 group-path 入口，而不是复刻原 Python 项目的外层模型名路由：

- `POST /groups/:group_id/openai/v1/chat/completions`
- `POST /groups/:group_id/openai/v1/responses`
- `POST /groups/:group_id/anthropic/v1/messages`
- `POST /groups/:group_id/google/v1beta/models/:model_action`
- `POST /groups/:group_id/google/models/:model_action`

健康与管理：

- proxy: `GET /healthz`
- admin: `GET /admin/healthz`
- admin UI: `GET /admin`

这样做的原因是：

- 原 Python 项目的“请求模型名 -> group / route / provider target”属于单体 API 外壳逻辑
- 在当前拆分架构里，这层不应该放进 `race-gateway`
- 但其核心竞速逻辑必须完整迁移

## 6. 与原项目对齐的核心对象

### 6.0 Timeout 术语约定

这里必须先把三个容易混淆的时间概念拆开：

- `request_timeout`
  - 指单个 candidate 下游 HTTP 请求的传输超时 / 请求超时
  - 用于连接、首字节、长时间无网络进展等传输层控制
  - 不参与并发竞速的 winner 判定
- `response_protection_timeout`
  - 指 candidate 发起后，“更低优先级候选即使先返回有效内容，也仍需继续等待”的保护时间窗口
  - 它是并发竞速核心语义的一部分
  - 原 Python 实现里很多地方把它直接命名为 `timeout`，新设计里明确改名，避免与请求超时混淆
- `race_max_wait_time`
  - 指一整轮竞速允许持续的最大总时长
  - 若超过该时长仍无 winner，则按 all-failed / fallback 逻辑收口

后文若未特别说明：

- “保护时间”默认指 `response_protection_timeout`
- “请求超时”默认指 `request_timeout`

### 6.1 RaceModelCatalog

保留原项目“模型管理”这一层，但用途收敛为：

- 给管理界面提供可选模型列表
- 给 candidate 配置提供下拉和校验
- 保存 candidate 目标端点的元数据

建议对象：

```text
RaceModelDescriptor
- id
- display_name
- upstream_model
- endpoints[]
- description
- enabled
- metadata optional
```

说明：

- `endpoints[]` 表示“同一个模型在不同协议族下可用的下游原生入口”
- 每个 endpoint 至少包含：
  - `protocol_family`
  - `base_url`
  - `auth_strategy`
  - `key_pool_id`
  - `request_timeout`
  - `extra_headers` / `extra_query`
- `base_url` 可以指向真实 provider，也可以指向任意兼容网关
- 这样 `race-gateway` 不依赖 `upstream-gateway` 的 provider_id 语义
- `auth_strategy` 描述如何把 key 注入请求，例如 bearer/header/query
- `key_pool_id` 描述该模型或 target 默认使用哪一个共享 key 池

### 6.2 RaceGroup

保留原项目的组级调度参数：

```text
RaceGroup
- id
- display_name
- fallback_ratio
- decay_factor
- penalty_rate
- recovery_rate
- race_max_wait_time optional
- enabled
- candidates[]
```

字段语义保持和原实现一致：

- `fallback_ratio`：后续候选相对前一候选的发起比例
- `decay_factor`：后续延迟衰减系数
- `penalty_rate`：低权重胜出后，对更高权重候选的一次性扣减量
- `recovery_rate`：当前权重每秒线性恢复量
- `race_max_wait_time`：整轮竞速最大等待时间；若为空，默认 `max(candidate.response_protection_timeout) * 3`

关键取舍：

- `RaceGroup` 本身不带协议归属字段
- 同一 `RaceGroup` 可以通过不同协议入口被调用
- 这些请求共享同一份 `WeightTracker`
- 统计展示可以按协议分桶，但惩罚/恢复/排序经验不能按协议拆开

### 6.3 RaceCandidate

保留原项目 candidate 的核心字段和优先级语义：

```text
RaceCandidate
- id
- group_id
- name
- model_ref optional
- upstream_model
- inline_endpoint_overrides optional
- initial_weight
- response_protection_timeout
- enabled
- metadata optional
```

字段语义：

- `initial_weight`：基础权重，静态值，定义恢复上限
- `response_protection_timeout`：首个有效响应保护时间；它用于竞速调度和 winner 阻塞判断，不是整个请求的硬超时
- `inline_endpoint_overrides`：可选，仅用于覆盖某个协议下的 endpoint / key 池 / 认证配置

约束：

- 同一 group 内 `candidate.name` 唯一
- `response_protection_timeout` 必须在 `1..120s`
- tie-break 采用配置顺序，保持和原实现一致的稳定排序
- 同一 candidate 不允许为同一 `protocol_family` 配置多个 enabled inline endpoint

说明：

- Phase 1 默认不把 key 池绑到 candidate 级
- candidate 认证默认继承其 `model_ref` 或 target 所绑定的共享 key 池
- candidate 级独立 key 池覆盖能力可以作为后续扩展，但不应改变 Phase 1 的默认语义

### 6.3.1 请求时的 endpoint 解析与参赛资格

这里需要把“group 不按协议拆”与“请求一定带协议入口”这两件事同时固定下来。

规则如下：

1. 每个请求先由 proxy 路由确定 `protocol_family` 和 `route_kind`。
2. `RaceCore` 开始前，先对每个 enabled candidate 解析当前协议对应的 endpoint。
3. endpoint 解析顺序固定为：
   - candidate 级 inline endpoint override
   - model descriptor 上同协议的 endpoint
4. 某个 candidate 若在当前协议下没有 enabled endpoint：
   - 该 candidate 本轮不参赛
   - 不参与本轮排序、阻塞、惩罚、participants 统计
   - 但其长期权重状态仍保留在 group 的共享 `WeightTracker` 中
5. 若一个 group 在某协议入口下没有任何可参赛 candidate：
   - 直接返回“该 group 未配置当前协议可用 target”的配置型错误
   - 不进入竞速

这条规则保证：

- group 级权重经验值仍按旧生产逻辑共享
- 但不会因为“某 candidate 没有 Anthropic endpoint”就强行参与 Anthropic 竞速
- 也不会把 group 错误地拆成“openai 组”和“anthropic 组”

### 6.4 RaceKeyPool

由于 `race-gateway` 也需要独立运行，因此必须保留自己的 key 池抽象。

建议对象：

```text
RaceKeyPool
- id
- display_name
- auth_strategy
- keys[]
```

```text
RaceKey
- id
- key_pool_id
- secret
- enabled
- weight optional
- metadata optional
```

语义：

- key 池属于 `race-gateway` 自身配置，不依赖 `upstream-gateway`
- Phase 1 默认采用旧生产语义：一个 target 或模型绑定一个共享 key 池
- 每次 candidate 发起下游请求时，从该共享 key 池随机选取一把 key
- 该 key 池主要服务于认证与竞速发起，不强制附带 `upstream-gateway` 风格的 quota runtime
- candidate 级独立 key 池、权重选 key、轮转策略扩展，不作为 Phase 1 必需语义

补充约束：

- Phase 1 的 key 选择仅发生在“candidate 已经通过当前协议 endpoint 解析之后”
- key 池是认证资源池，不参与 winner 排序
- key 选择失败只会让该 candidate 本轮失败，不改变 group 级权重模型

## 7. 调度公式

这里必须按原实现保留“相对延迟”，而不是改成绝对延迟表。

假设排序后的候选为 `C0, C1, C2 ...`：

- `C0` 立即发起，延迟为 `0`
- 第 `i` 个候选相对于第 `i-1` 个候选的延迟为：

```text
delay_i = response_protection_timeout_(i-1) * fallback_ratio * decay_factor^(i-1)
```

绝对发起时刻：

```text
T0 = 0
Ti = T(i-1) + delay_i
```

Rust 侧必须保留与原实现一致的 `compute_schedule(candidates, fallback_ratio, decay_factor)` 语义：

- 返回 `[(candidate, relative_delay)]`
- 每个 delay 都是相对上一候选，不是相对竞速开始

## 8. WeightTracker 语义

这里必须按原实现保留“当前权重”和“基础权重”的二元关系。

定义：

- `initial_weight`：配置静态值
- `effective_weight`：运行时当前权重

保留的行为：

1. 启动时 `effective_weight = initial_weight`
2. 不使用后台定时器恢复，而是 `tick()` 懒恢复
3. 恢复是线性的：`effective += recovery_rate * elapsed`
4. 恢复上限是 `initial_weight`
5. 惩罚时执行：
   - `effective = max(effective - penalty_rate, 0)`
6. 相等权重不互相惩罚
7. 竞速排序使用“本轮开始时的当前权重快照”

建议 Rust 侧保留同类接口：

```text
WeightTracker
- reconfigure(...)
- tick()
- apply_penalty(penalized_names)
- effective_weight(name)
- sorted_candidates(candidates)
- snapshot()
- reset()
```

注意：

- `reconfigure()` 要尽量保留仍然存在的 candidate 的 `effective_weight`
- 删除的 candidate 运行时权重可以直接丢弃
- 新增的 candidate 从 `initial_weight` 开始

## 9. RaceCore 语义

### 9.1 CandidateState

必须保留原实现的每候选运行时状态：

```text
CandidateState
- candidate
- queue
- launched_at
- first_content_at
- winner_selected
- failed
- ended
- error
- buffered_count
- relative_delay
- weight_snapshot
- protocol_state
```

其中：

- `queue` 是 bounded queue
- `protocol_state` 给协议适配器保存流收尾状态
- `first_content_at` 是判定 winner 的关键时间点

### 9.2 缓冲与反压

必须保留 bounded queue 语义，而不是让 loser 无限堆内存。

阶段一建议保持与原逻辑一致：

- `MAX_BUFFER_EVENTS = 100_000`
- `BUFFER_BACKPRESSURE_TIMEOUT = 0.1s`

语义：

- winner 未决时，写入 queue 要受超时保护
- winner 已决时，winner queue 走正常背压
- 若未决期间 queue 长时间写不进去，视为 `buffer overflow`
- `buffer overflow` 会把该 candidate 标记为失败

### 9.3 发起

必须保留 `_launch()` 语义：

- 按 schedule 逐个发起
- 每个 candidate 都记录 `launched_at`
- 每个 candidate 启一个 drain task

### 9.4 首个有效内容

必须保留 `_drain()` 里“第一次出现有效内容就记录 `first_content_at`”的逻辑：

- 不是首包就算
- 不是收到任何字节就算
- 必须由协议适配器判断什么是“有效内容”

### 9.5 胜出判定

必须保留 `_find_winner()` + `_can_win()` 这一套规则：

1. 候选顺序基于**本轮开始时的当前权重快照**
2. 低权重候选先收到有效内容时，不一定立即胜出
3. 只要有更高当前权重候选：
   - 已发起
   - 未失败
   - 距其 `launched_at` 仍在 `response_protection_timeout` 保护期内
   那么低权重候选就继续等待
4. 更高当前权重候选在以下情况下不再阻塞：
   - 明确失败
   - 未发起
   - 保护期已过且仍没有有效内容
   - 本轮因当前协议无 endpoint 而未参赛

补充保真语义：

- `failed` 不等于“彻底失去胜出资格”
- 只要某个 candidate 已经产出过可用内容，并且不是 `buffer overflow` 这类不可用缓冲状态，它仍然可能成为 winner
- 也就是说，winner 判定应基于“是否已有可用内容”而不是只看“最终是否 failed”

### 9.6 惩罚

winner 确认后：

- 找出本轮权重快照里排在 winner 前面、且权重严格高于 winner 的候选
- 对这些候选一次性应用 `penalty_rate`
- 相等权重不惩罚

### 9.7 超时后的 usable-content fallback

这里必须保留旧生产实现的 fallback 语义：

- 当整轮达到 `race_max_wait_time` 时
- 不应立刻按 all-failed 处理
- 必须先按当前排序顺序检查各 candidate 是否已有可用内容
- 若存在，则返回排序中第一个“已有可用内容且仍可发出”的 candidate 作为 winner
- 只有所有 candidate 都没有可用内容时，才进入 all-failed

这里的“可用内容”判断应等价于旧实现中的 usable-content 语义：

- 已产生 `first_content_at`
- 且不是 `buffer overflow` 造成的不可用缓冲状态

## 10. 协议适配器设计

必须保留“RaceCore + 协议适配器”的分层，不允许把协议语义写死在核心里。

建议抽象：

```text
RaceProtocolAdapter
- protocol
- is_content_event(event_bytes or event)
- feed_event(state, event)
- all_failed_stream(group_name, errors)
- fallback_close_stream(group_name, state)
```

### 10.1 OpenAI

保留原项目 `openai_chat_race.py` 的判断语义：

以下算有效内容：

- `choices[].delta.content`
- `choices[].delta.reasoning_content`
- `choices[].delta.tool_calls`
- 旧式 `choices[].delta.function_call`

以下不算：

- 只有 `role`
- 空 `delta`
- 只有 `usage`
- 只有 `finish_reason`
- `data: [DONE]`

### 10.2 Anthropic

保留原项目 `race.py` 的判断语义：

有效内容事件必须是：

- `event: content_block_delta`

并且包含以下任一 delta：

- `text_delta`
- `thinking_delta`
- `input_json_delta`

### 10.3 Gemini

Gemini 第一阶段也必须走同一适配器模式。

判定原则：

- 只在真正产生模型内容增量时触发 `first_content_at`
- 不能把只有 usage / finish / metadata 的 chunk 算成 winner 内容

实现要求：

- 设计阶段先定义 `GoogleRaceAdapter`
- 实现阶段按 Gemini 流式协议细分“有效内容字段”
- 不允许直接复用 OpenAI 或 Anthropic 的规则

## 11. 失败、取消、收尾

### 11.1 all-failed

必须保留原逻辑：

- 若所有 candidate 都失败，或在整轮超时内没有任何可用内容
- 返回该协议族自己的“全失败流”

### 11.2 winner 之后的 loser

winner 决定后：

- 取消所有 loser task
- 等待 loser 清理完成
- 只允许 winner 的缓冲和后续流继续对外输出

### 11.3 winner 中途失败

winner 若在已经胜出后中途异常：

- 不回切到 loser
- 由协议适配器补齐本协议的关闭序列

这个行为也应保持与原实现一致，避免把竞速语义复杂化成“winner 失效后重新竞选”。

补充：

- 若该 winner 在失败前已经积累了可用内容，则允许继续以“winner 已确定、后续做协议收尾”的方式结束
- 不因为 winner 后续失败就重新开启第二轮竞选

## 12. 运行时统计与诊断

### 12.1 RaceStats

保留原项目统计语义：

```text
RaceRecord
- id
- timestamp
- group
- protocol
- winner
- duration_ms
- buffer_events
- participants
- first_content_times
- penalty_applied
- errors
```

聚合视图至少包括：

- `total_races`
- `winner_distribution`
- `protocol_distribution`
- `by_protocol`
- `avg_race_duration_ms`
- `avg_buffer_events`
- `recent_races`

### 12.2 diagnostics header

保留原项目“可开关、默认可关闭、脱敏”的诊断设计。

建议保留一个 header 开关，例如：

- `ENABLE_RACE_DIAGNOSTICS_HEADER`

建议对外 header：

- `x-nyro-race-diagnostics`

header 中的诊断信息应包含：

- group
- protocol
- winner
- 是否 all_failed
- penalty_applied
- penalized_candidates
- duration_ms
- 每个 candidate 的：
  - name
  - upstream_model
  - masked key / masked auth identity
  - relative delay
  - launch_offset
  - first_content_offset
  - initial_weight
  - effective_weight
  - weight_deviation
  - status
  - failed
  - error

要求：

- key 必须脱敏
- diagnostics 关闭时不构建重对象、不做额外序列化

## 13. 持久化与运行时状态

### 13.1 SQLite 持久化

只持久化配置态：

- `race_models`
- `race_groups`
- `race_group_candidates`
- `race_key_pools`
- `race_keys`
- `race_settings`

说明：

- `race_models` 对应原项目的“模型管理”
- `race_groups` 对应组级配置
- `race_group_candidates` 对应候选配置
- `race_key_pools` / `race_keys` 对应 `race-gateway` 自己的认证与 key 池
- `race_settings` 用于全局设置，如 diagnostics header 开关、默认 buffer 策略等

Phase 1 配置约束也应在持久化层和管理 API 层同时校验：

- 每组 candidate 数量 `>= 1` 且 `<= 8`
- `response_protection_timeout` 范围 `1..120s`
- candidate 名称在组内唯一

### 13.2 内存运行态

以下内容不落盘，保留原项目的进程内状态模型：

- `WeightTracker`
- `RaceStats`
- 活跃竞速中的 `CandidateState`
- bounded queue
- diagnostics sink

原因：

- 这是热路径状态
- 原项目也是进程内模型
- 第一阶段不引入 Redis 或共享状态存储

## 14. 管理界面设计

管理界面不沿用旧 Python 页面的布局，改为参考当前 `nyro` WebUI 风格。

设计原则：

- 左侧导航 + 右侧详情/表单
- 统一使用 `nyro` 当前 WebUI 的布局、表单、badge、table、dialog 风格
- 运行时面板突出当前权重、状态色、winner 统计、最近竞速记录

建议页面：

- `Groups`
  - group 列表
  - group 基本参数编辑
  - candidate 列表与顺序调整
- `Models`
  - model catalog 管理
  - 下游 target 元数据管理
- `Runtime`
  - 当前有效权重
  - 当前协议可参赛 candidate 概览
  - candidate 状态
  - 最近 winner 分布
  - 按协议统计
- `Diagnostics`
  - 最近失败原因
  - 缓冲溢出 / 超时 / 全失败记录
- `Settings`
  - diagnostics header 开关
  - 运行时阈值类设置

重点：

- UI 风格对齐 `nyro`，不是复刻旧 Python admin
- 但 UI 展示的数据结构和行为语义，必须对齐原竞速实现

## 15. Rust 模块划分

建议目录：

```text
race-gateway/
├── src/
│   ├── main.rs
│   ├── app/
│   ├── config/
│   ├── domain/
│   ├── data_plane/
│   ├── control_plane/
│   ├── group/
│   │   ├── types.rs
│   │   ├── scheduler.rs
│   │   ├── weight_tracker.rs
│   │   ├── race_core.rs
│   │   ├── stats.rs
│   │   └── diagnostics.rs
│   ├── adapters/
│   │   ├── openai.rs
│   │   ├── anthropic.rs
│   │   └── google.rs
│   ├── downstream/
│   ├── storage/
│   └── web/
└── docs/
```

模块职责：

- `group/scheduler.rs`：相对延迟计算
- `group/weight_tracker.rs`：惩罚与懒恢复
- `group/race_core.rs`：协议无关竞速核心
- `group/stats.rs`：运行时统计
- `group/diagnostics.rs`：诊断对象与 header 序列化
- `domain/`：配置态对象、summary DTO、校验器、endpoint 解析规则
- `adapters/*`：协议特定内容判断与收尾
- `downstream/`：把 winner / candidate 请求发往下游 provider-native endpoint
- `storage/`：SQLite 配置持久化

## 16. 实现保真要求

实现阶段需要把以下内容作为“不能随意改语义”的保真点：

1. `compute_schedule` 必须是相对延迟，不得改成绝对延迟配置表。
2. `WeightTracker` 必须使用 lazy tick 恢复，不引入后台周期任务。
3. 排序、阻塞判断、惩罚判断，都基于“本轮当前权重快照”，不是基础权重。
4. 相等权重不惩罚。
5. loser 取消后不允许继续向客户端输出任何 chunk。
6. winner 中途失败后不做重新竞选，只做协议收尾。
7. queue 必须有上限，不能改成无限缓冲。
8. diagnostics 必须可关闭，且关闭时不引入额外热路径负担。
9. OpenAI / Anthropic / Gemini 的“有效内容”判断必须各自独立实现。
10. 同一 group 的权重状态不能按协议拆开。
11. 默认 key 池语义应保持“共享池 + 每请求随机选 key”。
12. `race_max_wait_time` 到达后必须先执行 usable-content fallback，再决定 all-failed。
13. 运行时权重和统计仍保留进程内模型，第一阶段不引入共享状态。

## 17. 结论

`race-gateway` 的正确迁移方式，不是简单抽象成“有个 group 和 candidate”，而是：

- 保持它作为**独立竞速服务**
- 只迁移原项目里真正决定行为的竞速内核
- 明确保留调度公式、权重恢复、winner 判定、bounded buffer、协议适配器、诊断统计这些关键语义
- 管理界面改用 `nyro` 风格，但数据结构和行为逻辑必须忠实于原实现

这样做的结果是：

- 服务边界清晰
- 与 `upstream-gateway` 低耦合
- 迁移后的行为误差最小
- 后续开发和测试更容易对照原项目逐项验收
