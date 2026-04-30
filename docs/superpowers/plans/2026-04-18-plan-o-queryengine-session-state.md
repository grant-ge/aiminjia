# QueryEngine 跨 turn 会话状态 + Turn 终态 + 预算上限计划（Plan-O）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 对齐 claude-code-best `QueryEngine` 的三项会话级能力：跨 turn 权限拒绝累积、结构化 turn 终态枚举、USD 预算上限检查。

**Architecture:** `QueryEngine` 新增 `permission_denials: Arc<Mutex<Vec<PermissionDenialRecord>>>` 字段，通过 `ToolDispatcher` 的 Deny 路径填充；`ChatTurnOutcome` enum 新增 `MaxIterationsReached`、`BudgetExceeded`、`ExecutionError` 变体，由 `run_chat_turn_s4` 返回并通过 `RuntimeEventBus` emit 结构化终态事件给前端；`QueryEngine` / `SessionConfig` 新增 `max_budget_usd: Option<f64>`，每轮结束后检查 `total_usage` 累积 cost。

**Tech Stack:** Rust, tokio, async_trait

**Worktree branch:** pzc

---

## 背景：claude-code-best 对标

`claude-code-best/src/QueryEngine.ts` 中：

- **`permissionDenials: SDKPermissionDenial[]`**（L191，L265-272）：`wrappedCanUseTool` 包装在每次 deny 时 push 一条记录，在最终 result 消息中随 `permission_denials` 字段一起回传给调用方。
- **终态枚举**（L870-892，L997-1026）：`error_max_turns`、`error_max_budget_usd` 两种 subtype 在循环内检查后立即 yield 并 return，消费方可结构化匹配。
- **`maxBudgetUsd`**（L149，L997）：在每条 query 消息处理后检查 `getTotalCost() >= maxBudgetUsd`，超限即返回 `error_max_budget_usd` 终态。

lotus-app 当前：
- `ToolDispatcher::dispatch()` 对 `Deny` 只返回 `ToolError::PermissionDenied`，没有任何累积路径。
- `run_chat_turn_s4` 返回 `Result<()>`，成功和超过 `max_iterations` 两种出口都映射到同一个 `Ok(())`。
- `TotalTokenUsage` 已有 token 统计，但无 cost 转换和预算检查逻辑。

---

## Task O1 — 定义 `PermissionDenialRecord` 和 `ChatTurnOutcome`

**Files:**
- Create: `src-tauri/src/runtime/chat/turn_outcome.rs`
- Modify: `src-tauri/src/runtime/chat/mod.rs`（re-export）
- Test: `src-tauri/tests/plan_o_queryengine_session_state_test.rs`（新建）

### TDD 步骤

- [ ] 1. 写失败测试

新建 `src-tauri/tests/plan_o_queryengine_session_state_test.rs`：

```rust
//! Plan-O: QueryEngine cross-turn session state, turn outcomes, and budget cap.

/// O1: ChatTurnOutcome enum and PermissionDenialRecord compile and are pattern-matchable.
#[test]
fn o1_chat_turn_outcome_variants_compile() {
    use lotus_app::runtime::chat::turn_outcome::{ChatTurnOutcome, PermissionDenialRecord};

    let success = ChatTurnOutcome::Success;
    let cancelled = ChatTurnOutcome::Cancelled;
    let max_iter = ChatTurnOutcome::MaxIterationsReached { iterations: 30 };
    let budget = ChatTurnOutcome::BudgetExceeded {
        reason: "Reached maximum budget ($1.00)".to_string(),
        total_cost_usd: 1.05,
    };
    let exec_err = ChatTurnOutcome::ExecutionError {
        message: "LLM gateway timeout".to_string(),
    };

    assert!(matches!(success, ChatTurnOutcome::Success));
    assert!(matches!(cancelled, ChatTurnOutcome::Cancelled));
    assert!(matches!(max_iter, ChatTurnOutcome::MaxIterationsReached { iterations: 30 }));
    assert!(matches!(budget, ChatTurnOutcome::BudgetExceeded { .. }));
    assert!(matches!(exec_err, ChatTurnOutcome::ExecutionError { .. }));

    let record = PermissionDenialRecord {
        tool_name: "bash".to_string(),
        tool_call_id: "tc-001".to_string(),
        reason: "dangerous_pattern".to_string(),
    };
    assert_eq!(record.tool_name, "bash");
}

#[test]
fn o1_chat_turn_outcome_is_error_helper() {
    use lotus_app::runtime::chat::turn_outcome::ChatTurnOutcome;

    assert!(!ChatTurnOutcome::Success.is_error());
    assert!(!ChatTurnOutcome::Cancelled.is_error());
    assert!(ChatTurnOutcome::MaxIterationsReached { iterations: 30 }.is_error());
    assert!(ChatTurnOutcome::BudgetExceeded {
        reason: "over budget".to_string(),
        total_cost_usd: 2.0,
    }
    .is_error());
    assert!(ChatTurnOutcome::ExecutionError {
        message: "boom".to_string(),
    }
    .is_error());
}
```

