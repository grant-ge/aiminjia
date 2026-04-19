# 运行时恢复语义（Plan-W）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:test-driven-development — 每个 Task 必须先写测试再写实现，所有测试必须通过后再 commit。

**Goal:** 让 turn loop 按具体 transition / stop reason / withheld provider error 做恢复，而不是对 `TurnError` 做笼统 recoverable 分类；优先补齐 `max_output_tokens`、stop-hook blocking continuation、prompt-too-long/compact 协同与 orphaned permission 收口。
**Architecture:** W1 以具体恢复原因驱动 loop transition；W2 在 driver 主循环检测 `max_output_tokens` 并注入恢复消息；W3 把 stop_hook 执行点前移到 loop 内并支持 blocking message + continue；W4 在 SessionRuntime 跟踪 orphaned permission。
**Tech Stack:** Rust, tokio
**Worktree branch:** pzc

---

## 对标修订（2026-04-19）

- 删除“所有 `LlmError` 都是 recoverable”的思路；恢复逻辑必须绑定具体 provider stop reason / withheld error / transition reason。
- `max_output_tokens` 恢复次数仍对齐 `claude-code-best` 的 3 次上限。
- `stop_hook` 的关键不是简单前移，而是放进主循环内部，在存在 blocking message 时允许 `continue`。
- `prompt_too_long -> compact/reactive recovery` 属于本计划范围，避免把 overflow 全压给 `Plan-AD`。

---

## 背景与现状差距

### 当前架构（lotus-app）

- `TurnError` 有 4 个 variant：`LlmError(String)` / `Cancelled` / `MaxRetriesExceeded` / `PersistenceError(String)`，但无任何 `is_recoverable()` 方法，全部通过 `?` 向上传播，调用方无法区分重试与终止。
- `LlmStepResult` 有 3 个 variant：`ToolCalls` / `ContentComplete` / `Cancelled`，无 `MaxTokens` variant，驱动层无法检测到 LLM 因 output token 截断而停止的情况。
- stop_hook 执行位于 Step 7（persist_assistant_message）**之后**（`run_chat_turn_s4` L917-L930），此时 turn loop 已退出，hook 返回的任何指令都无法驱动新一轮 LLM 调用；`HookOutcome` 当前只有 `prevent_continuation` 和 `stop_reason`，没有 `blocking_errors`（表示"继续但带有错误消息"的语义）。
- `PendingPermissionRequestStore` 在 `SessionRuntime` 层是单次 turn 的生命周期；跨 turn 的 orphaned permission（turn 结束后仍悬挂的 AskRequired）没有任何追踪字段，也没有清理协议。

### 对标（claude-code-best/src/query.ts）

- `MAX_OUTPUT_TOKENS_RECOVERY_LIMIT = 3`（L164）：检测到截断后最多注入 3 次恢复消息继续循环。
- stop_hook 在主循环**内部**执行（L1270-L1308）：`blockingErrors.length > 0` 时 `continue` 驱动新 LLM 轮；`preventContinuation` 时直接 return。
- orphaned messages 在 streaming fallback 时通过 tombstone 机制清理（L713-L740）。

---

## Task W1 — 基于具体恢复原因的主循环重试（替代 blanket recoverable 分类）

### 目标

不再为 `TurnError` 增加泛化 `is_recoverable()`；改为在 `run_chat_turn_s4` 中根据具体 provider stop reason、withheld API error 与 transition reason 生成有限状态恢复分支，只有命中明确恢复条件时才注入诊断消息并 `continue`。

### 文件范围

- `src-tauri/src/runtime/chat/turn_config.rs` — 扩展 `TurnError`
- `src-tauri/src/runtime/chat/chat_turn_driver.rs` — 修改 step_result 错误处理

### 详细实现

**turn_config.rs**

在 `TurnError` 中新增：

