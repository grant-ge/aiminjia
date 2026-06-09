# lotus-app vs claude-code-best 上下文压缩机制详细对比分析

> 分析日期：2026-06-01
> 对标基准：`/Users/a20250311/github/claude-code-best`
> 关联文档：
> - `docs/archive/2026-05/2026-04-17-full-gap-assessment.md`（哲学差距 Φ3）
> - `docs/archive/2026-05/superpowers/plans/2026-04-18-plan-k-autocompact.md`（Plan-K 实现记录）
> - `docs/archive/2026-05/superpowers/plans/2026-04-19-plan-ad-token-and-thinking.md`（Token 预算缺口）
> - `docs/archive/2026-05/superpowers/plans/2026-04-24-message-storage-architecture-fix-plan.md`（存储架构修复）

---

## 一、概述

本文档系统性地对比 lotus-app（AIjia）和 claude-code-best 的上下文压缩（Context Compaction）机制。lotus-app 已在 Plan-K 中实现了基础的四阶段预处理管线，但在架构深度、功能完整性、token 预算管理、状态持久化等方面与 claude-code-best 仍存在显著差距。

---

## 二、当前项目（lotus-app）实现全景

### 2.1 架构层级

```
transport/tauri_commands/chat.rs
  → TauriLegacyTurnExecutor::load_history()     ← 历史加载（现已委托 history.rs）
  → RuntimeChatTurnDriver::run_chat_turn_s4()   ← 主循环
    → prepare_messages_for_llm()                ← 四阶段预处理管线
      ├─ Stage 1: apply_tool_result_budget()    ← 工具结果预算裁剪 (64K chars)
      ├─ Stage 2: microcompact()                ← 清旧 tool results (120K chars 触发)
      ├─ Stage 3: collapse_tool_results()        ← 压缩重复/超长结果 (8K chars)
      └─ Stage 4: compact_messages_via_llm()    ← LLM 摘要压缩 (480K chars 触发)
    → run_llm_step()                            ← LLM 调用
```

### 2.2 各阶段详情

#### Stage 1: `apply_tool_result_budget()` (`preprocess.rs:228-274`)

- **触发**：Always runs（无条件）
- **预算**：`aggregate_char_budget = 64,000`（约 16K tokens）
- **保留规则**：
  - 错误结果（`isError: true`）
  - 含文件 ID 的结果（`fileId:` 或 `"fileId"`）
  - 最近 N 条（默认 2）
  - 保留工具名（从 `TOOL_CATALOG.preserve_tool_use_results`）
- **替换**：`[budget-trimmed]\n{preview}\n[trimmed X chars...]`
- **输出**：`ToolResultBudgetResult { messages, executed, tokens_freed_estimate }`

#### Stage 2: `microcompact()` (`compaction.rs:114-218`)

- **触发**：`total_chars >= trigger_chars`（默认 120,000 / ~30K tokens）
- **策略**：收集所有 `role: "tool"` 消息，保留最近 N 条（默认 2），其余替换
- **保护规则**：
  - `preserved_tool_names` 中的工具不清理
  - 保留最近 N 条 tool result
- **替换**：`"[microcompacted]"`（纯占位符，无预览）
- **输出**：`MicrocompactResult { messages, executed, tokens_freed_estimate }`

#### Stage 3: `collapse_tool_results()` (`preprocess.rs:276-312`)

- **触发**：Always runs（无条件）
- **阈值**：`long_result_chars = 8,000`
- **替换**：`[collapsed]\n{preview}\n[{reason} tool result hidden: original size X chars]`
- **原因**：`"duplicate"` 或 `"long"`
- **保护规则**：同 Stage 1（错误、文件 ID、最近、保留工具）

#### Stage 4: `compact_messages_via_llm()` (`compaction.rs` 核心)