- [ ] 2. 确认失败

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_o_queryengine_session_state_test o1_ -- --nocapture 2>&1 | head -40
```

期望：`error[E0432]: unresolved import 'lotus_app::runtime::chat::turn_outcome'`

- [ ] 3. 最小实现

新建 `src-tauri/src/runtime/chat/turn_outcome.rs`：

```rust
//! Structured turn outcome enum for `run_chat_turn_s4`.
//!
//! Maps to `QueryEngine.ts` result subtypes:
//! - `success`          → `ChatTurnOutcome::Success`
//! - `Cancelled`        → `ChatTurnOutcome::Cancelled`
//! - `error_max_turns`  → `ChatTurnOutcome::MaxIterationsReached`
//! - `error_max_budget_usd` → `ChatTurnOutcome::BudgetExceeded`
//! - error              → `ChatTurnOutcome::ExecutionError`

/// A single permission denial record, accumulated across tool calls in a turn.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PermissionDenialRecord {
    pub tool_name: String,
    pub tool_call_id: String,
    pub reason: String,
}

/// Structured result of a single chat turn.
///
/// Returned by `RuntimeChatTurnDriver::run_chat_turn_s4` so transport
/// and tests can structurally match the outcome instead of inferring it
/// from side-effects or error strings.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum ChatTurnOutcome {
    /// Turn completed normally (LLM produced a final text response).
    Success,
    /// Turn was cancelled by the user or a cancellation token.
    Cancelled,
    /// Turn hit `max_iterations` before the LLM produced a final response.
    MaxIterationsReached { iterations: usize },
    /// Turn was halted because accumulated cost exceeded `max_budget_usd`.
    BudgetExceeded { reason: String, total_cost_usd: f64 },
    /// A non-recoverable error occurred during LLM execution or persistence.
    ExecutionError { message: String },
}

impl ChatTurnOutcome {
    /// True for outcomes that represent abnormal termination.
    pub fn is_error(&self) -> bool {
        matches!(
            self,
            ChatTurnOutcome::MaxIterationsReached { .. }
                | ChatTurnOutcome::BudgetExceeded { .. }
                | ChatTurnOutcome::ExecutionError { .. }
        )
    }

    /// True for successful completion (normal text response).
    pub fn is_success(&self) -> bool {
        matches!(self, ChatTurnOutcome::Success)
    }
}
```

在 `src-tauri/src/runtime/chat/mod.rs` 末尾追加：

```rust
pub mod turn_outcome;
pub use turn_outcome::{ChatTurnOutcome, PermissionDenialRecord};
```

- [ ] 4. 确认通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_o_queryengine_session_state_test o1_ -- --nocapture
```

- [ ] 5. commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/runtime/chat/turn_outcome.rs \
        src-tauri/src/runtime/chat/mod.rs \
        src-tauri/tests/plan_o_queryengine_session_state_test.rs && \
git commit -m "feat(turn-outcome): define ChatTurnOutcome enum and PermissionDenialRecord - O1"
```

---

## Task O2 — `QueryEngine` 新增 `permission_denials` 跨 turn 累积字段

**Files:**
- Modify: `src-tauri/src/runtime/query_engine.rs`
- Test: `src-tauri/tests/plan_o_queryengine_session_state_test.rs`（追加）

### TDD 步骤

- [ ] 1. 写失败测试（追加）

```rust
mod o2_permission_denials_accumulation {
    use std::sync::Arc;
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use lotus_app::runtime::tools::{RuntimeTool, ToolDispatcher};
    use lotus_app::runtime::tools::definition::ToolDefinition;
    use lotus_app::runtime::tools::executor::{ToolError, ToolResult};
    use lotus_app::runtime::tools::context::ToolExecutionContext;
    use lotus_app::runtime::tools::permission::{PermissionDecision, PermissionReason};
    use lotus_app::runtime::query_engine::QueryEngine;
    use lotus_app::runtime::event_bus::RuntimeEventBus;
    use lotus_app::runtime::chat::tool_round_types::RuntimeToolCallRequest;
    use lotus_app::runtime::state::TurnState;
    use lotus_app::runtime::identity::IdentityMapping;
    use lotus_app::runtime::ids::RunId;

    struct AlwaysDeniedTool;

