# 运行时恢复语义（Plan-W）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:test-driven-development — 每个 Task 必须先写测试再写实现，所有测试必须通过后再 commit。

**Goal:** 让 `run_chat_turn_s4` 按具体 stop reason / hook 结果 / orphaned permission 状态做恢复，而不是给 `TurnError` 加一个笼统的 recoverable 分类；本轮优先补齐 `max_output_tokens` 恢复、stop hook loop 内 continuation、`prompt_too_long -> compact` 兜底，以及 turn 结束时的 orphaned permission 收口。
**Architecture:** W1 只引入“具体恢复原因”的错误建模（尤其 `PromptTooLong`），不引入 blanket `is_recoverable()`；W2 基于 `ContentComplete.stop_reason` 做 `max_tokens` 恢复；W3 把 stop hook 放回主循环内部，并支持 `blocking_errors -> continue`；W4 在 turn 结束前检测并取消 orphaned permission。
**Tech Stack:** Rust, tokio
**Worktree branch:** pzc

---

## 对标修订（2026-04-19）

- 不采纳“所有 `LlmError` 都可恢复”的设计；恢复必须绑定到明确原因：
  - provider `stop_reason == max_tokens`
  - provider/gateway 明确判定为 `prompt_too_long`
  - stop hook 返回 `blocking_errors`
- `max_output_tokens` 恢复上限严格对齐 `claude-code-best/src/query.ts`：`MAX_OUTPUT_TOKENS_RECOVERY_LIMIT = 3`。
- stop hook 的关键不是“前移一点”，而是**放入 turn loop 内部**，并在 `blocking_errors` 非空时驱动新一轮 LLM 调用。
- lotus-app 目前没有 claude-code-best 那种 withheld assistant API-error message 管线；因此 `prompt_too_long` 的对齐方式是：
  - executor 先返回结构化 `TurnError::PromptTooLong`
  - driver 先尝试一次 reactive compact recovery
  - 恢复失败后再发射 `StreamError` 并终止
- `max_tokens` 在 lotus-app 中也不是 withheld API-error；恢复耗尽后保留已经生成的 partial content，并追加显式截断提示后结束 turn，避免丢失内容。

---

## 背景与现状差距

### 当前架构（lotus-app）

- `TurnError` 当前只有 `LlmError(String)` / `Cancelled` / `MaxRetriesExceeded` / `PersistenceError(String)`，缺少“明确可恢复原因”的错误建模；driver 只能一律 `return Err(...)`。
- `LlmStepResult::ContentComplete` 当前没有 `stop_reason`，driver 无法识别 `max_tokens` 截断。
- stop hook 目前发生在 Step 7 `persist_assistant_message` 之后，loop 已退出，hook 无法驱动新一轮调用。
- `HookOutcome` 当前没有 `blocking_errors`。
- `PendingPermissionRequestStore` 已有 `cancel_for_session`，但 driver turn 结束前没有 orphaned request 检测与事件。

### 对标（claude-code-best）

- `src/query.ts`：`MAX_OUTPUT_TOKENS_RECOVERY_LIMIT = 3`
- `src/query.ts`：stop hook 在主循环内部执行；`blockingErrors.length > 0` 时构造下一轮 state 并 `continue`
- `src/query.ts`：`prompt-too-long` 不做 blanket retry，而是走 collapse/reactive compact 的特定恢复路径

---

## Task W1 — 具体恢复原因建模（替代 blanket recoverable 分类）

### 目标

把 turn loop 的恢复入口收敛到明确原因，而不是 `TurnError::is_recoverable()`：

- 新增 `TurnError::PromptTooLong(String)`
- `TauriLegacyTurnExecutor` 在识别到 prompt/context overflow 时返回该 variant
- driver 只对这个 variant 触发 reactive compact recovery；其余错误仍按 fatal 处理

### 文件范围

- `src-tauri/src/runtime/chat/turn_config.rs`
- `src-tauri/src/transport/tauri_commands/chat.rs`
- `src-tauri/src/runtime/chat/chat_turn_driver.rs`