- **触发**：`total_chars >= threshold_chars`（默认 480,000 / ~120K tokens）
- **特殊触发**：`PromptTooLongRecovery` 时无条件强制触发
- **熔断保护**：连续失败 3 次后跳过
- **流程**：
  1. 找到最后一条非 `isCompactSummary` 的 user 消息 → 作为 tail 起点
  2. 保留该 user 消息及其后所有消息（完整 tail round）
  3. 调用 `CompactSummaryClient::compact_summary()` 生成摘要
  4. 组装：`[boundary_system] + [summary_user] + [tail_round]`
- **边界标记**：`subtype: "compact_boundary"` 的 system 消息
- **持久化**：`compact_boundaries.jsonl`（追加写）

### 2.3 熔断保护 (`AutoCompactState`)

```rust
pub struct AutoCompactState {
    pub compacted: bool,
    pub turn_counter: u32,
    pub consecutive_failures: u32,       // >= 3 时熔断
}
```

- 成功时重置 `consecutive_failures = 0`
- 失败时递增，>= 3 在**当前 turn 内**跳过后续 compact 尝试
- 状态存在 `TurnIterationState` 中，每个 turn 通过 `TurnIterationState::new()` 重新初始化（per-turn 熔断，非跨 turn 持久化）

### 2.4 Token 估算

所有 token 估算使用：`chars / 4`（纯字符数 / 4）
- `estimate_total_chars()`：JSON 序列化后统计
- 无真实 tokenizer，无 provider-aware context window 感知

### 2.5 现存关键问题

| # | 问题 | 来源 | 影响 |
|---|---|---|---|
| G1 | `CompactSummaryClient` 无生产实现 | `compact_client.rs` | auto-compact 实际从未触发（返回空摘要）|
| G2 | Token 估算 = chars/4，精度低 | `compaction.rs` | 触发阈值不准，可能过早或过晚 compact |
| G3 | 无 context window 感知 | 全局 | 不知道剩余窗口空间，无法预测 overflow |
| G4 | 熔断器是 per-turn 的 | `compaction.rs:245` | `record_success()` 清零 + `TurnIterationState::new()` 每 turn 重建，不存在永久跳过 |
| G5 | 前端 compact 完成结果展示未实现 | 前端 | compact 后 token 节省提示未展示（Compacting spinner 已存在） |
| G6 | `context_decay.rs` 的旧 decay 未被清理 | `llm/context_decay.rs` | 旧逻辑与新管线并存，可能冲突 |
| G7 | 前端 preprocess 前置检查不匹配 | `preprocess.rs` | 部分场景下 compact 被跳过 |

---

## 三、claude-code-best 实现全景

### 3.1 三层架构

```
src/query.ts（主循环）
  ├─ Layer 1: microcompactMessages()           ← 本地清理旧 tool results
  │   └─ src/services/compact/microCompact.ts
  ├─ Layer 2: sessionMemoryCompact()           ← 零 API 成本，用 Session Memory 做摘要
  │   └─ src/services/compact/sessionMemoryCompact.ts
  └─ Layer 3: compactConversation()            ← 传统 LLM 摘要压缩
      └─ src/services/compact/compact.ts
          ├─ stripImagesFromMessages()         ← 预清理大图
          ├─ stripReinjectedAttachments()      ← 预清理可重建的附件
          ├─ compactConversation()             ← LLM 调用生成摘要
          └─ buildPostCompactMessages()        ← 组装 compact 后消息
              ├─ preservedSegment 保留         ← 完整保留最近 round
              └─ postCompactRestore            ← 重注入文件/skill/MCP 上下文
```

### 3.2 各层详情

#### Layer 1: MicroCompact（`microCompact.ts`）

- **可 compact 的工具**：`FILE_READ`, `SHELL`, `GREP`, `GLOB`, `WEB_SEARCH`, `WEB_FETCH`, `FILE_EDIT`, `FILE_WRITE`
- **替换文本**：`'[Old tool result content cleared]'`
- **图片处理**：>2000 tokens 的图片内容也清除
- **缓存变体**（Ant-only）：`CACHED_MICROCOMPACT` feature flag 控制，使用 `cache_edits` blocks

#### Layer 2: Session Memory Compact（`sessionMemoryCompact.ts`）