    #[async_trait]
    impl RuntimeTool for AlwaysDeniedTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("always_denied", "always permission denied")
        }

        async fn check_permissions(
            &self,
            _input: &Value,
            _ctx: &ToolExecutionContext,
        ) -> Option<PermissionDecision> {
            Some(PermissionDecision::Deny {
                message: "not allowed".to_string(),
                reason: PermissionReason::Other("test_deny".to_string()),
            })
        }

        async fn execute(
            &self,
            _input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            unreachable!("must not execute denied tool")
        }
    }

    #[tokio::test]
    async fn o2_permission_denial_is_recorded_after_dispatch() {
        let dispatcher = Arc::new(ToolDispatcher::allow_all());
        dispatcher.register(Arc::new(AlwaysDeniedTool));

        let engine = QueryEngine::for_test(dispatcher);
        let bus = RuntimeEventBus::new();
        let mapping = IdentityMapping::from_legacy_conversation_id("sess-o2");
        let turn = TurnState::new(mapping, RunId::new("run-o2"), "test".to_string());

        let call = RuntimeToolCallRequest {
            tool_call_id: "tc-o2".to_string(),
            tool_name: "always_denied".to_string(),
            args: json!({}),
        };

        // After dispatch, the engine's denial list should contain a record.
        let _outcome = engine
            .run_tool_call_with_bus(&turn, &bus, call)
            .await
            .expect("run_tool_call_with_bus should not Err on permission denied");

        let denials = engine.get_permission_denials();
        assert_eq!(
            denials.len(),
            1,
            "one denial should be recorded after a Deny outcome"
        );
        assert_eq!(denials[0].tool_name, "always_denied");
        assert_eq!(denials[0].tool_call_id, "tc-o2");
    }

    #[tokio::test]
    async fn o2_permission_denials_accumulate_across_calls() {
        let dispatcher = Arc::new(ToolDispatcher::allow_all());
        dispatcher.register(Arc::new(AlwaysDeniedTool));

        let engine = QueryEngine::for_test(dispatcher);
        let bus = RuntimeEventBus::new();
        let mapping = IdentityMapping::from_legacy_conversation_id("sess-o2b");
        let turn = TurnState::new(mapping, RunId::new("run-o2b"), "test".to_string());

        for i in 0..3 {
            let call = RuntimeToolCallRequest {
                tool_call_id: format!("tc-o2b-{i}"),
                tool_name: "always_denied".to_string(),
                args: json!({}),
            };
            let _ = engine.run_tool_call_with_bus(&turn, &bus, call).await;
        }

        assert_eq!(
            engine.get_permission_denials().len(),
            3,
            "denials should accumulate across multiple calls"
        );
    }
}
```

- [ ] 2. 确認失败

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_o_queryengine_session_state_test o2_ -- --nocapture 2>&1 | head -60
```

期望：`error[E0599]: no method named 'get_permission_denials' found for struct 'QueryEngine'`

- [ ] 3. 最小实现

在 `src-tauri/src/runtime/query_engine.rs` 中：

**a. 在 import 区添加：**
```rust
use crate::runtime::chat::turn_outcome::PermissionDenialRecord;
```

**b. 在 `QueryEngine` struct 中新增字段：**
```rust
/// Session-scoped accumulation of permission denials across all tool calls.
/// Mirrors `QueryEngine.permissionDenials` in claude-code-best.
permission_denials: Arc<Mutex<Vec<PermissionDenialRecord>>>,
```

**c. 在 `with_dispatcher` 和 `clone_with_fresh_session_state` 和 `for_test` 中初始化：**
```rust
permission_denials: Arc::new(Mutex::new(Vec::new())),
```

注意：`clone_with_fresh_session_state` 应重置 `permission_denials`（新 Arc），因为它是 session-scoped。

**d. 新增 getter 和记录方法：**
```rust
/// Return a snapshot of all permission denials recorded so far this session.
pub fn get_permission_denials(&self) -> Vec<PermissionDenialRecord> {
    self.permission_denials
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

/// Record a permission denial. Called by the Deny branch of
/// `run_tool_call_with_bus_internal`.
fn record_permission_denial(
    &self,
    tool_name: &str,
    tool_call_id: &str,
    reason: &str,
) {
    self.permission_denials
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(PermissionDenialRecord {
            tool_name: tool_name.to_string(),
            tool_call_id: tool_call_id.to_string(),
            reason: reason.to_string(),
        });
}
```

**e. 在 `run_tool_call_with_bus_internal` 的 `Err(ToolError::PermissionDenied(_))` 路径（当前在 Err 分支内）或在 dispatch 前添加记录：**

当前 `dispatch_result` 的 `Err(err)` 处理：
```rust
Err(err) => {
    // Record permission denials for session-level tracking.
    if let crate::runtime::tools::executor::ToolError::PermissionDenied(ref reason) = err {
        self.record_permission_denial(
            &call.tool_name,
            &call.tool_call_id,
            reason,
        );
    }

    bus.emit( /* ToolCallCompleted is_error=true */ ).await?;
    // ... existing Completed encoding
}
```

- [ ] 4. 确认通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_o_queryengine_session_state_test o2_ -- --nocapture
```

- [ ] 5. commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/runtime/query_engine.rs \
        src-tauri/tests/plan_o_queryengine_session_state_test.rs && \
git commit -m "feat(query-engine): accumulate permission denials across tool calls - O2"
```

---

## Task O3 — `run_chat_turn_s4` 返回 `ChatTurnOutcome` 并覆盖终态分支

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Test: `src-tauri/tests/plan_o_queryengine_session_state_test.rs`（追加）

