# P1/P1-A Tool Round Ownership 迁移实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 tool round ownership 从 legacy executor 迁移到 RuntimeChatTurnDriver，让 3 条 review 红灯测试变绿。

**Architecture:** 拆分 executor 职责——LLM call + streaming 留在 executor 内部，但 tool dispatch 交还给 RuntimeChatTurnDriver 通过 session-level ToolRoundDriver 执行。核心改动是让 `TauriLegacyTurnExecutor` override `run_chat_turn_with_calls`，将 LLM 产生的 tool calls 返回给 runtime driver，而非在 legacy `agent_loop` 内部自行 dispatch。

**Tech Stack:** Rust, Tauri v2, async_trait, tokio

---

## 当前架构（为什么测试红）

```
send_message → SessionRuntime::run_chat_request
  → RuntimeChatTurnDriver::run_chat_turn
    → executor.run_chat_turn_with_calls()  ← 默认实现返回 vec![]
      → executor.run_chat_turn()
        → legacy_send_message_impl()
          → tokio::spawn(agent_loop)  ← fire-and-forget
            → (内部自建 local QueryEngine/EventBus)
            → (内部 ToolRoundDriver dispatch)   ← 不经过 session-level 组件
    → tool_calls.is_empty() == true  ← 永远走这里
    → ToolRoundDriver::execute_round 永远不执行
```

T1/T2/T3 测试在 session-level QueryEngine 注册 spy tool，在 session-level EventBus 上挂 recording host。由于 tool dispatch 从不经过 session-level 组件，spy 不被调用，events 不被录。

## 目标架构

```
send_message → SessionRuntime::run_chat_request
  → RuntimeChatTurnDriver::run_chat_turn
    → executor.run_chat_turn_with_calls()
      → (单次 LLM call + streaming, 收集 tool_calls)
      → 返回 Vec<RuntimeToolCallRequest>         ← 新行为
    → tool_calls.is_empty() == false
    → ToolRoundDriver::execute_round(session QueryEngine, session EventBus, tool_calls)
      → spy 被调用 ← T1/T3 变绿
      → ToolCallExecuting/ToolCallCompleted → session bus ← T2 变绿
    → (如果仍有 tool calls, 循环调用 executor)     ← 多轮迭代
```

## 关键设计决策

### 多轮迭代问题

agent_loop 是 multi-iteration 的：每轮 LLM call 可能返回 tool calls，tool 执行结果回传 LLM，LLM 再返回更多 tool calls。当前 `run_chat_turn_with_calls` 只调用一次、返回一次。

**方案：在 RuntimeChatTurnDriver 中加迭代循环**

```rust
// chat_turn_driver.rs — executor-backed mode
loop {
    let tool_calls = executor.run_chat_turn_with_calls(request.clone()).await?;
    if tool_calls.is_empty() {
        break; // LLM 返回纯文本，结束
    }
    let outcomes = round_driver.execute_round(turn, &self.event_bus, tool_calls).await;
    // 将 outcomes 回传给 executor 以便下次 LLM call
    executor.feed_tool_results(outcomes).await;
}
```

但这需要修改 `RuntimeTurnExecutor` trait 加一个 `feed_tool_results` 方法，改动面大。

**更简单的方案：一次性返回所有 tool calls**

由于 `agent_loop` 已经是 fire-and-forget spawn（`legacy_send_message_impl` 立即返回），且 `run_chat_turn_with_calls` 的 default impl 就是调用 `run_chat_turn` 后返回 `vec![]`，我们可以：

1. 让 executor 仍然运行完整的 agent_loop（包含多轮 LLM + tool）
2. 但在 tool dispatch 阶段，不用 local QueryEngine/EventBus，而是用从外部传入的 session-level 实例
3. 红灯测试中 `SilentLegacyExecutor` 不产生 tool calls → 不进入 ToolRoundDriver → 仍红

这个方案无法让测试变绿。测试要求的是 session-level dispatch。