- **条件**：需要 `tengu_session_memory` + `tengu_sm_compact` feature flags
- **核心优势**：零 API 调用，使用已提取的 Session Memory 作为摘要
- **窗口计算**：
  ```typescript
  minTokens: 10_000        // 最少保留 token
  minTextBlockMessages: 5  // 最少文本消息条数
  maxTokens: 40_000        // 硬上限
  ```
- **算法**：`calculateMessagesToKeepIndex()` → 从 `lastSummarizedMessageId` 开始反向累计，满足 minTokens + minTextBlockMessages 后停止
- **工具对保护**：`adjustIndexToPreserveAPIInvariants()` → 确保 tool_use/tool_result 配对完整
- **Thinking 块保护**：Forward scan 找出与保留 assistant 消息共享 message.id 的 thinking block

#### Layer 3: Traditional API Compaction（`compact.ts`）

- **预清理**：
  - `stripImagesFromMessages()`：替换图片为 `[image]` 文本
  - `stripReinjectedAttachments()`：移除 `skill_discovery`/`skill_listing`（节省 tokens，压缩后重新注入）
- **核心流程**：`compactConversation()` → 独立的 LLM 调用生成摘要
- **边界消息**：`type: 'compact_boundary'`, `subtype: 'compact_boundary'`
- **preservedSegment** 记录：
  ```typescript
  compactMetadata.preservedSegment = {
    headUuid: string,    // 第一条保留消息
    anchorUuid: string,  // 与摘要的边界消息
    tailUuid: string,    // 最后一条保留消息
  }
  ```

#### Post-Compaction Reinjection（`buildPostCompactMessages`）

在 compact 完成后，有专门的 **50K token 预算** 重注入重要的上下文：

```typescript
POST_COMPACT_MAX_FILES_TO_RESTORE = 5
POST_COMPACT_TOKEN_BUDGET = 50_000
POST_COMPACT_MAX_TOKENS_PER_FILE = 5_000
POST_COMPACT_MAX_TOKENS_PER_SKILL = 5_000
POST_COMPACT_SKILLS_TOKEN_BUDGET = 25_000
```

**重注入内容**：
1. 最近读取的文件（最多 5 个，每个截断到 5K tokens）
2. 激活的 Skill（最多 25K total，5K per skill）
3. CLAUDE.md 内容
4. MCP 工具发现结果

### 3.3 Compact Boundary 视图机制

```typescript
// 核心函数：只返回 last boundary 之后的消息
function getMessagesAfterCompactBoundary(messages: Message[]): Message[] {
  const lastBoundary = messages.findLastIndex(m => isCompactBoundaryMessage(m))
  return lastBoundary >= 0 ? messages.slice(lastBoundary + 1) : messages
}
```

**关键原则**：所有操作（包括 microcompact、auto-compact 检查）都只对 boundary 之后的消息执行，确保 compact 后的历史不会被重复处理。

### 3.4 Auto-Compact 触发与熔断

```typescript
// 阈值计算
effectiveContextWindow = getContextWindowForModel(model) - MAX_OUTPUT_TOKENS_FOR_SUMMARY
autoCompactThreshold = effectiveContextWindow - AUTOCOMPACT_BUFFER_TOKENS (13_000)

// 警告阈值
WARNING_THRESHOLD_BUFFER_TOKENS = 20_000
ERROR_THRESHOLD_BUFFER_TOKENS = 20_000

// 熔断
MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES = 3
```

**关键区别**：阈值是 **动态的**（基于模型 context window 计算），而非固定常量。

### 3.5 PTL 应急恢复（`truncateHeadForPTLRetry`）

当 compact 请求本身触发 Prompt Too Long 时：
1. **Group-based truncation**：按 API round 分组丢弃最旧组
2. **Fallback**：如果 token gap 无法解析，丢弃 20% 组
3. **最大重试**：`MAX_PTL_RETRIES = 3`
4. **标记**：`PTL_RETRY_MARKER = '[earlier conversation truncated for compaction retry]'`

### 3.6 Hook 系统

- `executePreCompactHooks`：compact 前注入自定义指令
- `executePostCompactHooks`：compact 后验证
- `processSessionStartHooks('compact')`：SM compact 专用