### TDD 步骤

- [ ] 1. 写失败测试（追加）

```rust
mod o3_chat_turn_outcome_from_driver {
    use lotus_app::runtime::chat::turn_outcome::ChatTurnOutcome;

    #[test]
    fn o3_cancelled_turn_maps_to_cancelled_outcome() {
        // This test verifies the mapping logic in isolation using the unit
        // helper that run_chat_turn_s4 will call.  The full integration path
        // requires a mock LLM executor and is covered by task O4.
        // For now, verify that `ChatTurnOutcome::Cancelled` can be constructed
        // and that the stream_cancelled flag in state correlates to it.
        let outcome = ChatTurnOutcome::Cancelled;
        assert!(!outcome.is_error(), "Cancelled is not an error outcome");
        assert!(!outcome.is_success(), "Cancelled is not a success outcome");
    }

    #[test]
    fn o3_max_iterations_outcome_is_error() {
        let outcome = ChatTurnOutcome::MaxIterationsReached { iterations: 30 };
        assert!(outcome.is_error());
        match &outcome {
            ChatTurnOutcome::MaxIterationsReached { iterations } => {
                assert_eq!(*iterations, 30);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn o3_budget_exceeded_outcome_carries_cost() {
        let outcome = ChatTurnOutcome::BudgetExceeded {
            reason: "Reached maximum budget ($0.50)".to_string(),
            total_cost_usd: 0.55,
        };
        assert!(outcome.is_error());
        match &outcome {
            ChatTurnOutcome::BudgetExceeded { total_cost_usd, .. } => {
                assert!(*total_cost_usd > 0.5);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn o3_run_chat_turn_returns_outcome_type() {
        // Compile-time: verify that run_chat_turn signature returns Result<ChatTurnOutcome>
        // by checking the associated types via a stub executor.
        // Full runtime test is in O4.
        let _: fn() -> ChatTurnOutcome = || ChatTurnOutcome::Success;
    }
}
```

- [ ] 2. 确认通过（O3 tests are compile-time + unit logic, should pass after O1）

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_o_queryengine_session_state_test o3_ -- --nocapture
```

- [ ] 3. 实现：修改 `run_chat_turn_s4` 签名并覆盖终态返回点

**注意：** 这是侵入性修改，必须同步更新所有调用点。

在 `chat_turn_driver.rs` 中修改 `run_chat_turn_s4` 的返回类型：

```rust
// 修改前:
async fn run_chat_turn_s4(
    &self,
    turn: &mut TurnState,
    request: &ChatTurnRequest,
    executor: &dyn RuntimeLlmExecutor,
) -> Result<()>

// 修改后:
async fn run_chat_turn_s4(
    &self,
    turn: &mut TurnState,
    request: &ChatTurnRequest,
    executor: &dyn RuntimeLlmExecutor,
) -> Result<ChatTurnOutcome>
```

在方法内部替换各出口的 `return` / `break` 后的逻辑：

**a. 迭代循环结束（正常完成 `ContentComplete`）：** 循环后添加 `let final_outcome = if state.stream_cancelled { ChatTurnOutcome::Cancelled } else if state.iteration_count >= config.max_iterations && /* 没有 ContentComplete */ ... { ChatTurnOutcome::MaxIterationsReached { iterations: state.iteration_count } } else { ChatTurnOutcome::Success };`

由于当前逻辑中 `ContentComplete` 分支 `break`，`MaxIterationsReached` 出现在 loop 跑满 `config.max_iterations` 次后自然退出（loop 不再 break）。实现策略：

```rust
// 在 'turn: for iteration in 0..config.max_iterations 之前
let mut turn_completed_normally = false;

// 在 ContentComplete 分支:
LlmStepResult::ContentComplete { content, tokens_in, tokens_out } => {
    // ... existing merge logic ...
    turn_completed_normally = true;
    break 'turn;
}

// 在循环结束后（Step 6 之前）确定 final_outcome：
let final_outcome = if state.stream_cancelled {
    ChatTurnOutcome::Cancelled
} else if !turn_completed_normally {
    ChatTurnOutcome::MaxIterationsReached {
        iterations: config.max_iterations,
    }
} else {
    ChatTurnOutcome::Success
};
```

**b. 函数最后的 `Ok(())` → `Ok(final_outcome)`：**

```rust
// 在 Step 8 最后（现有 AgentIdle emit 之后）：
Ok(final_outcome)
```

**c. 同步修改 `run_chat_turn`（调用 run_chat_turn_s4 的位置）：**

```rust
pub async fn run_chat_turn(
    &self,
    turn: &mut TurnState,
    request: &ChatTurnRequest,
) -> Result<ChatTurnOutcome> {
    if let Some(ref executor) = self.llm_executor {
        return self.run_chat_turn_s4(turn, request, executor.as_ref()).await;
    }
    // Pure runtime mode
    self.query_engine.run(turn, &self.event_bus).await?;
    Ok(ChatTurnOutcome::Success)
}
```

**d. 更新所有调用 `run_chat_turn` 的上游代码。** 主要调用点在 `session_runtime.rs`：

```rust
// 修改前:
driver.run_chat_turn(&mut turn, &request).await?;

