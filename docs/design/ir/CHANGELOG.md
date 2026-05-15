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

<!-- PR-1 及以后条目在合并后追加于此处 -->