---

## 四、系统性差距对比

### 4.1 功能完整性对比

| 功能 | claude-code-best | lotus-app | 差距等级 |
|---|---|---|---|
| Microcompact（轻量清理）| ✅ `microCompact.ts` | ✅ `microcompact()` | **相当** |
| Tool result 预算 | ✅ `contentReplacementState` | ✅ `apply_tool_result_budget()` | **相当** |
| 重复/长结果折叠 | ❌（无等价概念） | ✅ `collapse_tool_results()` | lotus 特有 |
| **层 2：Session Memory Compact** | ✅ `sessionMemoryCompact.ts` | ❌ 无 | **P1 缺失** |
| **层 3：LLM 摘要 Compact** | ✅ `compact.ts` | ✅ `compact_messages_via_llm()` | **有但无生产实现** |
| **Post-Compact Reinjection** | ✅ `buildPostCompactMessages`（50K budget） | ❌ 无 | **P1 缺失** |
| **Compact Boundary 视图隔离** | ✅ `getMessagesAfterCompactBoundary()` | ❌ 仅 boundary 截断，无视图隔离 | **P1 缺失** |
| **Dynamic threshold** | ✅ 基于 model context window | ❌ 固定 chars 常量 | **P2 缺失** |
| **Token 精确估算** | ✅ `tokenEstimation.ts` | ❌ `chars/4` 粗估 | **P2 缺失** |
| **PTL 应急恢复** | ✅ `truncateHeadForPTLRetry` | ✅ PromptTooLongRecovery | **相当** |
| **熔断降级** | ✅ 3 次失败熔断 | ✅ 3 次失败熔断 | **相当** |
| **Hook 系统** | ✅ Pre/Post compact hooks | ❌ 无 | **P2 缺失** |
| **前端 UI 展示** | ✅ compact 事件通知 | ⚠️ Compacting spinner 已有，缺少 CompactCompleted 结果展示 | **P2 缺失** |
| **PreservedSegment 元数据** | ✅ headUuid/anchorUuid/tailUuid | ❌ 仅 tail_message_id | **P1 缺失** |
| **图片预清理** | ✅ `stripImagesFromMessages()` | ❌ 无 | **P2 缺失** |
| **可重建附件预清理** | ✅ `stripReinjectedAttachments()` | ❌ 无 | **P2 缺失** |

### 4.2 架构设计理念差异

| 维度 | claude-code-best | lotus-app | 影响 |
|---|---|---|---|
| **Context 管理哲学** | 一等公民：主动管理、资源预算 | 临时补丁：被动的收缩措施 | 哲学差距 Φ3 |
| **Token 视角** | token-based（精确 + provider-aware） | chars-based（粗估 chars/4） | 精度差 3-5x |
| **触发时机** | 动态窗口计算 | 固定 chars 阈值 | 不适应不同模型 |
| **状态隔离** | `getMessagesAfterCompactBoundary()` 视图隔离 | load_history 全量加载后再截断 | compact 后仍可能加载旧历史 |
| **状态持久化** | boundary 记录 + preservedSegment | boundary 记录 + tail_message_id | 回溯能力弱 |
| **后处理** | 系统性地重新注入关键上下文 | 无后处理 | compact 后模型失去文件/skill 记忆 |
| **可扩展性** | Hook 系统支撑 | 无扩展点 | 定制困难 |
| **错误恢复** | 多层降级：SM→API→PTL→Group truncation | 单一重试 | 容错能力弱 |
| **前端感知** | 事件通知 + UI 展示 | Compacting spinner 已有（`useStreaming.ts:789` / `StreamingBubble.tsx:100`），缺 CompactCompleted 结果展示 | 压缩进行中可感知，token 节省量未反馈 |

### 4.3 实现细节差异

#### 4.3.1 Microcompact 策略