```rust
impl TurnError {
    /// 返回 true 表示 driver 可以向 LLM 注入诊断消息后继续循环；
    /// 返回 false 表示 fatal，driver 必须立即终止 turn。
    pub fn is_recoverable(&self) -> bool {
        match self {
            // LLM 临时错误（超时/5xx）：可恢复
            TurnError::LlmError(_) => true,
            // 用户主动取消：不可恢复
            TurnError::Cancelled => false,
            // 重试上限已耗尽：不可恢复
            TurnError::MaxRetriesExceeded => false,
            // 持久化失败：不可恢复（数据一致性风险）
            TurnError::PersistenceError(_) => false,
        }
    }

    /// 生成注入到消息列表的诊断内容（用于 recoverable 错误）。
    pub fn recovery_message(&self) -> String {
        match self {
            TurnError::LlmError(msg) => {
                format!(
                    "The previous LLM call encountered a temporary error: {}. \
                     Continuing from where you left off.",
                    msg
                )
            }
            _ => "A temporary error occurred. Continuing.".to_string(),
        }
    }
}
```

**chat_turn_driver.rs — Step 5b 错误处理**

当前代码（L736-L739）对 `run_llm_step` 的错误直接 `map_err(|e| anyhow::anyhow!("{}", e))?`，无法区分 recoverable/fatal。

修改为：

```rust
// ── Step 5b: single LLM step ─────────────────────────────────────
let step_result = match executor
    .run_llm_step(&input, &self.event_bus, &cancel)
    .await
{
    Ok(result) => result,
    Err(err) if err.is_recoverable() => {
        // Recoverable error: inject a diagnostic user message and
        // continue the loop so the LLM can acknowledge and resume.
        log::warn!(
            "[run_chat_turn_s4] recoverable LLM error on iteration {}: {}",
            iteration,
            err
        );
        let diagnostic_msg = serde_json::json!({
            "role": "user",
            "content": err.recovery_message(),
        });
        state.messages.push(diagnostic_msg);
        continue 'turn;
    }
    Err(err) => {
        return Err(anyhow::anyhow!("{}", err));
    }
};
```

`TurnIterationState` 中新增字段以防无限恢复循环：

```rust
pub struct TurnIterationState {
    // ... existing fields ...
    /// 已消耗的 recoverable-error 重试次数（上限 = MAX_RECOVERABLE_RETRIES）
    pub recoverable_error_count: usize,
}
```

常量定义（driver 顶部）：

```rust
/// recoverable LLM 错误最多允许注入诊断消息重试的次数。
/// 超过此值视为 fatal，直接终止 turn。
const MAX_RECOVERABLE_RETRIES: usize = 2;
```

recoverable 分支需先检查计数器：

```rust
Err(err) if err.is_recoverable()
    && state.recoverable_error_count < MAX_RECOVERABLE_RETRIES =>
{
    state.recoverable_error_count += 1;
    // inject diagnostic and continue ...
}
Err(err) => {
    return Err(anyhow::anyhow!("{}", err));
}
```

### 测试

文件：`src-tauri/tests/review_w1_recoverable_turn_error_test.rs`