### 详细实现

**turn_config.rs**

- `TurnError` 新增：

```rust
PromptTooLong(String)
```

- 不新增 `is_recoverable()` 一类 blanket API。

**transport/tauri_commands/chat.rs**

- 把当前 `classify_llm_error_str()` 升级为结构化分类：
  - `prompt/context length / 413 + token/context 关键词` -> `TurnError::PromptTooLong(...)`
  - 其它仍归类为 `TurnError::LlmError(...)`
- 对 `PromptTooLong` 不提前发 `StreamError`，把恢复决策交给 driver。

**chat_turn_driver.rs**

- `run_llm_step()` 返回 `PromptTooLong` 时：
  - 若本 turn 尚未尝试 reactive compact，则调用现有 `compact_summary + compact_messages_via_llm` 做一次恢复，并 `continue 'turn`
  - 若已尝试过或 compact 无法生成摘要，则发 `RuntimeEventKind::StreamError` 并终止
- 不对普通 `LlmError` 做诊断消息注入重试。

### 测试

- `plan_w_runtime_recovery_test.rs`
  - `prompt_too_long` 首次命中时 driver 会先 compact 后继续
  - reactive compact 已尝试过时，`PromptTooLong` 会被直接 surface 为 `StreamError + Err`

---

## Task W2 — `max_output_tokens` 恢复循环

### 目标

检测 `ContentComplete.stop_reason == Some("max_tokens")`，注入 resume meta message 并继续 turn，最多 3 次；超过上限后保留 partial content，并追加显式截断提示后结束。

### 文件范围

- `src-tauri/src/runtime/chat/turn_config.rs`
- `src-tauri/src/transport/tauri_commands/chat.rs`
- `src-tauri/src/runtime/chat/chat_turn_driver.rs`

### 详细实现

**turn_config.rs**

- `LlmStepResult::ContentComplete` 新增：

```rust
stop_reason: Option<String>
```

- 新增常量：

```rust
pub const MAX_OUTPUT_TOKENS_RECOVERY_LIMIT: usize = 3;
```

- `TurnIterationState` 新增：

```rust
pub max_output_tokens_recovery_count: usize,
```

**transport/tauri_commands/chat.rs**

- 生产 executor 把 provider `StopReason` 映射为字符串：
  - `EndTurn -> "end_turn"`
  - `ToolUse -> "tool_use"`
  - `MaxTokens -> "max_tokens"`
  - `StopSequence -> "stop_sequence"`

**chat_turn_driver.rs**

- `ContentComplete` 分支先检查 `stop_reason == Some("max_tokens")`
- 若未达上限：
  - 把 partial assistant content 追加到 `state.messages`
  - 注入一条 meta user message：
    - `Output token limit hit. Resume directly ...`
  - `state.max_output_tokens_recovery_count += 1`
  - `continue 'turn`
- 若已达上限：
  - 不再继续循环
  - 在 `state.full_content` 末尾追加显式截断提示
  - 正常进入后续 persist / terminal events

### 测试

- `plan_w_runtime_recovery_test.rs`
  - recovery limit 恒为 3
  - `max_tokens -> resume -> normal completion` 路径可闭环
  - 超过上限不会无限循环，并会保留 partial content + 截断提示

---

## Task W3 — stop hook 回到 loop 内，并支持 `blocking_errors`

### 目标

stop hook 在 `ContentComplete + 非 max_tokens` 的同一轮内执行：

- `prevent_continuation=true` -> 终止 turn
- `blocking_errors` 非空 -> 把这些消息 append 到 transcript，驱动下一轮 LLM 调用
- 用 `stop_hook_active` 防止死循环

### 文件范围

- `src-tauri/src/runtime/hooks/runner.rs`
- `src-tauri/src/runtime/chat/turn_config.rs`
- `src-tauri/src/runtime/chat/chat_turn_driver.rs`

### 详细实现

**runner.rs**

- `HookOutcome` / `HookOutput` 新增：

```rust
blocking_errors: Vec<String>
```

