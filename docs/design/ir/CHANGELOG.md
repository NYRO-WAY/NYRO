# IR 演进日志（CHANGELOG）

> 记录每次 IR 结构变更：新增字段/变体、语义变更、删除字段、重命名。  
> **格式规范**：每个 PR 合并后在此追加条目，格式参照下方模板。  
> 阅读顺序：最新条目在上方。

---

## 模板

```
## [PR-N] <标题> — YYYY-MM-DD

### 新增
- `TypeName::field_name: Type` — 说明

### 变更（语义或类型改动）
- `TypeName::field_name`: `OldType` → `NewType` — 原因

### 删除
- `TypeName::field_name` — 已被 X 替代

### 重命名
- `OldName` → `NewName` — 原因
```

---

## [PR-0] 设计文档骨架 — 2026-05-14

### 新增（文档）
- `docs/design/ir/FIELD_HOMING.md` — 字段归属决策表（4 协议全字段 × 归属/依据）
- `docs/design/ir/CHANGELOG.md` — 本文件
- `docs/design/ir/README.md` — 目录导航与 IR 设计概览

---

## [PR-1] IR 类型重塑 + 流式事件分层 + Schema 抽象 — 2026-05-15

### 新增

**新模块**
- `ir/cache.rs` — `CacheControl { ttl: CacheTtl, breakpoint_priority: u8 }` / `CacheTtl { Ephemeral5m, Ephemeral1h }`
- `ir/error.rs` — `AiError { kind, message, status_code, raw }` / `AiErrorKind` (15 变体) + `is_retryable()`
- `ir/ext.rs` — `ProtocolExt` 枚举 + `OpenAIChatExt` / `OpenAIResponsesExt` / `AnthropicExt` / `GoogleExt`
- `ir/schema.rs` — `SchemaObject` (JSON Schema 归一化，`to_google_wire()` 大写转换)

**ContentBlock 新变体**
- `ContentBlock::Thinking { thinking, signature? }` ← 重命名自 `Reasoning`（字段 `text` → `thinking`）
- `ContentBlock::RedactedThinking { data }` — Anthropic redacted thinking
- `ContentBlock::Audio { source: MediaSource }` — 音频内容块
- `ContentBlock::File { source: MediaSource }` — 文件内容块
- `ContentBlock::Document { source, title?, context?, cache_control? }` — Anthropic DocumentBlockParam
- `ContentBlock::SearchResult { content, source, title, cache_control? }` — Anthropic SearchResultBlockParam
- `ContentBlock::ServerToolUse { id, name, input, server_type?, cache_control? }` — 服务端工具调用
- `ContentBlock::ServerToolResult { tool_use_id, content, server_type?, cache_control? }` — 服务端工具结果
- `ContentBlock::Citation { cited_text, source }` — 引用块
- `ContentBlock::ExecutableCode { code, language?, id? }` — Google executableCode
- `ContentBlock::CodeExecutionResult { return_code, stdout, stderr, id? }` — 代码执行结果
- `ContentBlock::ContainerUpload { file_id, cache_control? }` — Anthropic 容器上传
- `ContentBlock::Refusal { refusal }` — 模型拒绝

**ContentBlock 已有变体扩展**
- `ContentBlock::Image`: `media_type/data` → `source: MediaSource` + `cache_control?`
- `ContentBlock::Text`: 新增 `cache_control?`
- `ContentBlock::ToolUse`: 新增 `cache_control?`
- `ContentBlock::ToolResult`: 新增 `is_error?` + `cache_control?`

**新类型**
- `MediaSource { Base64 { media_type, data }, Url(String), FileId { file_id, detail? } }`
- `DocumentSource { Base64Pdf, PlainText, Url, Blocks }`
- `ReasoningEffort { None, Minimal, Low, Medium, High, Xhigh, Budget(u32) }`

**AiRequest 新字段**
- `disable_parallel_tool_calls: Option<bool>` — 与 ANT `disable_parallel_tool_use` 对应
- `ext: Option<ProtocolExt>` — 协议域 Ext 载体

**ToolSpec 新字段**
- `strict: Option<bool>` — OAI + ANT strict schema 校验
- `cache_control: Option<CacheControl>` — ANT 工具级别缓存断点

**ReasoningConfig 扩展**
- `effort: Option<ReasoningEffort>` 类型从 `Option<String>` 改为强类型 enum
- `display: Option<String>` — ANT thinking display 模式

**AiResponse 新字段**
- `error: Option<AiError>` — 规范化错误（非 2xx 或内容过滤时填充）

**AiStreamDelta 新变体**
- `StreamDelta::ThinkingDelta(String)` ← 重命名自 `ReasoningDelta`
- `StreamDelta::ThinkingSignature(String)` ← 重命名自 `ReasoningSignature`
- `StreamDelta::StreamError { error: AiError }` — 流式中途错误
- `StreamDelta::UnexpectedEof` — 流被截断

### 变更（语义）
- `ContentBlock::Reasoning { text, signature }` → `ContentBlock::Thinking { thinking, signature }` — 字段名 `text` 改为 `thinking`；compat.rs 已更新做透明桥接
- `ResponseItem::Reasoning { text }` → `ResponseItem::Thinking { text }`
- `GenerationConfig`: 标注 `logit_bias` / `n` / `top_k` 为 TODO(PR-2) 待迁移到 ProtocolExt

### 删除
- `AiRequest::modalities` 字段 — 已移入 `OpenAIChatExt.modalities`

<!-- PR-2 及以后条目在合并后追加于此处 -->