```rust
//! W1 回归测试：TurnError.is_recoverable() 分类正确性
//! + driver 对 recoverable 错误注入诊断消息并继续循环，
//!   对 fatal 错误立即终止。

use lotus_app::runtime::chat::turn_config::TurnError;

// ── 单元测试：is_recoverable() 分类 ────────────────────────────────

#[test]
fn w1_llm_error_is_recoverable() {
    let err = TurnError::LlmError("connection reset".to_string());
    assert!(
        err.is_recoverable(),
        "LlmError must be recoverable so driver can inject diagnostic and retry"
    );
}

#[test]
fn w1_cancelled_is_not_recoverable() {
    assert!(
        !TurnError::Cancelled.is_recoverable(),
        "Cancelled must not be recoverable — user intent is final"
    );
}

#[test]
fn w1_max_retries_exceeded_is_not_recoverable() {
    assert!(
        !TurnError::MaxRetriesExceeded.is_recoverable(),
        "MaxRetriesExceeded must not be recoverable — retry budget exhausted"
    );
}

#[test]
fn w1_persistence_error_is_not_recoverable() {
    let err = TurnError::PersistenceError("disk full".to_string());
    assert!(
        !err.is_recoverable(),
        "PersistenceError must not be recoverable — data consistency risk"
    );
}

#[test]
fn w1_recovery_message_contains_original_error() {
    let err = TurnError::LlmError("timeout after 90s".to_string());
    let msg = err.recovery_message();
    assert!(
        msg.contains("timeout after 90s"),
        "recovery_message must include the original error text for diagnostics"
    );
}

// ── 集成测试：driver 对 recoverable 错误 continue ──────────────────
// 使用 MockFailingExecutor：第 1 次 run_llm_step 返回 LlmError，
// 第 2 次返回 ContentComplete。
// 期望：driver 正常完成，state.messages 包含诊断消息。

#[tokio::test]
async fn w1_driver_injects_diagnostic_on_recoverable_error_and_completes() {
    // 实现略（mock executor + driver fixture，见下方说明）
    // 核心断言：
    // 1. driver.run_chat_turn_s4() 返回 Ok(())
    // 2. 最终 messages 包含一条 role=user 的诊断消息
    // 3. full_content 非空（第 2 次 LLM 调用成功）
}

#[tokio::test]
async fn w1_driver_terminates_on_fatal_error() {
    // MockFatalExecutor：第 1 次返回 PersistenceError
    // 期望：driver.run_chat_turn_s4() 返回 Err
}

#[tokio::test]
async fn w1_driver_terminates_after_max_recoverable_retries() {
    // MockAlwaysFailingExecutor：每次都返回 LlmError
    // 期望：driver 在 MAX_RECOVERABLE_RETRIES 次后终止，返回 Err
}
```

### Commit message

```
feat(turn-driver): classify TurnError as recoverable/fatal and inject diagnostic on retry - W1
```

---

## Task W2 — max_output_tokens 恢复循环

### 目标

检测 LLM 因输出 token 上限截断停止的情况，注入 "Output token limit hit. Resume directly..." 恢复消息，最多重试 3 次（`MAX_OUTPUT_TOKENS_RECOVERY_LIMIT = 3`）。

### 文件范围

- `src-tauri/src/runtime/chat/turn_config.rs` — 为 `LlmStepResult::ContentComplete` 增加 `stop_reason` 字段；新增常量
- `src-tauri/src/runtime/chat/chat_turn_driver.rs` — 检测截断并注入恢复消息

### 详细实现

**turn_config.rs**

为 `LlmStepResult::ContentComplete` 增加 `stop_reason` 字段：

```rust
pub enum LlmStepResult {
    ToolCalls { ... },  // 不变
    ContentComplete {
        content: String,
        tokens_in: u64,
        tokens_out: u64,
        /// LLM provider 返回的原始停止原因。
        /// Some("max_tokens") 表示因 output token 上限被截断；
        /// None / Some("end_turn") 表示正常完成。
        stop_reason: Option<String>,
    },
    Cancelled,
}
```

新增常量：

```rust
/// max_output_tokens 触发后允许注入恢复消息继续的最大次数。
/// 对齐 claude-code-best MAX_OUTPUT_TOKENS_RECOVERY_LIMIT。
pub const MAX_OUTPUT_TOKENS_RECOVERY_LIMIT: usize = 3;
```

**TurnIterationState** 新增计数器：

```rust
pub struct TurnIterationState {
    // ... existing fields ...
    /// 已消耗的 max_output_tokens 恢复次数（上限 MAX_OUTPUT_TOKENS_RECOVERY_LIMIT）
    pub max_tokens_recovery_count: usize,
}
```

**chat_turn_driver.rs — Step 5c 处理**

当前 `ContentComplete` 分支（L743-L753）直接 `break 'turn`。修改为先检查截断：

