# Gateway 管理手册

本文档说明当前三个服务各自的管理入口、适用场景和日常运维动作。

## 1. 先判断该进哪个管理面

### 1.1 `nyro` 管理面

适合处理：

- 路由配置
- 协议转换入口
- API key 管理
- 主 WebUI 与主 Admin API

默认地址：

- `http://127.0.0.1:19531`

### 1.2 `upstream-gateway` 管理面

适合处理：

- provider 配置
- provider 绑定 key 池
- provider 维度模型限流规则
- provider 运行时使用量
- 上游限流相关排障

默认地址：

- `http://127.0.0.1:2081/admin`

### 1.3 `race-gateway` 管理面

适合处理：

- model catalog
- race group / candidate 配置
- key pool 配置
- 竞速运行时、赢家分布、最近错误
- Prometheus 指标与压测联调

默认地址：

- `http://127.0.0.1:2091/admin`

## 2. `upstream-gateway` 管理面

### 2.1 页面职责

当前管理页重点看两类信息：

- Provider 配置
  - base URL
  - 认证方式
  - key pool
  - model rules
- Runtime
  - 每个 provider / key / model 的当前限流占用
  - 是否接近或达到 RPM / RPD / TPM 限额

### 2.2 日常操作

新增 provider：

1. 进入 `Providers`
2. 新增或编辑 provider
3. 绑定 keys
4. 配置 model rule
5. 到 runtime 页面确认 provider 可见

排查限流：

1. 打开 runtime 面板
2. 先看 provider 级总览
3. 再看 key 和 model 的具体占用
4. 若某个 key 满额，优先看：
   - RPM 是否满
   - TPM 是否满
   - RPD 是否到日窗口上限

### 2.3 常用管理接口

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

## 3. `race-gateway` 管理面

### 3.1 页面结构

当前页面分为：

- `Overview`
  - 代理面、管理面、runtime group 数、指标入口
- `Models`
  - 模型目录与协议 endpoint
- `Groups`
  - 竞速组、candidate、保护时间、惩罚恢复参数
- `Key Pools`
  - candidate / model 会复用的认证 key 池
- `Runtime`
  - 当前权重、赢家分布、最近竞速
- `Diagnostics`
  - 最近错误竞速记录
- `Settings`
  - diagnostics header、buffer 阈值

### 3.2 Models

用于维护：

- `upstream_model`
- 协议族 endpoint
- `key_pool_id`
- 请求超时

推荐流程：

1. 先建 `Key Pool`
2. 再建 `Model`
3. 再在 `Group` 里引用该 `model_id`

### 3.3 Groups

用于维护：

- `fallback_ratio`
- `decay_factor`
- `penalty_rate`
- `recovery_rate`
- `race_max_wait_time_ms`
- candidate 顺序与初始权重
- `response_protection_timeout_ms`

重点理解：

- `response_protection_timeout_ms`
  - 是竞速保护时间
  - 不是 HTTP 请求总超时
- `race_max_wait_time_ms`
  - 是整轮竞速上限

### 3.4 Key Pools

用于维护：

- 竞速用认证 key 集合
- key 是否启用
- key pool 认证方式

当前默认行为：

- 每次请求从共享 key 池随机挑选一个 enabled key
- key 池不负责 `upstream-gateway` 风格的 RPM / TPM / RPD 配额

### 3.5 Runtime

这里最适合排查“竞速为什么没按预期选 winner”。

你可以看到：

- group 当前是否启用
- 每个 candidate 当前 `effective_weight`
- candidate 当前状态
  - `normal`
  - `recovering`
  - `penalized`
- 每个协议下当前可参赛 candidate 数
- 最近 winner 分布
- 最近竞速记录

页面上的进度条重点看：

- `Effective Weights`
  - 当前权重是否已经被惩罚拉低
- `Winner Distribution`
  - 长期是否过度偏向某个 candidate

### 3.6 Diagnostics

适合排查：

- all-failed
- 单个 candidate upstream 失败
- buffer overflow
- 某协议特定 chunk 判断异常

当前支持：

- group 过滤
- protocol 过滤
- 文本搜索

### 3.7 Settings

当前可管：

- `enable_race_diagnostics_header`
- `max_buffer_events`
- `buffer_backpressure_timeout_ms`

建议：

- 线上默认只在需要排查时打开 diagnostics header
- `max_buffer_events` 不要改得过大，否则 winner 未决阶段的内存峰值会上升

### 3.8 常用管理接口

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

## 4. Prometheus 与压测

### 4.1 `race-gateway` 指标

入口：

- `GET /admin/metrics`

当前重点指标：

- `race_gateway_http_requests_total`
- `race_gateway_http_request_duration_seconds`
- `race_gateway_active_races`
- `race_gateway_races_total`
- `race_gateway_race_duration_seconds`

### 4.2 内置压测工具

命令：

```bash
cd race-gateway
cargo run --bin race-loadtest -- \
  --url http://127.0.0.1:2090/groups/demo/openai/v1/chat/completions \
  --requests 100 \
  --concurrency 20 \
  --timeout-ms 60000 \
  --header content-type:application/json \
  --header x-api-key:test \
  --body-file examples/loadtest/openai-chat.json
```

它适合：

- 验证竞速首包延迟
- 对比不同 group 配置
- 结合 `/admin/metrics` 看吞吐和活跃竞速数

### 4.3 联动观察方法

推荐做法：

1. 先打开 `Runtime`
2. 再压测一轮
3. 同时看 `/admin/metrics`
4. 最后回到 `Diagnostics` 检查是否出现 all-failed / overflow

## 5. 管理面排障顺序

### 5.1 客户端请求慢

顺序：

1. 先看 `nyro` 是否转换异常
2. 若是 provider 限流问题，看 `upstream-gateway`
3. 若是 winner 选择慢，看 `race-gateway Runtime`
4. 若要量化，再看 `race-gateway /admin/metrics`

### 5.2 请求直接失败

顺序：

1. 先看 `nyro` 路由是否命中
2. 看 `race-gateway Diagnostics`
3. 看 `upstream-gateway runtime`
4. 再看三边服务日志

### 5.3 为什么 winner 不稳定

优先看：

- `fallback_ratio`
- `response_protection_timeout_ms`
- `penalty_rate`
- `recovery_rate`
- candidate 当前 `effective_weight`

## 6. 相关文档

- [统一启动与部署文档](./nyro-upstream-gateway-startup.md)
- [Race Gateway 架构设计](../design/race-gateway-architecture.md)
- [Race Gateway 压测与指标说明](../../race-gateway/docs/loadtest-and-metrics.md)
