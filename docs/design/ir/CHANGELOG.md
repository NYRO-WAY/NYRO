# IR 演进日志（CHANGELOG）

> 记录每次 IR 结构变更：新增字段/变体、语义变更、删除字段、重命名。  
> **格式规范**：每个 PR 合并后在此追加条目，格式参照下方模板。  
> 阅读顺序：最新条目在上方。

---

## [PR-6] 删除 InternalRequest / InternalResponse + 清理 compat.rs — 2026-05-15

### 删除

**`protocol/types.rs`**
- `InternalRequest` struct — 全流程已用 `AiRequest` 替代
- `InternalResponse` struct — 全流程已用 `AiResponse` 替代

**`ir/compat.rs`**
- `From<InternalRequest> for AiRequest` / `From<AiRequest> for InternalRequest`
- `From<InternalResponse> for AiResponse` / `From<AiResponse> for InternalResponse`
- 与 `InternalRequest`/`InternalResponse` 相关的所有 by-value 转换函数
- Round-trip 测试（`round_trip_internal_request`）

### 变更

**保留**（仍用于 codec 内部辅助）
- `types.rs` 内：`InternalMessage`, `Role`, `MessageContent`, `ContentBlock`, `ImageSource`, `ToolCall`, `ToolDef`, `ResponseItem`, `StreamDelta`, `TokenUsage`, `ServerToolUsage`
- `compat.rs` 内：by-ref helpers (`ai_msg_to_old_ref`, `ai_tool_choice_to_value`, `ai_tool_spec_to_old_ref`) + StreamDelta 双向转换函数

**`codec/reasoning.rs`**：改为接受 `&mut AiResponse`
**`codec/tool_correlation.rs`**：完全重写，使用 `AiRequest`/`ir::Message`
**`pipeline.rs`**：移除 compat 圈子转换，`normalize_tool_results` 和 `reasoning` 直接调用

**4 个 ResponseFormatter**：移除 `let resp: InternalResponse = resp.clone().into()` 内部转换
**4 个 ResponseParser**：直接构造 `AiResponse`，移除 `AiResponse::from(InternalResponse {...})`

**`tests/protocol_conversion.rs`**
- 删除 `ir_compat_preserves_per_message_reasoning_extra`（测试已删除的 compat round-trip）
- 添加本地 `InternalRequest`/`InternalResponse` shim + `From` 实现，其余 36 个测试零改动

---

## [PR-5] Dispatcher / Provider Adapter / Cache 全切到新 IR — 2026-05-15

### 变更

**`CacheEntry`** (`cache/entry.rs`)
- `internal_response: Option<InternalResponse>` → `Option<AiResponse>`

**`cache/key.rs`**
- `build_cache_key` 参数 `&InternalRequest` → `&AiRequest`
- `CODEC_SCHEMA_VERSION` v2 → v3（字段映射变更，清空旧缓存）

**Integration Hooks** (`integrations/hooks.rs`)
- `RequestHook::on_request`: `&mut InternalRequest` → `&mut AiRequest`
- `ResponseHook::on_response`: `&mut InternalResponse` → `&mut AiResponse`

**Vendor 层**
- `Vendor::build_request`: `&mut InternalRequest` → `&mut AiRequest`
- `Vendor::parse_response`: return `InternalResponse` → `AiResponse`
- Hook 方法 (`pre_encode`, `post_parse`, `pre_request`, `on_stream_delta`) 全部切换新类型
- `VendorExtension` 及其 blanket impl 同步更新
- Ollama `pre_request`：`req.extra.remove(...)` 改为 `req.meta.vendor.ingress.remove(...)`

**`pipeline.rs`**
- `build_request` 参数 `&mut InternalRequest` → `&mut AiRequest`；移除 `ai_req = req.clone().into()` 中转
- `parse_response` 返回 `AiResponse`；移除 `ai_resp.into()` 中转
- `egress_path` 传参 `req.stream` → `req.stream.enabled`

**11 个 vendor mod.rs（批量）**
- `build_request` / `parse_response` 签名同步切换