| 细节 | claude-code-best | lotus-app |
|---|---|---|
| 清理触发 | 总是执行 | 仅超阈值执行 |
| 保留策略 | 按时间的微衰减 | 按最近 N 条 |
| 保留工具 | `COMPACTABLE_TOOLS` set | `preserved_tool_names` set |
| 占位符 | `[Old tool result content cleared]` | `[microcompacted]` |
| 图片处理 | >2000 tokens 也清除 | 无图片处理 |
| Caching | `CACHED_MICROCOMPACT` feature flag | 无 |

#### 4.3.2 Compact Boundary 格式

| 字段 | claude-code-best | lotus-app |
|---|---|---|
| type | `'compact_boundary'` | `'compact_boundary'`（subtype）|
| 触发 | `'auto'` / `'manual'` / `'micro'` | `Auto` / `Manual`（enum）|
| preTokenCount | ✅ | ✅（pre_tokens）|
| lastUserMessageUuid | ✅（`id` 引用） | ✅（tail_message_id）|
| **preservedSegment** | ✅ {headUuid, anchorUuid, tailUuid} | ❌ |
| preCompactDiscoveredTools | ✅ | ❌ |
| messageSummarized | ✅ | ✅ |

#### 4.3.3 持久化策略

| 方面 | claude-code-best | lotus-app |
|---|---|---|
| Boundary 存储 | JSONL（会话文件） | `compact_boundaries.jsonl` |
| 历史保留 | 全量 + boundary 视图 | 全量 + boundary 视图 |
| 读取路径 | `getMessagesAfterCompactBoundary()` | `history.rs::apply_boundary()` |
| compact 后加载 | 只加载 boundary 后 | 先全量加载再截断 |
| 重注入信息 | 写入独立字段 | 无 |

### 4.4 性能与可靠性差距

| 指标 | claude-code-best | lotus-app | 风险 |
|---|---|---|---|
| Compact 成功率 | 高（三层降级） | 低（未接通 CompactSummaryClient） | 需 R1.1 完成后可达 |
| Token 利用率 | 高（动态窗口 + 精确估算） | 低（固定阈值 + 粗估） | 过早/过晚 compact |
| 模型上下文利用 | 优（post-compact reinjection） | 一般（无 reinjection） | compact 后能力下降 |
| 数据一致性 | 好（视图隔离） | 较好（boundary 截断） | 极端场景下可能加载旧历史 |
| 可调试性 | 好（PreservedSegment + hooks） | 一般（仅日志） | 问题定位困难 |

---

## 五、关键差距与改进方向

### 5.1 P0 级差距（影响正确性）

#### GAP-1: `CompactSummaryClient` 无生产实现

**现状**：`compact_client.rs` 定义了 trait，但生产路径通过 `None` 分支空转，auto-compact 实际永不触发。

```rust
// chat_turn_driver.rs
None => {
    warn_no_compact_client();
    Ok(String::new())  // 跳过 auto-compact
}
```

**对标**：claude-code-best 的 `compact.ts` 在生产中正常调用 LLM 摘要。

**改进方向**：
- 实现 `OpenAiCompactSummaryClient`（或其他 provider 对应的 client）
- 通过 runtime builder 注入（`RuntimeLlmExecutor::() → with_compact_client()`）

#### GAP-2: 无 Session Memory Compact（零 API 成本路径）

**现状**：lotus-app 没有 Session Memory 概念，每次 compact 都依赖独立的 LLM 调用。

**对标**：claude-code-best 的 `sessionMemoryCompact.ts` 使用已提取的 Session Memory 作为摘要，零 API 调用的 compact 路径。

**影响**：compact 本身消耗 tokens，在紧凑的 context 中可能反而引发 PromptTooLong。

**改进方向**：
- 引入 Session Memory 系统（独立立项）
- 在 compact 流程中优先检查 Session Memory 可用性

### 5.2 P1 级差距（影响核心能力）

#### GAP-3: 无 Post-Compact Reinjection

**现状**：compact 后仅保留 `[boundary + summary + tail_round]`，模型失去了关于已读文件、已发现的 MCP 工具、已激活 Skill 等上下文信息。