**实际可行方案：改变 executor 返回语义**

让 `run_chat_turn_with_calls` 变成一个"执行单次 LLM call 并返回 tool calls"的方法，而非"执行整个 turn"。`RuntimeChatTurnDriver` 负责迭代：

```
loop {
    let (tool_calls, done) = executor.run_llm_iteration(&request, &tool_results).await?;
    if tool_calls.is_empty() || done { break; }
    tool_results = round_driver.execute_round(turn, &bus, tool_calls).await;
}
```

这需要一个新的 trait 方法或改变现有方法的语义。

### 选定方案：扩展 RuntimeTurnExecutor trait

1. 新增 `run_llm_step` 方法：执行单次 LLM call（含 streaming），返回 tool calls 或 empty（表示完成）
2. 新增 `feed_tool_results` 方法：接收上一轮 tool execution 的结果
3. `RuntimeChatTurnDriver` 在 executor-backed 模式中 loop 调用两者
4. `TauriLegacyTurnExecutor` 实现这两个新方法，内部持有 conversation state
5. 保留 `run_chat_turn` / `run_chat_turn_with_calls` 的默认实现（向后兼容）

---

## 文件结构

| 文件 | 变更类型 | 职责 |
|------|----------|------|
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | 修改 | `RuntimeTurnExecutor` trait 新增 `run_llm_step` + `feed_tool_results`；`RuntimeChatTurnDriver::run_chat_turn` executor-backed 分支改为 loop |
| `src-tauri/src/transport/tauri_commands/chat.rs` | 修改 | `TauriLegacyTurnExecutor` 实现新 trait 方法 |
| `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` | 修改 | 提取单次 LLM call 逻辑为可复用函数；移除 local QueryEngine/EventBus 构建 |
| `src-tauri/tests/review_chat_tool_dispatch_runtime_test.rs` | 修改 | `SilentLegacyExecutor` 改为 `MockLlmExecutor`，模拟 LLM 返回 tool calls |
| `docs/superpowers/plans/README.md` | 修改 | 更新 P1/P1-A 状态 |

---

## Task 1: 扩展 RuntimeTurnExecutor trait

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs:43-83`

- [ ] **Step 1: 在 trait 中添加 `run_llm_step` 和 `feed_tool_results` 方法**

```rust
// 在 RuntimeTurnExecutor trait 中添加：

/// Execute a single LLM call (with streaming) and return any tool calls
/// the model emitted. Returns empty Vec when the model emits a final text
/// response with no tool calls (turn is done).
///
/// Default: calls run_chat_turn (full turn) and returns vec![] for
/// backward compatibility with executors that manage their own tool loop.
async fn run_llm_step(
    &self,
    request: ChatTurnRequest,
    _previous_tool_results: &[crate::runtime::chat::tool_round_types::RuntimeToolCallOutcome],
) -> std::result::Result<Vec<RuntimeToolCallRequest>, String> {
    self.run_chat_turn(request).await?;
    Ok(vec![])
}