```rust
LlmStepResult::ContentComplete {
    content,
    tokens_in,
    tokens_out,
    stop_reason,
} => {
    state.full_content.push_str(&content);
    state.step_tokens_in += tokens_in;
    state.step_tokens_out += tokens_out;
    state.iteration_count = iteration + 1;

    // ── W2: max_output_tokens 恢复循环 ─────────────────────────
    // 检测 LLM 因输出 token 上限截断（provider 返回 stop_reason = "max_tokens"）。
    // 注入恢复消息最多 MAX_OUTPUT_TOKENS_RECOVERY_LIMIT 次后才终止。
    // 对齐 claude-code-best L1226-L1254。
    if stop_reason.as_deref() == Some("max_tokens")
        && state.max_tokens_recovery_count < MAX_OUTPUT_TOKENS_RECOVERY_LIMIT
    {
        state.max_tokens_recovery_count += 1;
        log::info!(
            "[run_chat_turn_s4] max_output_tokens hit on iteration {}, \
             recovery attempt {}/{}",
            iteration,
            state.max_tokens_recovery_count,
            MAX_OUTPUT_TOKENS_RECOVERY_LIMIT,
        );
        // Append the assistant's partial response as history.
        state.messages.push(serde_json::json!({
            "role": "assistant",
            "content": content,  // partial content already appended to full_content above
        }));
        // Inject the recovery user message.
        state.messages.push(serde_json::json!({
            "role": "user",
            "content": "Output token limit hit. Resume directly — no apology, \
                        no recap of what you were doing. \
                        Pick up mid-thought if that is where the cut happened. \
                        Break remaining work into smaller pieces.",
        }));
        continue 'turn;
    }

    // Normal completion or recovery exhausted — break the loop.
    turn_completed_normally = true;
    break 'turn;
}
```

**executor 对接**：生产 executor（`TauriLegacyTurnExecutor::run_llm_step`）需要在 `ContentComplete` 中填充 `stop_reason`，从 LLM provider streaming response 的 `stop_reason` 字段映射。这是 executor 内部变更，Plan-W 只定义 driver 侧协议。

### 测试

文件：`src-tauri/tests/review_w2_max_tokens_recovery_test.rs`

```rust
//! W2 回归测试：max_output_tokens 恢复循环
//! - 检测 stop_reason = "max_tokens" 时注入恢复消息并继续循环
//! - 恢复次数达到 MAX_OUTPUT_TOKENS_RECOVERY_LIMIT 后正常终止

use lotus_app::runtime::chat::turn_config::MAX_OUTPUT_TOKENS_RECOVERY_LIMIT;

#[test]
fn w2_max_tokens_recovery_limit_is_three() {
    assert_eq!(
        MAX_OUTPUT_TOKENS_RECOVERY_LIMIT, 3,
        "recovery limit must match claude-code-best MAX_OUTPUT_TOKENS_RECOVERY_LIMIT"
    );
}

// ── 集成测试：正常截断 → 恢复 → 完成 ────────────────────────────────
// MockTruncatingExecutor：
//   - 第 1 次 run_llm_step → ContentComplete { stop_reason: Some("max_tokens"), content: "part1" }
//   - 第 2 次 run_llm_step → ContentComplete { stop_reason: None, content: "part2" }
// 期望：
//   - driver 返回 Ok(())
//   - full_content == "part1part2"
//   - state.max_tokens_recovery_count == 1
//   - messages 中包含恢复用的 user message

#[tokio::test]
async fn w2_recovery_message_injected_on_max_tokens_and_completes_on_second_attempt() {
    // ...
}

// ── 恢复次数耗尽 ────────────────────────────────────────────────────
// MockAlwaysTruncatingExecutor：每次都返回 max_tokens
// 期望：driver 在第 MAX_OUTPUT_TOKENS_RECOVERY_LIMIT 次截断后 break（turn_completed_normally=false）
// 而不是无限循环

#[tokio::test]
async fn w2_recovery_stops_after_limit_reached() {
    // ...
    // 断言：driver 返回 Ok(())（不 panic），
    //       final_outcome 为 MaxIterationsReached 或自定义 MaxTokensRecoveryExhausted
}

// ── end_turn 不触发恢复 ────────────────────────────────────────────
// MockNormalExecutor：stop_reason = Some("end_turn")
// 期望：driver 正常完成，不注入恢复消息，recovery_count == 0

#[tokio::test]
async fn w2_no_recovery_on_end_turn_stop_reason() {
    // ...
}
```