**对标**：claude-code-best 有 50K token 预算用于重新注入：
- 最近读取的文件
- 激活的 Skill
- CLAUDE.md 内容
- MCP 工具发现结果

**影响**：compact 后模型需要重新了解上下文，多个 turn 内能力下降。

**改进方向**：
- 在 `compact_messages_via_llm()` 后增加 reinjection 阶段
- **本期范围仅 CLAUDE.md 重注入**：lotus-app 上下文与 claude-code-best 不同——Skill 已 stateless（`load_skill` 工具按需加载）、MCP 工具动态注册到 `ToolRegistry`、FileStateCache 未立项。直接对标 4 类重注入会错位
- 文件 / Skill / MCP 重注入留待 FileStateCache 子系统及 ContentBudgetManager 专项就位后追加（详见 replication-fix-plan §R2、§13）

#### GAP-4: 无 Compact Boundary 视图隔离

**现状**：`history.rs::apply_boundary()` 通过 `tail_message_id` 找到截断点，但存在两个问题：
1. `tail_message_id` 找不到时降级为全量加载
2. compact 后的 turn 没有专门的视图隔离机制

**对标**：claude-code-best 的 `getMessagesAfterCompactBoundary()` 是一个通用的、贯穿所有操作的前置过滤。

**改进方向**：
- 将 boundary 截断提升为 runtime 层的历史重建标准步骤
- 所有历史操作（加载、compact 检查、裁剪）都建立在 boundary 视图之上

#### GAP-5: 无 PreservedSegment 元数据

**现状**：`CompactBoundaryRecord` 只有 `tail_message_id`，无法精确追溯 compact 保留了哪些消息。

**对标**：claude-code-best 有 `preservedSegment = { headUuid, anchorUuid, tailUuid }`。

**影响**：无法验证 compact 是否正确保留了 tail round，调试困难。

**改进方向**：
- 在 `CompactBoundaryRecord` 中增加 `preserved_segment` 字段
- 包含 `first_preserved_message_id`、`anchor_message_id`、`last_preserved_message_id`

### 5.3 P2 级差距（优化项）

#### GAP-6: Chars/4 Token 估算

**现状**：所有 token 估算使用 `chars/4`，误差可达 3-5x（不同语言/内容类型差异大）。

**对标**：claude-code-best 有专门的 `tokenEstimation.ts`。

**改进方向**：
- 接入真实 tokenizer（如 tiktoken-rs）
- 或至少使用 provider 的 token 计数 API

#### GAP-7: 固定阈值 vs 动态窗口

**现状**：microcompact 阈值 120K chars（~30K tokens）、auto-compact 阈值 480K chars（~120K tokens），对任何模型都相同。

**对标**：claude-code-best 的阈值基于 `getContextWindowForModel(model)` 动态计算。

**改进方向**：
- 使用 `cloud_model`（网关返回的真实模型名）+ `context_window_for_model(cloud_model)` 匹配（已在 replication-fix-plan Task 0.1 落地，原 `primary_model` 错位修正）
- 使用 `effectiveContextWindow - BUFFER` 作为动态阈值

#### GAP-8: 无图片预清理

**现状**：lotus-app 的 compaction 管线不处理图片内容，大图片可能浪费大量 token。

**对标**：claude-code-best 在 compact 前 `stripImagesFromMessages()`。

**改进方向**：
- 在 `prepare_messages_for_llm` 中添加图片预清理步骤

#### GAP-9: 无 CompactCompleted 结果展示

**现状**：后端会发 `TurnStage::Compacting` 事件，前端 `useStreaming.ts:789` + `StreamingBubble.tsx:100` 已有 spinner 渲染。但 compact 完成后的 token 节省提示和 boundary 消息渲染尚未实现。

**对标**：claude-code-best 在 compact 完成后展示结果。

**改进方向**：
- 后端新增 `RuntimeEventKind::CompactCompleted` 事件
- 前端订阅并展示 token 节省量
- compact_boundary system 消息渲染为折叠提示条

### 5.4 哲学级差距 Φ3（需独立立项）

> 引自 `docs/archive/2026-05/2026-04-17-full-gap-assessment.md`