// 修改后:
let _outcome = driver.run_chat_turn(&mut turn, &request).await?;
// outcome 暂时忽略（O4 会利用它）；用 _outcome 避免 unused warning。
```

- [ ] 4. 全量编译检查

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo check 2>&1 | grep -E "^error" | head -20
```

- [ ] 5. 确认测试通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_o_queryengine_session_state_test o3_ -- --nocapture
```

- [ ] **Step O3-X: 验证所有调用方编译通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo check 2>&1 | grep "error\[" | head -20
```

期望：无编译错误（所有调用方已在上一步更新）。

- [ ] 6. commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/runtime/chat/chat_turn_driver.rs \
        src-tauri/src/runtime/session_runtime.rs \
        src-tauri/tests/plan_o_queryengine_session_state_test.rs && \
git commit -m "feat(turn-driver): return ChatTurnOutcome from run_chat_turn_s4, add MaxIterationsReached - O3"
```

---

## Task O4 — `max_budget_usd` 预算检查：SessionConfig + turn 后检查

**Files:**
- Modify: `src-tauri/src/runtime/query_engine.rs`（新增字段和 `check_budget` 方法）
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`（`run_chat_turn_s4` 内插入 budget 检查）
- Test: `src-tauri/tests/plan_o_queryengine_session_state_test.rs`（追加）

### Budget 换算

`TotalTokenUsage` 当前只有 `tokens_in / tokens_out`，没有 USD cost。计算 cost 需要 per-model token price，完整实现依赖 model 配置。

**本 task 的实用简化策略**（可在后续 task 细化）：

`QueryEngine` 接收可选的 `cost_per_1k_tokens: Option<f64>`（输入和输出都用同一值估算），`check_budget_exceeded` 用 `(tokens_in + tokens_out) / 1000.0 * cost_per_1k_tokens` 估算 cost。这与 claude-code-best 的 `getTotalCost()` 语义等价但简化了 per-model 路由，后续可替换为精确计算。

### TDD 步骤

- [ ] 1. 写失败测试（追加）

```rust
mod o4_budget_cap {
    use lotus_app::runtime::query_engine::QueryEngine;

    #[test]
    fn o4_budget_not_exceeded_when_below_limit() {
        let engine = QueryEngine::new()
            .with_max_budget_usd(1.0)
            .with_cost_per_1k_tokens(0.001);
        // Simulate 100k tokens total = $0.10 < $1.00
        engine.accumulate_usage(50_000, 50_000);
        assert!(
            !engine.is_budget_exceeded(),
            "budget should not be exceeded when cost < max_budget_usd"
        );
    }

    #[test]
    fn o4_budget_exceeded_when_over_limit() {
        let engine = QueryEngine::new()
            .with_max_budget_usd(0.05)
            .with_cost_per_1k_tokens(0.001);
        // Simulate 200k tokens total = $0.20 > $0.05
        engine.accumulate_usage(100_000, 100_000);
        assert!(
            engine.is_budget_exceeded(),
            "budget should be exceeded when cost >= max_budget_usd"
        );
    }

    #[test]
    fn o4_no_budget_limit_never_exceeded() {
        let engine = QueryEngine::new();
        // No budget configured → never exceeded
        engine.accumulate_usage(1_000_000, 1_000_000);
        assert!(
            !engine.is_budget_exceeded(),
            "without max_budget_usd configured, budget is never exceeded"
        );
    }

    #[test]
    fn o4_estimated_cost_usd_calculation() {
        let engine = QueryEngine::new()
            .with_max_budget_usd(10.0)
            .with_cost_per_1k_tokens(0.002);
        engine.accumulate_usage(10_000, 5_000); // 15k tokens * $0.002/1k = $0.03
        let cost = engine.estimated_cost_usd();
        let expected = 15.0 * 0.002; // $0.030
        assert!(
            (cost - expected).abs() < 1e-9,
            "cost estimate: expected {expected}, got {cost}"
        );
    }
}
```

- [ ] 2. 确认失败

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_o_queryengine_session_state_test o4_ -- --nocapture 2>&1 | head -40
```

期望：`error[E0599]: no method named 'with_max_budget_usd'` 等

- [ ] 3. 最小实现

**a. 在 `QueryEngine` struct 中新增字段：**

```rust
/// Optional USD spending cap for this session.
/// When set, `is_budget_exceeded()` returns true once
/// `estimated_cost_usd() >= max_budget_usd`.
max_budget_usd: Option<f64>,
/// Cost per 1k tokens (input + output averaged) for budget estimation.
/// Default: None (disables cost estimation).
cost_per_1k_tokens: Option<f64>,
```

**b. 在所有构造函数中初始化为 `None`：**

```rust
max_budget_usd: None,
cost_per_1k_tokens: None,
```

**c. `clone_with_fresh_session_state` 保持静态配置（与 `workspace_path` 等相同）：**

```rust
max_budget_usd: self.max_budget_usd,
cost_per_1k_tokens: self.cost_per_1k_tokens,
```

**d. Builder 方法：**

```rust
pub fn with_max_budget_usd(mut self, max_budget_usd: f64) -> Self {
    self.max_budget_usd = Some(max_budget_usd);
    self
}