### Commit message

```
feat(turn-driver): add max_output_tokens recovery loop up to 3 retries - W2
```

---

## Task W3 — stop_hook 前移至 loop continue 判断之前

### 目标

将 stop_hook 执行点从"persist_assistant_message 之后（Step 7 后）"前移到"loop 退出前"，使 hook 返回的 `blocking_errors` 能驱动新一轮 LLM 调用；`prevent_continuation` 仍然终止 turn。

### 背景差距

当前 `run_chat_turn_s4` 的执行顺序（L917-L930）：

```
persist_assistant_message (Step 7)
  → run_hooks (stop_hook)
    → state.stop_hook_prevent_continuation = outcome.prevent_continuation
→ emit terminal events (Step 8)
```

此时 `'turn` loop 已经退出（L874 `}`），hook 返回的任何内容都只能影响事件，无法驱动新 LLM 轮。

claude-code-best 的顺序（query.ts L1270-L1308）：

```
ContentComplete → (仍在循环内)
  → handleStopHooks()
    → if preventContinuation: return
    → if blockingErrors: append errors + continue (新 LLM 轮)
  → 正常终止
```

### 文件范围

- `src-tauri/src/runtime/hooks/runner.rs` — `HookOutcome` 新增 `blocking_errors` 字段
- `src-tauri/src/runtime/chat/turn_config.rs` — `TurnIterationState` 新增 `stop_hook_active` 字段
- `src-tauri/src/runtime/chat/chat_turn_driver.rs` — 重构 stop_hook 执行点

### 详细实现

**runner.rs — `HookOutcome` 扩展**

```rust
#[derive(Debug, Clone)]
pub struct HookOutcome {
    pub decision: HookDecision,
    pub updated_input: Option<Value>,
    pub prevent_continuation: bool,
    pub stop_reason: Option<String>,
    /// stop hook 返回的阻塞错误消息列表。
    /// 非空时 driver 将这些消息 append 到 transcript 并 continue 循环。
    /// 对齐 claude-code-best stopHookResult.blockingErrors。
    pub blocking_errors: Vec<String>,
}
```

`HookOutput`（反序列化结构）同步新增：

```rust
#[derive(Debug, Deserialize)]
struct HookOutput {
    // ... existing fields ...
    #[serde(rename = "blockingErrors", default)]
    blocking_errors: Vec<String>,
}
```

`run_hook` 返回时映射 `blocking_errors`。

**turn_config.rs — `TurnIterationState`**

```rust
pub struct TurnIterationState {
    // ... existing fields ...
    /// stop_hook 已执行且返回 blocking_errors，进入下一轮时标记。
    /// 防止重复执行：若 blocking_errors 驱动的新轮再次触发 stop_hook，
    /// 需要避免死循环（对齐 claude-code-best stopHookActive 标志）。
    pub stop_hook_active: bool,
}
```

**chat_turn_driver.rs — 重构执行点**

当前 Step 5c 的 `ContentComplete` 处 `break 'turn` 之前（W2 之后），在 break 前插入 stop_hook 逻辑：