**claude-code-best 的哲学**：上下文窗口是有限资源，必须主动管理：
- `FileStateCache`：避免重复注入相同文件内容（去重）
- `contentReplacementState`：工具结果有全局预算，超限自动截断并告知模型
- `auto-compact`：context 接近上限时主动触发 summarization，对话可以无限持续
- `fileReadingLimits`：单次读取有 token 上限

**lotus-app 的现状**：四阶段预处理管线是良好的基础，但仍缺少：
- 统一的 Context Budget 机制
- token 计数器
- 文件内容去重（FileStateCache turn-level）
- 工具结果大小限制
- context 超限时的主动 compaction 策略（summarize vs truncate vs evict）

**改进方向**：
- 设计统一的 Context Budget 机制
- 将现有的四阶段管线纳入预算框架
- 引入 FileStateCache 避免文件重复读取
- 工具结果增加 `maxResultSizeChars` + `contentReplacementState`

---

## 六、实现状态追踪

### 6.1 Plan-K 完成状态

| Task | 状态 | 文件 | 备注 |
|---|---|---|---|
| K1: compact_boundary 类型 | ✅ 完成 | `compaction.rs`, `compact_boundaries.rs` | 结构体 + 存储 |
| K2: microcompact | ✅ 完成 | `compaction.rs` | 纯内存操作 |
| K3: LLM compact 核心 | ✅ 完成 | `compaction.rs`, `chat_turn_driver.rs` | 函数实现 + trait 定义 |
| K4: 主线集成 | ✅ 完成 | `chat_turn_driver.rs`, `turn_config.rs` | 主循环接入 + 熔断 |
| K5: review_ 测试 | ✅ 完成 | `review_autocompact_constraints_test.rs` | 7 条约束测试 |

### 6.2 Plan-AD 完成状态（Token 相关）

| Task | 状态 | 备注 |
|---|---|---|
| AD1: `estimate_tokens()` | ❌ 未实现 | 需加到 `context_decay.rs` |
| AD2: context window 观测 | ❌ 未实现 | 需加到 `chat_turn_driver.rs` |
| AD3: `LlmStepInput.estimated_tokens` | ❌ 未实现 | 需修改 `turn_config.rs` |
| AD4: `ThinkingConfig` | ❌ 未实现 | 需修改 `streaming.rs` |
| AD5: Claude thinking body | ❌ 未实现 | 需修改 `claude.rs` |

### 6.3 存储架构修复完成状态

| Phase | 状态 | 备注 |
|---|---|---|
| A: 存储单文件化 | ✅ 完成 | `messages.jsonl` + UUID dedup |
| B: history.rs 接管 | ✅ 完成 | boundary 截断 + tool pair 校验 + round 裁剪 |
| C: compact tail round | ✅ 完成 | 保留完整 tail round |
| D: token_budget + agentName | ✅ 完成 | 8192 default + agentName 透传 |

---

## 七、改进路线建议

### 7.1 优先级排序

```
P0（立即修，否则 compact 不工作）
  ├─ GAP-1: 实现 CompactSummaryClient 生产版本
  │   └─ 依赖：需要一个独立的 LLM 调用通道（不依赖 streaming executor）
  │
P1（当前版本体验短板）
  ├─ GAP-3: Post-Compact Reinjection
  │   └─ 依赖：FileStateCache + ToolDiscoveryCache + SkillRegistry 可查询
  ├─ GAP-4: Boundary 视图隔离强化
  │   └─ 依赖：history.rs 现有实现增强
  ├─ GAP-5: PreservedSegment 元数据
  │   └─ 依赖：CompactBoundaryRecord 扩展
  │
P2（优化项）
  ├─ GAP-6: 接入真实 tokenizer
  ├─ GAP-7: 动态阈值（已并入 Task 0.1）
  ├─ GAP-8: 图片预清理（已并入 R1.3）
  ├─ GAP-9: CompactCompleted 事件 + boundary 渲染（已并入 R5）
  │
独立立项
  ├─ GAP-2: Session Memory 系统（零 API 成本 compact）
  └─ Φ3: Context Budget 机制（统一框架）
```