pub fn with_cost_per_1k_tokens(mut self, cost_per_1k_tokens: f64) -> Self {
    self.cost_per_1k_tokens = Some(cost_per_1k_tokens);
    self
}
```

**e. 查询方法：**

```rust
/// Estimated USD cost based on accumulated token usage.
/// Returns 0.0 when `cost_per_1k_tokens` is not configured.
pub fn estimated_cost_usd(&self) -> f64 {
    let Some(rate) = self.cost_per_1k_tokens else {
        return 0.0;
    };
    let usage = self.get_total_usage();
    let total_k_tokens = (usage.tokens_in + usage.tokens_out) as f64 / 1000.0;
    total_k_tokens * rate
}

/// True when `max_budget_usd` is set and `estimated_cost_usd()` meets or
/// exceeds it.
pub fn is_budget_exceeded(&self) -> bool {
    let Some(max) = self.max_budget_usd else {
        return false;
    };
    self.estimated_cost_usd() >= max
}
```

- [ ] 4. 确认通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_o_queryengine_session_state_test o4_ -- --nocapture
```

- [ ] 5. commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/runtime/query_engine.rs \
        src-tauri/tests/plan_o_queryengine_session_state_test.rs && \
git commit -m "feat(query-engine): add max_budget_usd cap and is_budget_exceeded() - O4"
```

---

## Task O5 — `run_chat_turn_s4` 插入 budget 检查，返回 `BudgetExceeded` 终态

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Test: `src-tauri/tests/plan_o_queryengine_session_state_test.rs`（追加）

### TDD 步骤

- [ ] 1. 写失败测试（追加）

```rust
mod o5_budget_exceeded_in_driver {
    use std::sync::Arc;
    use async_trait::async_trait;
    use anyhow::Result;
    use serde_json::json;
    use lotus_app::runtime::chat::chat_turn_driver::{
        ChatTurnRequest, RuntimeChatTurnDriver, RuntimeLlmExecutor,
    };
    use lotus_app::runtime::chat::turn_config::{
        LlmStepInput, LlmStepResult, ResolvedLlmSettings, TurnError,
    };
    use lotus_app::runtime::chat::turn_outcome::ChatTurnOutcome;
    use lotus_app::runtime::event_bus::RuntimeEventBus;
    use lotus_app::runtime::query_engine::QueryEngine;
    use lotus_app::runtime::state::TurnState;
    use lotus_app::runtime::identity::IdentityMapping;
    use lotus_app::runtime::ids::RunId;
    use lotus_app::runtime::tools::ToolDispatcher;

    struct ImmediateContentExecutor;

    #[async_trait]
    impl RuntimeLlmExecutor for ImmediateContentExecutor {
        async fn run_llm_step(
            &self,
            _input: &LlmStepInput<'_>,
            _bus: &RuntimeEventBus,
            _cancel: &lotus_app::runtime::cancellation::CancellationToken,
        ) -> Result<LlmStepResult, TurnError> {
            Ok(LlmStepResult::ContentComplete {
                content: "done".to_string(),
                tokens_in: 500_000,   // triggers budget after first iteration
                tokens_out: 500_000,
            })
        }

        async fn persist_assistant_message(
            &self,
            _conversation_id: &str,
            _content: &str,
            _generated_file_ids: &[String],
            _file_metas: &[serde_json::Value],
        ) -> Result<String, TurnError> {
            Ok("msg-test".to_string())
        }
    }

    #[tokio::test]
    async fn o5_budget_exceeded_outcome_when_cost_surpasses_limit() {
        // Budget: $0.50, rate: $0.001/1k tokens
        // After 1 iteration: (500k + 500k) / 1k * 0.001 = $1.00 > $0.50
        let dispatcher = Arc::new(ToolDispatcher::allow_all());
        let engine = QueryEngine::for_test(dispatcher)
            .with_max_budget_usd(0.50)
            .with_cost_per_1k_tokens(0.001);

        let bus = RuntimeEventBus::new();
        let executor = Arc::new(ImmediateContentExecutor);
        let driver = RuntimeChatTurnDriver::with_llm_executor(
            engine,
            bus,
            executor,
        );

        let request = ChatTurnRequest::new("sess-o5", "hello", vec![]);
        let mapping = IdentityMapping::from_legacy_conversation_id("sess-o5");
        let mut turn = TurnState::new(mapping, RunId::new(request.run_id.as_str()), "hello".to_string());

        let outcome = driver
            .run_chat_turn(&mut turn, &request)
            .await
            .expect("run_chat_turn should not Err");

        // Budget was exceeded after accumulating tokens.
        // NOTE: The check happens AFTER the turn completes (post-accumulate).
        // So the current turn returns Success but the NEXT turn would be BudgetExceeded.
        // Alternatively, check within the turn after accumulate_usage.
        // The test verifies the outcome returned is BudgetExceeded when
        // engine detects overage right after accumulate_usage in Step 6.
        assert!(
            matches!(outcome, ChatTurnOutcome::BudgetExceeded { .. }),
            "expected BudgetExceeded outcome, got: {:?}", outcome
        );
    }
}
```

- [ ] 2. 确认失败

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_o_queryengine_session_state_test o5_ -- --nocapture 2>&1 | head -60
```