**`StreamResponseAccumulator`** (`dispatcher/accumulator.rs`)
- 全面切换 `AiStreamDelta`；`into_internal_response()` 改为 `into_ai_response()` 返回 `AiResponse`

**Dispatcher** (`dispatcher/mod.rs`)
- 移除 `let mut internal: InternalRequest = request.into()`；全程直接使用 `request: AiRequest`
- `replay_cached_stream` / `split_text_deltas` / 新增 `ai_response_to_deltas` 切换为 `AiStreamDelta`

**`dispatcher/non_stream.rs` + `stream.rs`**
- 移除所有 `AiResponse::from(...)` 和 `ai_stream_delta_to_old(...)` 包装
- Accumulator 直接调用 `.apply_all(&ai_deltas)` / `.into_ai_response()`
- Cache entry `internal_response` 直接存 `AiResponse`

**`dispatcher/util.rs`**
- `request_has_image_input` / `extract_semantic_embedding_input` 改为 `&AiRequest`

### 不变
- `compat.rs` 本体保留（供测试和外部使用）
- 老 `InternalRequest`/`InternalResponse` 类型仍在 `types.rs`（PR-6 删除）
- Parser/Formatter/StreamParser/StreamFormatter 已在 PR-3/PR-4 完成切换

---

## [PR-4] Codec Parser + Formatter 全切换到 AiResponse / AiStreamDelta — 2026-05-15

### 变更

**4 个 trait 签名更新**
- `ResponseParser::parse_response` 返回 `AiResponse`（原 `InternalResponse`）
- `ResponseFormatter::format_response` 参数改为 `&AiResponse`
- `StreamParser::parse_chunk` / `finish` 返回 `Vec<AiStreamDelta>`
- `StreamFormatter::format_deltas` 参数改为 `&[AiStreamDelta]`

**`compat.rs` 新增 StreamDelta 双向转换**
- `old_stream_delta_to_new` — `&OldStreamDelta → AiStreamDelta`
- `ai_stream_delta_to_old` — `&AiStreamDelta → OldStreamDelta`

**4 套 Codec 实现（stream.rs + parser.rs + formatter.rs）**
- Parsers：内部仍构造 `InternalResponse`，边界 `Ok(AiResponse::from(...))` 转换
- Formatters：入口 `let resp: InternalResponse = resp.clone().into();` 转换
- StreamParsers：内部构造 `Vec<StreamDelta>`，出口 `.map(old_stream_delta_to_new)` 转换
- StreamFormatters：入口 `let old: Vec<StreamDelta> = deltas.iter().map(ai_stream_delta_to_old).collect();` 转换
- Embeddings 存根签名同步更新

**Dispatcher 适配（stream.rs + non_stream.rs + mod.rs）**
- 所有 `format_response(&internal)` 改为 `format_response(&AiResponse::from(internal.clone()))`
- 所有 `accumulator.apply_all(&deltas)` 在 `Vec<AiStreamDelta>` 上加 `ai_stream_delta_to_old` 转换
- `replay_cached_stream` 中旧 `StreamDelta` 先转新再传给 formatter

**Provider 层适配**
- `LegacyStreamParserAdapter`：`parse_chunk` / `finish` 将 `Vec<AiStreamDelta>` 转换回 `Vec<StreamDelta>` 供 ProviderStreamParser 调用方
- `pipeline.rs::parse_response`：codec 返回 `AiResponse` 后 `.into()` 转回 `InternalResponse`

### 不变
- `StreamResponseAccumulator` 仍用 `StreamDelta`（PR-5 会迁移）
- Provider Vendor trait 签名不变（PR-5 迁移）

---

## [PR-3] Codec Encoder 全切换到 AiRequest — 2026-05-15

### 变更

**`EgressEncoder` trait**
- `encode_request` 参数类型由 `&InternalRequest` → `&AiRequest`

**4 大 Encoder 重写**
- `OpenAIEncoder` — 直接读取 `AiRequest`；`req.extra` → `req.meta.vendor.ingress`；标量字段改用 `req.generation.*`
- `ResponsesEncoder` — 同上；消息先通过 `ai_msg_to_old_ref` 转换后复用原有逻辑
- `AnthropicEncoder` — 同上；`__anthropic_raw_*` 字段从 `ingress` 读取
- `GoogleEncoder` — 同上；`__google_*` 字段从 `ingress` 读取