```rust
// （此处已完成 W2 的 max_tokens 检测，turn_completed_normally = true）

// ── W3: stop_hook 在 loop 内执行 ───────────────────────────────────
// stop_hook 在 turn 正常完成（ContentComplete + non-max_tokens）时执行，
// 且仅在没有被 stop_hook_active 保护（防死循环）时才运行。
// 对齐 claude-code-best L1270-L1308。
if let Some(registry) = config.hook_registry.as_ref() {
    if !state.stop_hook_active {
        let stop_hooks = registry.hooks_for(HookEvent::Stop, "__stop__");
        if !stop_hooks.is_empty() {
            let stop_input = serde_json::json!({
                "stop_reason": "content_complete",
                "content": &state.full_content,
            });
            let runner = HookRunner::new();
            if let Ok(outcome) = runner.run_hooks(&stop_hooks, "__stop__", &stop_input).await {
                if outcome.prevent_continuation {
                    // Hook wants to terminate: record and break normally.
                    state.stop_hook_prevent_continuation = true;
                    state.stop_hook_reason = outcome.stop_reason;
                    turn_completed_normally = true;
                    break 'turn;
                }
                if !outcome.blocking_errors.is_empty() {
                    // Hook returned errors: append as user messages and drive
                    // a new LLM turn (continue loop with stop_hook_active guard).
                    state.stop_hook_active = true;
                    for error_msg in &outcome.blocking_errors {
                        state.messages.push(serde_json::json!({
                            "role": "user",
                            "content": error_msg,
                        }));
                    }
                    // Reset max_tokens recovery count for the new sub-turn
                    // (blocking errors may arrive after a recovered truncation).
                    state.max_tokens_recovery_count = 0;
                    turn_completed_normally = false;
                    continue 'turn;
                }
            }
        }
    }
}

turn_completed_normally = true;
break 'turn;
```

**Step 7 之后的 stop_hook 代码（L917-L930）删除**，因为执行点已前移。terminal event 中已有 `stop_hook_prevent_continuation` 的使用（L963），保持不变。

### 测试

文件：`src-tauri/tests/review_w3_stop_hook_drives_new_llm_turn_test.rs`

```rust
//! W3 回归测试：stop_hook blocking_errors 驱动新 LLM 轮
//! + prevent_continuation 正确终止
//! + stop_hook_active 防止死循环

// ── blocking_errors 触发新 LLM 轮 ──────────────────────────────────
// 场景：
//   - MockExecutor：第 1 次 ContentComplete(stop_reason=None)
//                   第 2 次 ContentComplete(stop_reason=None)
//   - StopHookScript：第 1 次调用返回 blocking_errors=["Fix: missing summary"]
//                     第 2 次（stop_hook_active=true）不执行
// 期望：
//   - driver 执行了 2 次 LLM 调用
//   - messages 中包含 "Fix: missing summary" 的 user message
//   - 最终 driver 返回 Ok(())

#[tokio::test]
async fn w3_stop_hook_blocking_errors_drive_new_llm_turn() {
    // ...
}

// ── prevent_continuation 终止 ──────────────────────────────────────
// StopHookScript：返回 prevent_continuation=true
// 期望：
//   - driver 只执行 1 次 LLM 调用
//   - state.stop_hook_prevent_continuation == true

#[tokio::test]
async fn w3_stop_hook_prevent_continuation_terminates_turn() {
    // ...
}

// ── stop_hook_active 防止死循环 ─────────────────────────────────────
// 场景：StopHookScript 每次都返回 blocking_errors
// 期望：stop_hook 只执行 1 次（第 2 次因 stop_hook_active=true 跳过）

#[tokio::test]
async fn w3_stop_hook_active_prevents_infinite_blocking_loop() {
    // ...
}

// ── stop_hook 不在 persist 之后执行（原位置已移除）──────────────────
// 通过 grep/静态检查确认 L917-L930 原代码已删除
// 可在 review_ 系列中加断言：确认 hook_registry 只在 'turn loop 内被访问
```

### Commit message

```
refactor(turn-driver): move stop_hook execution into turn loop; add blocking_errors continuation - W3
```

---

## Task W4 — orphaned permission 跨 turn 追踪

### 目标