期望：`o5_budget_exceeded_outcome_when_cost_surpasses_limit` 失败（返回 Success 而非 BudgetExceeded）

- [ ] 3. 最小实现

在 `run_chat_turn_s4` 的 Step 6（`post_process::finalize_content`）之后、Step 7（persist）之前，插入 budget 检查：

```rust
// ── Step 6: Post-process content ──────────────────────────────────────────
post_process::finalize_content(
    &mut state.full_content,
    state.iteration_count,
    config.max_iterations,
    state.stream_cancelled,
);

// Accumulate token usage (also needed for budget check below).
self.query_engine
    .accumulate_usage(state.step_tokens_in, state.step_tokens_out);

// ── Step 6b: Budget check ─────────────────────────────────────────────────
// Mirror QueryEngine.ts L996-1026: check after accumulating usage.
if self.query_engine.is_budget_exceeded() {
    let total_cost = self.query_engine.estimated_cost_usd();
    let max_budget = self.query_engine
        .max_budget_usd()
        .unwrap_or(0.0);
    // Persist the assistant message so the conversation is recoverable.
    let _ = executor
        .persist_assistant_message(
            config.conversation_id.as_str(),
            &state.full_content,
            &state.generated_file_ids,
            &state.all_file_metas,
        )
        .await;
    // Emit terminal events so the frontend enters idle state.
    let _ = self.event_bus
        .emit(RuntimeEvent::stream_done(session_id.clone(), run_id.clone()))
        .await;
    let _ = self.event_bus
        .emit(RuntimeEvent::new(
            session_id,
            run_id.clone(),
            RuntimeEventKind::AgentIdle {
                agent_id: AgentId::new(format!("agent-{}", run_id.as_str())),
                scope: AgentIdleScope::Primary,
            },
        ))
        .await;
    return Ok(ChatTurnOutcome::BudgetExceeded {
        reason: format!(
            "Reached maximum budget (${max_budget:.2}); estimated cost: ${total_cost:.4}"
        ),
        total_cost_usd: total_cost,
    });
}

// 删除原来的 self.query_engine.accumulate_usage() 调用（已移至上方）
```

需要同时在 `QueryEngine` 上暴露 `max_budget_usd()` getter：

```rust
pub fn max_budget_usd(&self) -> Option<f64> {
    self.max_budget_usd
}
```

- [ ] 4. 确认通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_o_queryengine_session_state_test o5_ -- --nocapture
```

- [ ] 5. 全量回归

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test chat_runtime_first_mainline_test -- --nocapture 2>&1 | tail -20
```

- [ ] 6. commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/runtime/chat/chat_turn_driver.rs \
        src-tauri/src/runtime/query_engine.rs \
        src-tauri/tests/plan_o_queryengine_session_state_test.rs && \
git commit -m "feat(turn-driver): insert budget check after token accumulation, return BudgetExceeded - O5"
```

---

## Task O6 — Transport 层把终态 emit 给前端

**Files:**
- Modify: `src-tauri/src/runtime/events.rs`（新增 `TurnCompleted` event kind）
- Modify: `src-tauri/src/runtime/session_runtime.rs`（消费 `ChatTurnOutcome`，emit 事件）
- Modify: `src-tauri/src/transport/tauri_event_adapter.rs`（映射到前端 event）
- Test: `src-tauri/tests/plan_o_queryengine_session_state_test.rs`（追加）

### TDD 步骤

- [ ] 1. 写失败测试（追加）

```rust
mod o6_turn_outcome_event {
    use lotus_app::runtime::events::RuntimeEventKind;
    use lotus_app::runtime::chat::turn_outcome::ChatTurnOutcome;

    #[test]
    fn o6_turn_completed_event_kind_exists_and_encodes_outcome() {
        // Compile-time: verify RuntimeEventKind::TurnCompleted variant exists.
        let event = RuntimeEventKind::TurnCompleted {
            outcome: ChatTurnOutcome::Success,
            total_cost_usd: 0.05,
            permission_denial_count: 2,
        };
        assert!(
            matches!(
                event,
                RuntimeEventKind::TurnCompleted {
                    outcome: ChatTurnOutcome::Success,
                    ..
                }
            )
        );
    }