/// Receive tool execution results from the previous round.
/// Default: no-op (backward compatible).
async fn feed_tool_results(
    &self,
    _outcomes: Vec<crate::runtime::chat::tool_round_types::RuntimeToolCallOutcome>,
) -> std::result::Result<(), String> {
    Ok(())
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --no-run 2>&1 | grep "^error"`
Expected: 无 error（默认实现保证向后兼容）

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/runtime/chat/chat_turn_driver.rs
git commit -m "feat(runtime): extend RuntimeTurnExecutor with run_llm_step + feed_tool_results"
```

---

## Task 2: RuntimeChatTurnDriver loop 改造

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs:151-197`

- [ ] **Step 1: 改造 executor-backed 分支为 loop**

```rust
if let Some(executor) = &self.legacy_executor {
    // Iterative LLM ↔ tool round loop.
    // Each iteration: executor runs one LLM call → returns tool calls
    // → runtime dispatches tools → feeds results back → next iteration.
    let round_driver = ToolRoundDriver::new(self.query_engine.clone());
    let mut previous_outcomes: Vec<crate::runtime::chat::tool_round_types::RuntimeToolCallOutcome> = vec![];

    loop {
        let tool_calls = executor
            .run_llm_step(request.clone(), &previous_outcomes)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        if tool_calls.is_empty() {
            break;
        }

        let round_results = round_driver
            .execute_round(turn, &self.event_bus, tool_calls)
            .await;

        // Collect Ok outcomes for feeding back to executor
        previous_outcomes = round_results
            .iter()
            .filter_map(|r| match r {
                crate::runtime::ToolRoundResult::Ok(outcome) => Some(outcome.clone()),
                crate::runtime::ToolRoundResult::Blocked(blocked) => {
                    Some(crate::runtime::chat::tool_round_types::RuntimeToolCallOutcome {
                        tool_call_id: blocked.tool_call_id.clone(),
                        tool_name: blocked.tool_name.clone(),
                        content: blocked.reason.clone(),
                        is_error: true,
                        file_meta: None,
                        is_degraded: false,
                        degradation_notice: None,
                    })
                }
            })
            .collect();

        executor
            .feed_tool_results(previous_outcomes.clone())
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
    }

    // Emit terminal events
    // ... (保留现有 MessagePersisted / StreamDone / AgentIdle)
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --no-run 2>&1 | grep "^error"`
Expected: 无 error

- [ ] **Step 3: 跑现有绿灯测试确认无回退**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test review_runtime_executor_bypass_test --test chat_runtime_dispatcher_production_path_test --test send_message_production_path_test -- --nocapture`
Expected: 全绿（现有测试用 SilentLegacyExecutor，`run_llm_step` 默认返回 `vec![]`，loop 立即 break）

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/runtime/chat/chat_turn_driver.rs
git commit -m "feat(runtime): RuntimeChatTurnDriver iterative LLM-tool loop via run_llm_step"
```

---

## Task 3: 修改红灯测试使用 MockLlmExecutor

**Files:**
- Modify: `src-tauri/tests/review_chat_tool_dispatch_runtime_test.rs`

- [ ] **Step 1: 创建 MockLlmExecutor 替代 SilentLegacyExecutor**

T1/T2/T3 当前用 `SilentLegacyExecutor`（什么都不做）。需要替换为一个模拟 LLM 行为的 executor：第一次 `run_llm_step` 返回一个指定的 tool call，后续（收到 tool results 后）返回 `vec![]` 表示完成。

```rust
/// Mock executor that simulates an LLM returning exactly one tool call
/// on the first iteration, then finishing after receiving results.
struct MockLlmExecutor {
    tool_name: &'static str,
    returned_calls: Arc<Mutex<bool>>,
}

impl MockLlmExecutor {
    fn new(tool_name: &'static str) -> Self {
        Self {
            tool_name,
            returned_calls: Arc::new(Mutex::new(false)),
        }
    }
}

#[async_trait]
impl RuntimeTurnExecutor for MockLlmExecutor {
    async fn run_chat_turn(&self, _request: ChatTurnRequest) -> Result<(), String> {
        // Not called when run_llm_step is overridden
        Ok(())
    }

    async fn run_llm_step(
        &self,
        _request: ChatTurnRequest,
        _previous_results: &[RuntimeToolCallOutcome],
    ) -> Result<Vec<RuntimeToolCallRequest>, String> {
        let mut returned = self.returned_calls.lock().unwrap();
        if !*returned {
            *returned = true;
            Ok(vec![RuntimeToolCallRequest {
                tool_call_id: format!("tc-mock-{}", self.tool_name),
                tool_name: self.tool_name.to_string(),
                args: serde_json::json!({}),
                purpose: None,
            }])
        } else {
            Ok(vec![])
        }
    }
}
```

- [ ] **Step 2: 替换 T1 中的 SilentLegacyExecutor**

```rust
// T1: 改为
let executor = Arc::new(MockLlmExecutor::new("spy_dispatch_tool_t1"));
```

- [ ] **Step 3: 替换 T2 中的 SilentLegacyExecutor**

```rust
// T2: 改为
let executor = Arc::new(MockLlmExecutor::new("spy_event_tool_t2"));
```

- [ ] **Step 4: 替换 T3**

T3 使用 `TrackingExecutor` 来验证"executor was called"。需要调整：

```rust
/// T3 executor: tracks that run_llm_step was called AND returns a tool call
struct TrackingMockExecutor {
    called: Arc<Mutex<bool>>,
    tool_name: &'static str,
    returned_calls: Arc<Mutex<bool>>,
}

#[async_trait]
impl RuntimeTurnExecutor for TrackingMockExecutor {
    async fn run_chat_turn(&self, _request: ChatTurnRequest) -> Result<(), String> {
        Ok(())
    }

    async fn run_llm_step(
        &self,
        _request: ChatTurnRequest,
        _previous_results: &[RuntimeToolCallOutcome],
    ) -> Result<Vec<RuntimeToolCallRequest>, String> {
        *self.called.lock().unwrap() = true;
        let mut returned = self.returned_calls.lock().unwrap();
        if !*returned {
            *returned = true;
            Ok(vec![RuntimeToolCallRequest {
                tool_call_id: format!("tc-mock-{}", self.tool_name),
                tool_name: self.tool_name.to_string(),
                args: serde_json::json!({}),
                purpose: None,
            }])
        } else {
            Ok(vec![])
        }
    }
}
```

更新 T3 使用 `TrackingMockExecutor`，assertion 1 验证 `called == true`，assertion 2 验证 `spy_called == true`。

- [ ] **Step 5: 跑 3 个测试验证全部变绿**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test review_chat_tool_dispatch_runtime_test -- --nocapture`
Expected: 6 passed, 0 failed

- [ ] **Step 6: 跑全量 review_ 回归**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test review_chat_tool_dispatch_runtime_test --test review_runtime_executor_bypass_test --test review_runtime_executor_duplicate_events_test --test review_permission_denial_normalization_test -- --nocapture`
Expected: 全绿

- [ ] **Step 7: Commit**

```bash
git add src-tauri/tests/review_chat_tool_dispatch_runtime_test.rs
git commit -m "test(P1-A): replace SilentLegacyExecutor with MockLlmExecutor, T1-T3 now green"
```

---

## Task 4: TauriLegacyTurnExecutor 实现 run_llm_step

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs:92-116`
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`

这是最大也是最复杂的 task。需要：

1. 从 `agent_loop` 中提取"单次 LLM call + streaming + 收集 tool_calls"逻辑为独立函数
2. 让 `TauriLegacyTurnExecutor` override `run_llm_step` 调用该函数
3. 移除 `agent_loop` 内部的 local tool dispatch（lines 2769-2827）——tool dispatch 现在由 `RuntimeChatTurnDriver` 负责

**注意：** 这个 task 的改动面最大，涉及 `chat_runtime_impl.rs` 的核心循环拆分。必须 TDD 走 — 先确认 Task 3 的测试能覆盖行为，再改实现。

- [ ] **Step 1: 设计 `LlmStepResult` 返回类型**

```rust
/// Result of a single LLM streaming step.
pub(crate) struct LlmStepResult {
    /// Tool calls the LLM wants to execute (empty = turn done).
    pub tool_calls: Vec<RuntimeToolCallRequest>,
    /// Whether the LLM streaming was cancelled.
    pub cancelled: bool,
}
```

- [ ] **Step 2: 给 TauriLegacyTurnExecutor 添加可变状态**

executor 需要在多轮间保持 messages / context / settings 等状态。添加 interior mutability：

```rust
struct TauriLegacyTurnExecutor {
    services: TauriChatServices,
    // Interior state for multi-round LLM loop
    state: tokio::sync::Mutex<Option<LegacyTurnState>>,
}

struct LegacyTurnState {
    messages: Vec<ChatMessage>,
    settings: AppSettings,
    // ... other per-turn state extracted from agent_loop
}
```

- [ ] **Step 3: 实现 `run_llm_step` — 执行单次 LLM streaming**

将 `agent_loop` 的核心 loop body（lines 2171-2730）中的"单次 LLM call"部分提取为 executor 的 `run_llm_step` 实现。

- [ ] **Step 4: 实现 `feed_tool_results` — 将 tool outcomes 回传为 messages**

将 `agent_loop` 的 tool result processing（lines 2829-2966）中的"将 tool results 加入 messages"部分提取为 `feed_tool_results` 实现。

- [ ] **Step 5: 移除 agent_loop 中的 local tool dispatch**

删除 `agent_loop` 中 lines 2740-2827 的 local `PluginContext` / `QueryEngine` / `EventBus` 构建和 `ToolRoundDriver` 调用。这部分现在由 `RuntimeChatTurnDriver` 负责。

- [ ] **Step 6: 编译验证**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --no-run 2>&1 | grep "^error"`

- [ ] **Step 7: 跑全量回归**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test review_chat_tool_dispatch_runtime_test --test review_runtime_executor_bypass_test --test chat_runtime_dispatcher_production_path_test --test send_message_production_path_test --test workspace_first_agent_golden_path_test -- --nocapture`
Expected: 全绿

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/transport/tauri_commands/chat.rs src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs
git commit -m "feat(P1-A): TauriLegacyTurnExecutor implements run_llm_step, tool dispatch via session runtime"
```

---

## Task 5: 更新文档和最终验证

**Files:**
- Modify: `docs/superpowers/plans/README.md`
- Modify: `docs/reviews/2026-04-14-chat-runtime-first-closure-review.md`

- [ ] **Step 1: 跑 review_ 全量回归**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test review_chat_tool_dispatch_runtime_test --test review_runtime_executor_bypass_test --test review_runtime_executor_duplicate_events_test --test review_permission_denial_normalization_test -- --nocapture`
Expected: 全绿（T1-T6 全部绿）

- [ ] **Step 2: 跑 Rust 全量测试**

Run: `cd src-tauri && cargo test`
Expected: 全绿（除了已知的非相关测试）

- [ ] **Step 3: 更新 README P1/P1-A 状态为已关闭**

- [ ] **Step 4: 更新 closure review 文档状态**

- [ ] **Step 5: Commit**

```bash
git add docs/
git commit -m "docs: mark P1/P1-A as closed after tool round ownership migration"
```

---

## 风险与注意事项

1. **Task 4 是高风险拆分**：`agent_loop` 有 ~3000 行，内部状态复杂。拆分单次 LLM call 逻辑时必须保证 streaming delta 事件、cancel 检查、ghost call recovery 等边界行为不变。
2. **fire-and-forget 模式变化**：当前 `legacy_send_message_impl` 立即 spawn 并返回。新模式下 `run_llm_step` 必须在 executor 的 async 上下文中等待 LLM streaming 完成。需要确认这不会 block Tauri IPC。
3. **多轮迭代的状态管理**：每轮 LLM call 需要前一轮的 tool results 作为 messages 输入。executor 需要持有可变 state。
4. **向后兼容**：`run_llm_step` 有默认实现，所有测试中的现有 executor 不需要改。只有真正要接管 tool dispatch 的 `TauriLegacyTurnExecutor` 需要 override。

## 实施优先级

Task 1-3（trait + driver loop + 测试）可以先做并验证，不需要改 production executor。
Task 4（真正的 production executor 实现）是最大的改动，也是让 3 个红灯在 production 路径上真正变绿的关键。
Task 5 在全部测试通过后做。