**`EmbeddingsEncoder` 更新**
- 参数同步改为 `&AiRequest`；`req.extra` → `req.meta.vendor.ingress`

**`compat.rs` 新增 by-ref 辅助**
- `ai_msg_to_old_ref` / `ai_tool_choice_to_value` / `ai_tool_spec_to_old_ref`

**`pipeline.rs` 适配**
- `build_request` 步骤 4 加一行：`let ai_req = req.clone().into();` 后调用 `encoder.encode_request(&ai_req)`

**集成测试更新**
- `protocol_registry.rs`：直接使用 decoder 返回的 `AiRequest` 传给 encoder（移除 `.into()` 中转）
- `protocol_conversion.rs`：`InternalRequest` 构造的测试改为 `.encode_request(&req.clone().into())`

### 不变
- Parser / StreamParser / Formatter / StreamFormatter 均未修改
- `compat.rs` 核心双向转换逻辑不变

---

## [PR-2] Codec Decoder 全切换到 AiRequest — 2026-05-15

### 变更

**`IngressDecoder` trait**
- `decode_request` 返回类型由 `InternalRequest` → `AiRequest`

**`GenerationConfig` 清理**
- 移除临时字段 `logit_bias`、`n`、`top_k`（已归属 `ProtocolExt`）

**4 大 Decoder 重写**
- `OpenAIDecoder` — 直出 `AiRequest`；`ProtocolExt::OpenAiChat(OpenAIChatExt)`
  - `audio / modalities / logit_bias / n / prediction / stream_options` 进 `OpenAIChatExt`
  - `service_tier / user` 进 `meta.vendor.ingress`（老 encoder 向后兼容）
  - `reasoning_effort` → `ReasoningConfig`；`stop` → `GenerationConfig.stop`
- `ResponsesDecoder` — 直出 `AiRequest`；`ProtocolExt::OpenAiResponses(OpenAIResponsesExt)`
  - `background / previous_response_id / truncation / include` 进 `OpenAIResponsesExt`
  - `reasoning` 字段 → `ReasoningConfig`；`reasoning_content` 附加到 `Message.meta`
- `AnthropicDecoder` — 直出 `AiRequest`；`ProtocolExt::Anthropic(AnthropicExt)`
  - `ContentBlock` 全面升级：`Thinking`、`Document`、`Audio`、`cache_control` 原生支持
  - 内置工具进 `AnthropicExt.server_tools`；用户工具进 `AiRequest.tools`（带 `cache_control`）
  - `thinking` → `ReasoningConfig`；`stop_sequences` → `GenerationConfig.stop`
  - 原始 wire JSON 保留在 `meta.vendor.ingress`（`__anthropic_raw_*`，兼容旧 encoder）
- `GoogleDecoder` — 直出 `AiRequest`；`ProtocolExt::Google(GoogleExt)`
  - `decode_with_model` 签名同步更新（model + stream 由 URL 路径注入）
  - `executableCode / codeExecutionResult` → `ContentBlock::ExecutableCode / CodeExecutionResult`
  - `thought=true` Part → `ContentBlock::Thinking`
  - `generationConfig` 扩展字段进 `GoogleExt`；`safety_settings` → `AiRequest.safety_settings`
  - `__google_*` 原始字段保留在 `meta.vendor.ingress`（兼容旧 encoder）

**`EmbeddingsDecoder` 更新**
- 返回类型同步改为 `AiRequest`；`__emb_*` 键保留在 `meta.vendor.ingress`

**5 个 Ingress Handler + Dispatcher**
- 移除 `let request: AiRequest = internal.into()` 一行（decoder 直出 `AiRequest`）

**`compat.rs` 修复**
- `block_to_old` 补充 `MediaSource::Url` / `MediaSource::FileId` → `OldContentBlock::Image` 映射

### 不变
- Encoder / Parser / Formatter 均未修改；通过 `AiRequest → InternalRequest`（compat.rs）继续工作
- `compat.rs` 双向转换逻辑核心不变

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