- `run_hooks()` 聚合所有 hook 返回的 `blocking_errors`

**turn_config.rs**

- `TurnIterationState` 新增：

```rust
pub stop_hook_active: bool,
```

**chat_turn_driver.rs**

- 在 `ContentComplete` 的正常结束路径里执行 stop hook
- 仅当 `!state.stop_hook_active` 时运行 hook
- 若 `blocking_errors` 非空：
  - 逐条 append 为 meta user message
  - `state.stop_hook_active = true`
  - `state.max_output_tokens_recovery_count = 0`
  - `continue 'turn`
- 删除 Step 7 之后旧的 stop hook 执行逻辑

### 测试

- `plan_w_runtime_recovery_test.rs`
  - stop hook 的 `blocking_errors` 会驱动第二轮 LLM 调用
  - `stop_hook_active` 会阻止重复进入无限 loop
- `plan_m_hook_runner_test.rs`
  - `blockingErrors` JSON 能被正确解析

---

## Task W4 — orphaned permission 检测与自动取消

### 目标

turn 结束前检查当前 session 是否还留有未解决的 pending permission request；若有，则自动 cancel 并发可观测事件。

### 文件范围

- `src-tauri/src/runtime/store/pending_permission_request_store.rs`
- `src-tauri/src/runtime/events.rs`
- `src-tauri/src/runtime/chat/turn_config.rs`
- `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- `src-tauri/tests/common.rs`

### 详细实现

**pending_permission_request_store.rs**

- `PendingPermissionControlPlane` trait 新增：

```rust
fn cancel_for_session(&self, session_id: &SessionId, message: &str) -> usize;
fn pending_count_for_session(&self, session_id: &SessionId) -> usize;
```

- `PendingPermissionRequestStore` 实现这两个方法

**events.rs**

- 新增：

```rust
RuntimeEventKind::OrphanedPermissionDetected { count: usize }
```

**turn_config.rs**

- `TurnIterationState` 新增：

```rust
pub orphaned_permission_count: usize,
```

**chat_turn_driver.rs**

- 在 terminal events 之前：
  - 读取当前 session 的 pending count
  - count > 0 时执行 `cancel_for_session(...)`
  - 发 `OrphanedPermissionDetected { count }`

### 测试

- `plan_w_runtime_recovery_test.rs`
  - 存在 orphaned permission 时会被自动 cancel
  - 会发 `OrphanedPermissionDetected`
  - 无 orphaned request 时不发事件

---

## 实施顺序

```text
W1 -> W2 -> W3 -> W4
```

- W1 先把“恢复原因建模”纠正为具体原因驱动
- W2 / W3 是和 `claude-code-best` 最直接对齐的 turn-loop 语义
- W4 最独立，放最后收口

---

## 测试命令

```bash
cd src-tauri
cargo test plan_w_runtime_recovery_test -- --nocapture
cargo test plan_m_hook_runner_test -- --nocapture
cargo test review_ --tests --no-fail-fast
```

---

## 关键文件索引

| 文件 | 变更范围 |
|---|---|
| `src-tauri/src/runtime/chat/turn_config.rs` | `TurnError::PromptTooLong`；`ContentComplete.stop_reason`；`MAX_OUTPUT_TOKENS_RECOVERY_LIMIT`；`TurnIterationState` 新字段 |
| `src-tauri/src/transport/tauri_commands/chat.rs` | prompt-too-long 识别；provider stop_reason 映射 |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | prompt-too-long reactive compact；max_tokens recovery；loop 内 stop hook；orphaned permission 检测 |
| `src-tauri/src/runtime/hooks/runner.rs` | `HookOutcome.blocking_errors` / `HookOutput.blockingErrors` |
| `src-tauri/src/runtime/store/pending_permission_request_store.rs` | `pending_count_for_session` / trait 扩展 |
| `src-tauri/src/runtime/events.rs` | `OrphanedPermissionDetected` |
| `src-tauri/tests/plan_w_runtime_recovery_test.rs` | Plan-W 回归测试 |