在 `QueryEngine`（或 `SessionRuntime`）增加 `has_orphaned_permission` 标志，在 turn 结束时检查是否存在未解决的 pending permission，若有则记录并在下次 turn 开始时清理或向 LLM 报告。

### 背景差距

当前 `PendingPermissionRequestStore` 是 `SessionRuntime` 层管理的，`cancel_session` 时会清理，但：

1. 正常 turn 结束时（非 cancel），驱动层不检查是否有悬挂的 permission request。
2. 若某次 turn 中 permission ask 发出后、用户还没有响应，此时另一个 turn 开始（理论上不应发生，但 race 或测试中可能），orphaned request 不会被发现。
3. 没有任何指标/事件来观测 orphaned permission 的存在。

### 文件范围

- `src-tauri/src/runtime/store/pending_permission.rs`（或 `session_runtime.rs`）— 新增查询接口
- `src-tauri/src/runtime/chat/chat_turn_driver.rs` — turn 结束时检查
- `src-tauri/src/runtime/events.rs` — 新增 `OrphanedPermissionDetected` 事件（可选，用于可观测性）

### 详细实现

**PendingPermissionRequestStore — 新增查询方法**

```rust
impl PendingPermissionRequestStore {
    /// 返回当前 session 下所有仍处于 pending 状态的 permission request 数量。
    /// turn 结束时调用，用于检测 orphaned requests。
    pub fn pending_count_for_session(&self, session_id: &SessionId) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .filter(|entry| entry.request.session_id == *session_id)
            .count()
    }
}
```

**TurnIterationState — 新增标志**

```rust
pub struct TurnIterationState {
    // ... existing fields ...
    /// turn 结束时检测到的 orphaned permission request 数量（通常为 0）。
    /// 非 0 时 driver 发射 OrphanedPermissionDetected 事件供监控。
    pub orphaned_permission_count: usize,
}
```

**chat_turn_driver.rs — Step 8（emit terminal events）之前新增检查**

```rust
// ── W4: orphaned permission check ─────────────────────────────────
// A permission ask that was never resolved (orphaned) can cause the
// next turn to deadlock. Detect and cancel any orphaned requests,
// then surface an observable event for monitoring.
if let Some(control_plane) = self.pending_permission_control_plane.as_ref() {
    let orphaned_count = control_plane.pending_count_for_session(&session_id);
    if orphaned_count > 0 {
        state.orphaned_permission_count = orphaned_count;
        log::warn!(
            "[run_chat_turn_s4] detected {} orphaned permission request(s) \
             at end of turn {} — cancelling",
            orphaned_count,
            run_id.as_str()
        );
        // Cancel all orphaned requests so the next turn starts clean.
        control_plane.cancel_for_session(
            &session_id,
            "Permission request was not resolved before the turn ended (orphaned).",
        );
        // Emit observable event.
        self.event_bus
            .emit(RuntimeEvent::new(
                session_id.clone(),
                run_id.clone(),
                RuntimeEventKind::OrphanedPermissionDetected {
                    count: orphaned_count,
                },
            ))
            .await?;
    }
}
```

**RuntimeEventKind — 新增 variant**（`src-tauri/src/runtime/events.rs`）：

```rust
pub enum RuntimeEventKind {
    // ... existing variants ...
    /// W4: turn 结束时检测到未被解决的 permission request。
    /// count > 0 表示存在 orphaned request（已被自动取消）。
    OrphanedPermissionDetected { count: usize },
}
```

**`PendingPermissionControlPlane` trait**（`src-tauri/src/runtime/store/mod.rs`）新增方法签名：

```rust
pub trait PendingPermissionControlPlane: Send + Sync {
    // ... existing methods ...
    fn pending_count_for_session(&self, session_id: &SessionId) -> usize;
    fn cancel_for_session(&self, session_id: &SessionId, message: &str) -> usize;
}
```

`PendingPermissionRequestStore` 实现上述 trait 方法（`cancel_for_session` 已存在于 `SessionRuntime` 层，需下沉到 store 实现）。

