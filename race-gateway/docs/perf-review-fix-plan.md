# Race Gateway 性能审查修复计划

## 目标

这份文档把当前 `race-gateway` 的性能审查结论整理成可执行修复项，按热路径优先级推进，尽量保持外部接口和现有竞速语义不变。

修复原则：

- 优先处理代理主链路上的高频热点
- 优先减少异步锁竞争、全量扫描和重复对象拷贝
- 尽量不改动协议适配层和管理 API 对外契约
- 每一项修复完成后都补最小必要验证

## 优先级

### P1. 去掉每个流式事件的全量 buffered states 扫描

现状：

- `src/group/race_core.rs`
- `queue_event()` 每收到一个 event，都会调用 `total_buffered_events()`
- `total_buffered_events()` 会逐个 `lock` 全部 candidate state，再累加 `buffered_count`

问题：

- 热路径复杂度为 `O(事件数 x candidate数)`
- 在 chunk 很碎、并发较高时，CPU 和锁竞争会明显放大

修复方向：

- 引入 race 级共享 buffered counter
- 在 event 真正入队后按需原子递增
- 直接基于共享计数做 overflow 判断
- 不再为每个 event 扫描全部 candidate state

验收标准：

- 删除 `queue_event()` 内对全量 state 的逐次扫描
- 共享缓冲上限语义保持不变
- 现有竞速测试通过

### P2. 收敛 winner 选择路径中的多层循环和重复加锁

现状：

- `src/group/race_core.rs`
- `find_winner()` 采用 50ms 轮询
- 每轮会多次遍历 participant
- `can_win()` 又会重复锁定更高优先级 candidate state

问题：

- 存在 `O(n^2)` 级判断路径
- 更大的问题是异步锁被重复获取，放大延迟和调度成本

修复方向：

- 每轮先一次性收集按 participant 顺序排列的轻量状态快照
- winner 判断在内存快照上完成，而不是反复锁 state
- 保持现有保护窗口语义不变

验收标准：

- `find_winner()` 单轮只做一轮 state 加锁收集
- winner 选择结果与当前语义保持一致
- 相关单元测试通过

### P2. 减少每请求的模型目录全量 clone 和线性解析

现状：

- `src/data_plane/runner.rs`
- `src/app/cache.rs`
- 当前每个请求都会 `list_models()`，再把全量模型 clone 成 map
- 候选如果通过 `upstream_model` 解析模型，还会再次线性扫描

问题：

- 请求固定成本和模型总量正相关
- 配置规模增大后，请求入口会持续承担不必要的全量复制

修复方向：

- 在配置缓存内维护 `upstream_model -> model_id` 索引
- 请求路径只装配当前 group candidate 可能用到的模型子集
- 保留候选手工 `upstream_model` 语义

验收标准：

- 请求路径不再每次 clone 全量模型目录
- 手工 `upstream_model`、`model_id`、inline override 三种路径行为不变
- 现有解析/验证测试通过

### P3. 收敛 admin runtime snapshot 的重复解析和重复遍历

现状：

- `src/runtime/mod.rs`
- 构建 group runtime snapshot 时，会对同一 candidate 重复执行：
  - `resolve_model_for_candidate()`
  - `candidate_effective_upstream_model()`
  - `eligible_protocols()`
- `eligible_counts()` 还会按协议再次扫一轮 candidate

问题：

- 管理端请求不是最热路径，但 group / candidate / model 多了以后会越来越重
- 重复解析会让代码也更难维护

修复方向：

- 为单个 group 先构建 candidate resolved view
- 复用该结果生成：
  - candidate runtime snapshot
  - protocol eligible count
  - effective upstream model 展示

验收标准：

- runtime snapshot 内部不再重复解析同一 candidate
- 管理端返回结构不变

## 实施顺序

1. `P1` shared buffered counter
2. `P2` winner 选择快照化
3. `P2` 配置缓存模型索引与按需装配
4. `P3` runtime snapshot 复用 resolved view
5. `cargo check`
6. `cargo test --lib`

## 风险点

- buffered counter 改造需要确保 overflow 语义与当前行为一致
- winner 判断快照化不能改变响应保护时间窗口规则
- 模型缓存索引更新时，要确保 `put_model` / `delete_model` 不留下脏索引
- runtime snapshot 优化不能影响管理界面的字段兼容性

## 状态

- [x] P1 shared buffered counter
- [x] P2 winner 选择快照化
- [x] P2 模型缓存索引与按需装配
- [x] P3 runtime snapshot 复用 resolved view
- [x] `cargo check`
- [x] `cargo test --lib`