### 7.2 关键文件清单

| 文件 | 当前角色 | 需要修改 |
|---|---|---|
| `src-tauri/src/runtime/chat/compaction.rs` | 核心 compact 函数 + 类型 | 扩展 PreservedSegment、Post-Compact Reinjection |
| `src-tauri/src/runtime/chat/preprocess.rs` | 四阶段编排 | 增加图片预清理、动态阈值 |
| `src-tauri/src/runtime/chat/compact_client.rs` | CompactSummaryClient trait | 无修改（trait 已定义）|
| `src-tauri/src/llm/` | 新增 CompactSummaryClient impl | 新建 `compact_summary_client.rs` |
| `src-tauri/src/runtime/chat/history.rs` | 历史重建 | 增强 boundary 视图隔离 |
| `src-tauri/src/llm/context_decay.rs` | 旧 decay 逻辑（仅 `storage/file_store/cognitive.rs:840` 调用，与 chat 管线无关） | AD1: 新增 `estimate_tokens()` / `resolve_context_window()`，保留 `apply_decay()` 不动 |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | 主循环 | AD2: token 风险观测 |
| `src-tauri/src/runtime/chat/turn_config.rs` | 配置类型 | AD3: `estimated_tokens` 字段 |

---

## 八、验证方案

### 8.1 现有测试

```bash
# Plan-K 专项测试（7 条测试）
cd src-tauri && cargo test --test review_autocompact_constraints_test -- --nocapture

# 历史重建测试（4 条测试）
cd src-tauri && cargo test --test history_rebuild_test -- --nocapture

# Microcompact 测试（3 条测试）
cd src-tauri && cargo test --test plan_k_microcompact_test -- --nocapture

# LLM Compact 测试（3 条测试）
cd src-tauri && cargo test --test plan_k_llm_compact_test -- --nocapture

# AutoCompact 跟踪测试（5 条测试）
cd src-tauri && cargo test --test plan_k_autocompact_tracking_test -- --nocapture

# 全量 review_ 回归
cd src-tauri && cargo test review_ --tests --no-fail-fast
```

### 8.2 验证口径

| 验证点 | 方法 | 通过条件 |
|---|---|---|
| Microcompact 触发 | 构造 >120K chars messages | content 被替换为 `[microcompacted]` |
| Auto-compact 触发 | 构造 >480K chars messages + mock CompactSummaryClient | `compact_messages_via_llm` 被调用 |
| 熔断器（per-turn） | 同 turn 内连续 3 次失败 | 第 4 次 `is_circuit_broken() == true`；下一 turn 通过 `TurnIterationState::new()` 自动重置 |
| Boundary 持久化 | 写入后读取 | 字段一致 |
| Tail round 保留 | compact 后验证 | tool_use/tool_result 配对完整 |
| Tool pair 完整性 | 历史重建后 | 无孤立 tool/assistant |
| Round-based 裁剪 | max_rounds=2, 5 rounds 输入 | 输出只剩 2 rounds |

---

## 九、总结

lotus-app 的上下文压缩系统已在 Plan-K 中建立了**基础的四阶段预处理管线架构**（budget → microcompact → collapse → auto-compact），并实现了熔断保护、boundary 持久化、Tail round 保留等关键机制。与 claude-code-best 的差距主要体现在以下维度：

1. **生产就绪度**：`CompactSummaryClient` 无生产实现是最关键的 P0 阻塞项
2. **架构深度**：缺少 Session Memory compact（零 API 成本路径）和 Post-Compact Reinjection（compact 后上下文恢复）
3. **Token 精度**：chars/4 粗估 vs provider-aware 精确估算
4. **可观测性**：缺少 PreservedSegment 元数据、CompactCompleted 结果事件（Compacting spinner 已有）
5. **可扩展性**：缺少 Hook 系统支撑自定义行为

最优先需解决的是 **GAP-1（CompactSummaryClient 生产实现）**——没有它，整个 auto-compact 管线在当前生产环境中实际处于空转状态。