### 测试

文件：`src-tauri/tests/review_w4_orphaned_permission_detection_test.rs`

```rust
//! W4 回归测试：orphaned permission 跨 turn 追踪
//! - turn 结束时检测 orphaned requests 并取消
//! - 发射 OrphanedPermissionDetected 事件
//! - 正常 turn（无 orphaned request）不发射事件

// ── 检测并取消 orphaned request ─────────────────────────────────────
// 场景：
//   1. 向 PendingPermissionRequestStore 插入一个 request（模拟发出但未解决的 ask）
//   2. 运行 driver（executor 直接 ContentComplete）
//   3. turn 结束时 driver 应检测到 1 个 orphaned request 并取消
// 期望：
//   - resolution_rx 收到 Cancel { message: "orphaned" 相关文字 }
//   - 事件总线中包含 OrphanedPermissionDetected { count: 1 }

#[tokio::test]
async fn w4_orphaned_permission_is_cancelled_at_turn_end() {
    // ...
}

// ── 正常 turn 无 orphaned request 时不发射事件 ──────────────────────

#[tokio::test]
async fn w4_no_event_when_no_orphaned_permissions() {
    // ...
}

// ── 单元测试：pending_count_for_session ─────────────────────────────

#[test]
fn w4_pending_count_returns_correct_session_count() {
    let store = PendingPermissionRequestStore::new();
    let session_a = SessionId::new("sess-w4-a");
    let session_b = SessionId::new("sess-w4-b");
    // insert 2 for A, 1 for B
    // assert pending_count_for_session(A) == 2
    // assert pending_count_for_session(B) == 1
    // cancel A → pending_count_for_session(A) == 0, B still 1
}
```

### Commit message

```
feat(session-runtime): detect and cancel orphaned permission requests at turn end - W4
```

---

## 实施顺序

```
W1 → W2 → W3 → W4
```

W1 是基础（`is_recoverable()` + 主循环 continue）；W2 复用 W1 的 continue 语义并扩展 `LlmStepResult`；W3 需要 W2 的 `state.max_tokens_recovery_count` 字段已存在；W4 独立但建议最后做（依赖 terminal events 结构稳定）。

## 测试运行命令

```bash
# W1
cd src-tauri && cargo test review_w1 --tests --no-fail-fast -- --nocapture

# W2
cd src-tauri && cargo test review_w2 --tests --no-fail-fast -- --nocapture

# W3
cd src-tauri && cargo test review_w3 --tests --no-fail-fast -- --nocapture

# W4
cd src-tauri && cargo test review_w4 --tests --no-fail-fast -- --nocapture

# 全量回归（确认原有约束不被破坏）
cd src-tauri && cargo test review_ --tests --no-fail-fast
```

## 关键文件索引

| 文件 | 变更范围 |
|---|---|
| `src-tauri/src/runtime/chat/turn_config.rs` | `TurnError::is_recoverable()` / `recovery_message()`；`LlmStepResult::ContentComplete.stop_reason`；`MAX_OUTPUT_TOKENS_RECOVERY_LIMIT`；`TurnIterationState` 新增字段 |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | Step 5b recoverable 错误处理；Step 5c max_tokens 检测；W3 stop_hook 前移；W4 orphaned permission 检查 |
| `src-tauri/src/runtime/hooks/runner.rs` | `HookOutcome.blocking_errors`；`HookOutput.blocking_errors` |
| `src-tauri/src/runtime/events.rs` | `RuntimeEventKind::OrphanedPermissionDetected` |
| `src-tauri/src/runtime/store/mod.rs` | `PendingPermissionControlPlane` trait 新增方法 |
| `src-tauri/tests/review_w1_*.rs` | W1 回归测试 |
| `src-tauri/tests/review_w2_*.rs` | W2 回归测试 |
| `src-tauri/tests/review_w3_*.rs` | W3 回归测试 |
| `src-tauri/tests/review_w4_*.rs` | W4 回归测试 |