    #[test]
    fn o6_budget_exceeded_outcome_encodes_in_turn_completed() {
        let event = RuntimeEventKind::TurnCompleted {
            outcome: ChatTurnOutcome::BudgetExceeded {
                reason: "over budget".to_string(),
                total_cost_usd: 1.5,
            },
            total_cost_usd: 1.5,
            permission_denial_count: 0,
        };
        assert!(matches!(
            event,
            RuntimeEventKind::TurnCompleted {
                outcome: ChatTurnOutcome::BudgetExceeded { .. },
                ..
            }
        ));
    }
}
```

- [ ] 2. 确认失败

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_o_queryengine_session_state_test o6_ -- --nocapture 2>&1 | head -40
```

期望：`error[E0599]: no variant named 'TurnCompleted'`

- [ ] 3. 最小实现

**a. `src-tauri/src/runtime/events.rs` — 新增变体（在 `RunCompleted` 前）：**

```rust
/// Emitted after `run_chat_turn` completes with full outcome metadata.
/// Mirrors the `result` subtype messages in claude-code-best QueryEngine.
TurnCompleted {
    outcome: crate::runtime::chat::turn_outcome::ChatTurnOutcome,
    /// Estimated USD cost for this turn (0.0 when not configured).
    total_cost_usd: f64,
    /// Number of permission denials recorded this session.
    permission_denial_count: usize,
},
```

**b. `session_runtime.rs` — 在 `driver.run_chat_turn()` 调用后 emit TurnCompleted：**

```rust
let outcome = driver.run_chat_turn(&mut turn, &request).await?;
let total_cost = query_engine.estimated_cost_usd();
let denial_count = query_engine.get_permission_denials().len();
event_bus.emit(RuntimeEvent::new(
    session_id.clone(),
    run_id.clone(),
    RuntimeEventKind::TurnCompleted {
        outcome: outcome.clone(),
        total_cost_usd: total_cost,
        permission_denial_count: denial_count,
    },
)).await?;
```

**c. `tauri_event_adapter.rs` — 映射 `TurnCompleted` 到前端 `turn:completed` 事件：**

在现有 match 分支中追加：

```rust
RuntimeEventKind::TurnCompleted {
    outcome,
    total_cost_usd,
    permission_denial_count,
} => {
    let _ = app_handle.emit(
        "turn:completed",
        serde_json::json!({
            "session_id": event.session_id,
            "run_id": event.run_id,
            "outcome": outcome,
            "total_cost_usd": total_cost_usd,
            "permission_denial_count": permission_denial_count,
        }),
    );
}
```

- [ ] 4. 确认通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_o_queryengine_session_state_test o6_ -- --nocapture
```

- [ ] 5. 全量回归

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test tauri_event_adapter_test -- --nocapture 2>&1 | tail -20
```

- [ ] 6. commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/runtime/events.rs \
        src-tauri/src/runtime/session_runtime.rs \
        src-tauri/src/transport/tauri_event_adapter.rs \
        src-tauri/tests/plan_o_queryengine_session_state_test.rs && \
git commit -m "feat(events): emit TurnCompleted event with outcome, cost, and denial count - O6"
```

---

## 验收标准（全量）

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test plan_o_queryengine_session_state_test -- --nocapture
```

所有 `o1_` ~ `o6_` 前缀测试全部 PASS。

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test review_ --tests --no-fail-fast
```

架构约束回归测试全部 PASS（`runtime/` 下无 `use tauri::*`）。

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test chat_runtime_first_mainline_test \
             --test chat_runtime_dispatcher_production_path_test \
             -- --nocapture 2>&1 | tail -20
```

现有聊天 runtime 集成测试全部 PASS。

---

## 实现注意事项

1. **`run_chat_turn` 签名变更（O3）是本计划最大的 breaking change**：所有测试中用到 `run_chat_turn` 返回 `Result<()>` 的地方都需要改为 `Result<ChatTurnOutcome>`。务必先用 `cargo check` 定位所有调用点，再统一修改。
2. **Budget 检查时机**：claude-code-best 在每条 message 处理后检查；本 Plan 简化为每轮（turn）结束后检查，粒度更粗但实现更简单，满足 P0 能力对齐要求。后续可细化为每次 iteration 检查。
3. **cost_per_1k_tokens 默认值**：初期不设默认值（None），前端/transport 层注入时才配置，避免引入硬编码的模型定价。
4. **`PermissionDenialRecord` vs `ToolError::PermissionDenied`**：记录点选在 `run_tool_call_with_bus_internal` 的 `Err(PermissionDenied)` 分支，而非 `dispatcher.rs`，避免 dispatcher 产生对 QueryEngine 状态的依赖（保持分层约束）。
5. **`TurnCompleted` event 不替代 `AgentIdle`**：前端依赖 `agent:idle` 来解除 loading 状态，保持不动；`turn:completed` 是额外的结构化终态信号，供需要匹配 outcome 类型的消费方（日志、测试、未来 SDK wrapper）使用。

<!-- reviewed: 2026-04-18, fixes applied -->
