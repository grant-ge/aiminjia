# 工具执行改进（Plan-N）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 lotus-app 的并发工具执行层补充三项改进：sibling error cascade、`interrupt_behavior()` trait 方法、`context_modifier` 回调。

**Architecture:** N1 在 `ToolRoundDriver::execute_round()` 的并发分支中，当某个工具出错时通过已有 `CancellationToken::SiblingError` 取消兄弟调用；N2 在 `RuntimeTool` trait 上新增 `fn interrupt_behavior() -> InterruptBehavior`，`ToolRoundDriver` 在决定是否级联取消时查询此方法；N3 在 `RuntimeTool` trait 上新增可选 `fn context_modifier()`，dispatcher 在非并发安全工具完成后将 modifier 结果透传给 `ToolDispatchOutcome`，上层 driver 应用于后续 LLM 消���上下文。

**Tech Stack:** Rust, tokio, async_trait, futures

**Worktree branch:** pzc

---

## 背景与对标

### claude-code-best 的实现

`StreamingToolExecutor.ts` 中的三个模式：

**sibling error cascade（第 44–59 行）**
```typescript
// Child of toolUseContext.abortController. Fires when a Bash tool errors
// so sibling subprocesses die immediately instead of running to completion.
private siblingAbortController: AbortController
```
当某工具出错时（第 347 行），`this.hasErrored = true` + `this.siblingAbortController.abort()`，其他并发工具在下次 loop 检查时通过 `getAbortReason()` 返回 `'sibling_error'` 并生成 synthetic error message。

**interruptBehavior（第 233–241 行）**
```typescript
private getToolInterruptBehavior(tool: TrackedTool): 'cancel' | 'block' {
    const definition = findToolByName(this.toolDefinitions, tool.block.name)
    if (!definition?.interruptBehavior) return 'block'
    return definition.interruptBehavior()
}
```
`Tool.ts` 第 416 行：`interruptBehavior?(): 'cancel' | 'block'` —— 默认 `block`（用户中断时不取消工具）。

**contextModifier（Tool.ts 第 329–330 行）**
```typescript
// contextModifier is only honored for tools that aren't concurrency safe.
contextModifier?: (context: ToolUseContext) => ToolUseContext
```
`ToolResult.contextModifier` 返回一个函数，`StreamingToolExecutor` 在收集 results 时累积 `contextModifiers`，完成后在 `getRemainingResults()` 透传给上层。

### lotus-app 当前状态

- `CancellationToken`（`cancellation.rs`）：`SiblingError` variant 已存在，支持 child token cascade，但 `ToolRoundDriver` 的并发 dispatch 用 `join_all` 无 cascade 逻辑。
- `RuntimeTool` trait（`dispatcher.rs`）：无 `interrupt_behavior()` 和 `context_modifier()`。
- `ToolRoundDriver::execute_round()`（`tool_round_driver.rs`）：并发分支（`permitted.len() > 1`）使用 `futures::future::join_all` 并无错误传播到兄弟工具的机制。
- `ToolResult`（`executor.rs`）：无 `context_modifier` 字段。

---

## Task N1 — Sibling Error Cascade

**文件**
- Modify: `src-tauri/src/runtime/chat/tool_round_driver.rs`
- Modify: `src-tauri/src/runtime/chat/tool_round_types.rs`（可选，添加辅助方法）
- Test: `src-tauri/tests/plan_n_sibling_cascade_test.rs`

### 设计

在 `ToolRoundDriver` 中引入一个 `sibling_cancel: CancellationToken`（每次 `execute_round` 新建），是本次 round 内所有并发工具调用共享的子 token。当任意工具返回 `is_error: true`（且工具名含有 bash-class 特征，或 `interrupt_behavior == Cancel`）时，cancel sibling token，其他工具的 `ToolExecutionContext` 持有的 cancel token 被 cascade 触发，工具执行中断并返回 `SiblingError` synthetic result。

由于 `ToolExecutionContext` 已有 `cancellation: CancellationToken`，只需在派发时为每个并发工具的 ctx 绑定 sibling token 的子 token 即可。

### TDD 步骤

**N1-T1: 写失败测试**

创建 `src-tauri/tests/plan_n_sibling_cascade_test.rs`：

```rust
//! Plan-N Task 1: Sibling error cascade 测试
//!
//! cargo test --test plan_n_sibling_cascade_test

use std::sync::{Arc, Mutex};
use std::time::Duration;
use async_trait::async_trait;
use serde_json::{json, Value};
use lotus_app::runtime::chat::tool_round_driver::ToolRoundDriver;
use lotus_app::runtime::chat::tool_round_types::{RuntimeToolCallRequest, ToolRoundResult};
use lotus_app::runtime::event_bus::RuntimeEventBus;
use lotus_app::runtime::identity::IdentityMapping;
use lotus_app::runtime::ids::RunId;
use lotus_app::runtime::query_engine::QueryEngine;
use lotus_app::runtime::state::TurnState;
use lotus_app::runtime::tools::{
    AllowAllPermissionPipeline, ToolDefinition, ToolDispatcher, ToolError,
    ToolExecutionContext, ToolResult,
};
use lotus_app::runtime::tools::dispatcher::RuntimeTool;
use lotus_app::runtime::cancellation::CancellationToken;

fn make_turn() -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id("test-conv");
    TurnState::new(mapping, RunId::new("test-run"), "test".to_string())
}

/// 立即失败的工具
struct FailTool {
    name: String,
}

#[async_trait]
impl RuntimeTool for FailTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(&self.name, "always fails")
    }
    fn is_concurrency_safe(&self, _input: &Value) -> bool { true }
    async fn execute(&self, _input: Value, _ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        Err(ToolError::ExecutionFailed("intentional failure".to_string()))
    }
}

/// 慢速工具（等待取消）
struct SlowTool {
    name: String,
    started: Arc<Mutex<bool>>,
    cancelled: Arc<Mutex<bool>>,
}

#[async_trait]
impl RuntimeTool for SlowTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(&self.name, "slow tool")
    }
    fn is_concurrency_safe(&self, _input: &Value) -> bool { true }
    async fn execute(&self, _input: Value, ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        *self.started.lock().unwrap() = true;
        // 轮询取消信号，最多等 2 秒
        for _ in 0..200 {
            if ctx.cancellation.is_cancelled() {
                *self.cancelled.lock().unwrap() = true;
                return Err(ToolError::ExecutionFailed("cancelled by sibling error".to_string()));
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok(ToolResult::new(&self.name, "slow done", None))
    }
}

// N1-1: 并发执行中，fail_tool 出错后，slow_tool 被级联取消
#[tokio::test]
async fn sibling_error_cascades_to_concurrent_tool() {
    let slow_started = Arc::new(Mutex::new(false));
    let slow_cancelled = Arc::new(Mutex::new(false));

    let fail_tool = Arc::new(FailTool { name: "fail_tool".to_string() });
    let slow_tool = Arc::new(SlowTool {
        name: "slow_tool".to_string(),
        started: slow_started.clone(),
        cancelled: slow_cancelled.clone(),
    });

    let dispatcher = Arc::new(ToolDispatcher::allow_all());
    dispatcher.register(fail_tool);
    dispatcher.register(slow_tool);

    let engine = QueryEngine::with_dispatcher(dispatcher);
    let driver = ToolRoundDriver::new(engine);
    let bus = RuntimeEventBus::new();
    let turn = make_turn();

    let results = driver
        .execute_round(
            &turn,
            &bus,
            vec![
                RuntimeToolCallRequest {
                    tool_call_id: "tc-fail".into(),
                    tool_name: "fail_tool".into(),
                    args: json!({}),
                    purpose: None,
                },
                RuntimeToolCallRequest {
                    tool_call_id: "tc-slow".into(),
                    tool_name: "slow_tool".into(),
                    args: json!({}),
                    purpose: None,
                },
            ],
        )
        .await;

    assert_eq!(results.len(), 2);

    // fail_tool 应该有 error 结果
    let fail_result = &results[0];
    assert!(matches!(fail_result, ToolRoundResult::Ok(o) if o.is_error()),
        "fail_tool should produce an error result");

    // slow_tool 应该被取消
    let slow_result = &results[1];
    assert!(matches!(slow_result, ToolRoundResult::Ok(o) if o.is_error()),
        "slow_tool should be cancelled (sibling error)");

    // slow_tool 的结果内容应包含 sibling 取消标识
    if let ToolRoundResult::Ok(outcome) = slow_result {
        let content = outcome.content();
        assert!(
            content.contains("sibling") || content.contains("cancelled") || content.contains("parallel"),
            "sibling cancel message should mention the reason; got: {}",
            content
        );
    }
}

// N1-2: 单工具执行时无 sibling cascade（向后兼容）
#[tokio::test]
async fn single_tool_no_sibling_cascade() {
    let fail_tool = Arc::new(FailTool { name: "fail_tool".to_string() });
    let dispatcher = Arc::new(ToolDispatcher::allow_all());
    dispatcher.register(fail_tool);

    let engine = QueryEngine::with_dispatcher(dispatcher);
    let driver = ToolRoundDriver::new(engine);
    let bus = RuntimeEventBus::new();
    let turn = make_turn();

    let results = driver
        .execute_round(
            &turn,
            &bus,
            vec![RuntimeToolCallRequest {
                tool_call_id: "tc-solo".into(),
                tool_name: "fail_tool".into(),
                args: json!({}),
                purpose: None,
            }],
        )
        .await;

    assert_eq!(results.len(), 1);
    // 仅有一个结果，是原始错误
    assert!(matches!(&results[0], ToolRoundResult::Ok(o) if o.is_error()));
}

// N1-3: 并发工具都成功时无多余取消
#[tokio::test]
async fn concurrent_success_no_spurious_cancels() {
    struct OkTool { name: String }
    #[async_trait]
    impl RuntimeTool for OkTool {
        fn definition(&self) -> ToolDefinition { ToolDefinition::new(&self.name, "ok") }
        fn is_concurrency_safe(&self, _: &Value) -> bool { true }
        async fn execute(&self, _: Value, _: ToolExecutionContext) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new(&self.name, "ok", None))
        }
    }
    let t1 = Arc::new(OkTool { name: "tool_a".into() });
    let t2 = Arc::new(OkTool { name: "tool_b".into() });
    let dispatcher = Arc::new(ToolDispatcher::allow_all());
    dispatcher.register(t1);
    dispatcher.register(t2);
    let engine = QueryEngine::with_dispatcher(dispatcher);
    let driver = ToolRoundDriver::new(engine);
    let bus = RuntimeEventBus::new();
    let turn = make_turn();

    let results = driver.execute_round(&turn, &bus, vec![
        RuntimeToolCallRequest { tool_call_id: "tc-a".into(), tool_name: "tool_a".into(), args: json!({}), purpose: None },
        RuntimeToolCallRequest { tool_call_id: "tc-b".into(), tool_name: "tool_b".into(), args: json!({}), purpose: None },
    ]).await;

    assert_eq!(results.len(), 2);
    assert!(matches!(&results[0], ToolRoundResult::Ok(o) if !o.is_error()));
    assert!(matches!(&results[1], ToolRoundResult::Ok(o) if !o.is_error()));
}
```

**N1-T2: 确认失败**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test plan_n_sibling_cascade_test 2>&1 | head -30
```
期望：N1-1 中 `slow_tool` 不被取消（当前无 cascade 逻辑），测试失败。

**N1-T3: 最小实现**

修改 `ToolRoundDriver::execute_round()` 的并发分支：

```rust
// 在 execute_round() 的并发分支替换原有 join_all 逻辑：
} else {
    // 创建本次 round 的 sibling cancel token
    // 当任意工具出错时，通过此 token 取消其他工具
    let turn_cancel = turn.cancellation();
    let sibling_cancel = turn_cancel.child_token();

    let futures: Vec<_> = permitted
        .into_iter()
        .map(|(idx, call)| {
            let engine = self.query_engine.clone();
            let turn_clone = turn.clone();
            let bus_clone = bus.clone();
            // 每个工具的 cancel token 是 sibling_cancel 的子 token
            let tool_cancel = sibling_cancel.child_token();
            let sibling_cancel_clone = sibling_cancel.clone();
            async move {
                // 使用工具专属 cancel token（受 sibling cascade 影响）
                // 实现方式：通过 TurnState 覆盖 cancellation 传入 run_tool_call_with_bus
                // QueryEngine::run_tool_call_with_bus 已有签名：
                //   pub async fn run_tool_call_with_bus(
                //       &self, turn: &TurnState, bus: &RuntimeEventBus, call: RuntimeToolCallRequest
                //   ) -> Result<RuntimeToolCallOutcome>
                // 需在 TurnState 上临时替换 cancellation 为 tool_cancel，或在
                // QueryEngine 内部在 build_execution_context 时注入外部 cancel token。
                // 推荐：在 N1 实现时为 run_tool_call_with_bus 添加 cancel 参数重载：
                //   pub async fn run_tool_call_with_bus_cancellable(
                //       &self, turn: &TurnState, bus: &RuntimeEventBus,
                //       call: RuntimeToolCallRequest, cancel: CancellationToken,
                //   ) -> Result<RuntimeToolCallOutcome>
                let outcome = engine
                    .run_tool_call_with_bus_cancellable(
                        &turn_clone,
                        &bus_clone,
                        call,
                        tool_cancel,
                    )
                    .await;
                match outcome {
                    Ok(o) => {
                        // 工具出错时取消兄弟
                        if o.is_error() {
                            sibling_cancel_clone.cancel_with_reason(
                                crate::runtime::cancellation::CancellationReason::SiblingError
                            );
                        }
                        (idx, ToolRoundResult::Ok(o))
                    }
                    Err(e) => {
                        // dispatch 基础设施错误也触发 sibling cancel
                        sibling_cancel_clone.cancel_with_reason(
                            crate::runtime::cancellation::CancellationReason::SiblingError
                        );
                        let content = format!("Error: {}", e);
                        (idx, ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
                            tool_call_id: String::new(),
                            tool_name: String::new(),
                            content,
                            is_error: true,
                            file_meta: None,
                            is_degraded: false,
                            degradation_notice: None,
                            max_result_size_chars: 8_000,
                        }))
                    }
                }
            }
        })
        .collect();

    let concurrent_results = futures::future::join_all(futures).await;
    results.extend(concurrent_results);
}
```

> **注意（N1 方法名）**：`QueryEngine` 当前只有 `run_tool_call_with_bus`（`query_engine.rs` 第 231 行），没有带 cancel 参数的重载。N1 实现时须新增 `run_tool_call_with_bus_cancellable`（接受外部 `CancellationToken`，替换 `build_execution_context` 时的内部 token），或通过 `TurnState::with_cancellation(tool_cancel)` 临时覆盖后传入现有方法。二选一，保持现有调用点不变。

sibling cancel 触发后，`SlowTool` 在检查 `ctx.cancellation.is_cancelled()` 时将感知到取消，从而返回错误。`ToolRoundDriver` 在收集结果时，对已因 sibling cancel 而出错的工具生成 synthetic message（内容包含 "sibling" 关键字以通过测试 N1-1）。

**N1-T4: 确认通过**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test plan_n_sibling_cascade_test -- --nocapture
```

**N1-T5: commit**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/src/runtime/chat/tool_round_driver.rs src-tauri/tests/plan_n_sibling_cascade_test.rs && git commit -m "$(cat <<'EOF'
feat(tool-round): implement sibling error cascade via CancellationToken - N1

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task N2-pre — 在 `CancellationToken` 新增 `child_token_ignoring_reason`

**文件**
- Modify: `src-tauri/src/runtime/cancellation.rs`

**背景**：`Block` 行为的工具需要一种"只被 `SiblingError` 取消、不被 `Interrupt` 取消"的 child token。当前 `child_token()` 会传播所有 cancel reason。

**实现**（在 `cancellation.rs` 中新增方法）：

```rust
/// 返回一个 child token，当 parent 因 `ignored_reason` 取消时不传播，
/// 其他 reason 正常传播。
///
/// 用途：`InterruptBehavior::Block` 工具使用此 token，使其不受
/// `CancellationReason::Interrupt` 影响，但仍受 `SiblingError` 影响。
pub fn child_token_ignoring_reason(&self, ignored_reason: CancellationReason) -> CancellationToken {
    let child = CancellationToken::new();
    let parent = self.clone();
    let child_clone = child.clone();
    tokio::spawn(async move {
        // Poll parent token; only propagate if reason != ignored_reason
        loop {
            if parent.is_cancelled() {
                if parent.reason() != Some(ignored_reason.clone()) {
                    child_clone.cancel_with_reason(
                        parent.reason().unwrap_or(CancellationReason::UserCancel)
                    );
                }
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    });
    child
}
```

**验收**：新增后在 N2-T3 中使用此方法为 `Block` 工具构建 child token。

---

## Task N2 — `interrupt_behavior()` Trait 方法

**文件**
- Modify: `src-tauri/src/runtime/tools/dispatcher.rs`（`RuntimeTool` trait）
- Modify: `src-tauri/src/runtime/chat/tool_round_driver.rs`（查询并应用）
- Test: `src-tauri/tests/plan_n_interrupt_behavior_test.rs`

### 设计

新增 `InterruptBehavior` 枚举和 `RuntimeTool::interrupt_behavior()` 默认方法。在 `ToolRoundDriver::execute_round()` 的并发分支中，当 `turn` 的 cancel token 因 `Interrupt`（用户打断）而取消时，仅对 `interrupt_behavior() == Cancel` 的工具触发 sibling cancel；`Block` 工具继续执行。

```
InterruptBehavior::Cancel → 用户中断时立即停止（BashTool 类）
InterruptBehavior::Block  → 用户中断时等待完成（FileEditTool 等不可中断工具）
```

对标 `StreamingToolExecutor.ts` 第 233–241 行 `getToolInterruptBehavior`。

### TDD 步骤

**N2-T1: 写失败测试**

创建 `src-tauri/tests/plan_n_interrupt_behavior_test.rs`：

```rust
//! Plan-N Task 2: interrupt_behavior() trait 方法测试
//!
//! cargo test --test plan_n_interrupt_behavior_test

use async_trait::async_trait;
use serde_json::Value;
use lotus_app::runtime::tools::{
    ToolDefinition, ToolError, ToolExecutionContext, ToolResult,
};
use lotus_app::runtime::tools::dispatcher::{InterruptBehavior, RuntimeTool};

struct CancelTool;
struct BlockTool;

#[async_trait]
impl RuntimeTool for CancelTool {
    fn definition(&self) -> ToolDefinition { ToolDefinition::new("cancel_tool", "cancellable") }
    fn interrupt_behavior(&self) -> InterruptBehavior { InterruptBehavior::Cancel }
    async fn execute(&self, _: Value, _: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new("cancel_tool", "ok", None))
    }
}

#[async_trait]
impl RuntimeTool for BlockTool {
    fn definition(&self) -> ToolDefinition { ToolDefinition::new("block_tool", "blocking") }
    // 默认 Block，不实现 interrupt_behavior
    async fn execute(&self, _: Value, _: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new("block_tool", "ok", None))
    }
}

// N2-1: 未实现 interrupt_behavior 的工具默认 Block
#[test]
fn default_interrupt_behavior_is_block() {
    let tool = BlockTool;
    assert!(matches!(tool.interrupt_behavior(), InterruptBehavior::Block));
}

// N2-2: Cancel 工具声明 Cancel
#[test]
fn cancel_tool_declares_cancel() {
    let tool = CancelTool;
    assert!(matches!(tool.interrupt_behavior(), InterruptBehavior::Cancel));
}

// N2-3: InterruptBehavior 可被序列化（便于日志）
#[test]
fn interrupt_behavior_debug_format() {
    let cancel = InterruptBehavior::Cancel;
    let block = InterruptBehavior::Block;
    assert!(format!("{:?}", cancel).contains("Cancel"));
    assert!(format!("{:?}", block).contains("Block"));
}

// N2-4: ToolRoundDriver 在 Interrupt cancel 时仅取消 Cancel 工具，Block 工具完成
#[tokio::test]
async fn interrupt_only_cancels_cancel_behavior_tools() {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use lotus_app::runtime::cancellation::{CancellationReason, CancellationToken};
    use lotus_app::runtime::chat::tool_round_driver::ToolRoundDriver;
    use lotus_app::runtime::chat::tool_round_types::{RuntimeToolCallRequest, ToolRoundResult};
    use lotus_app::runtime::event_bus::RuntimeEventBus;
    use lotus_app::runtime::identity::IdentityMapping;
    use lotus_app::runtime::ids::RunId;
    use lotus_app::runtime::query_engine::QueryEngine;
    use lotus_app::runtime::state::TurnState;
    use lotus_app::runtime::tools::{AllowAllPermissionPipeline, ToolDispatcher};
    use serde_json::json;

    // Cancel-behavior 工具：检测 cancel 后立即返回错误
    struct CancelAwareTool { cancelled: Arc<Mutex<bool>> }
    #[async_trait]
    impl RuntimeTool for CancelAwareTool {
        fn definition(&self) -> ToolDefinition { ToolDefinition::new("cancel_aware", "cancel") }
        fn is_concurrency_safe(&self, _: &Value) -> bool { true }
        fn interrupt_behavior(&self) -> InterruptBehavior { InterruptBehavior::Cancel }
        async fn execute(&self, _: Value, ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
            for _ in 0..100 {
                if ctx.cancellation.is_cancelled() {
                    *self.cancelled.lock().unwrap() = true;
                    return Err(ToolError::ExecutionFailed("interrupted".to_string()));
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Ok(ToolResult::new("cancel_aware", "done", None))
        }
    }

    // Block-behavior 工具：忽略 cancel，完成执行
    struct BlockAwareTool { completed: Arc<Mutex<bool>> }
    #[async_trait]
    impl RuntimeTool for BlockAwareTool {
        fn definition(&self) -> ToolDefinition { ToolDefinition::new("block_aware", "block") }
        fn is_concurrency_safe(&self, _: &Value) -> bool { true }
        fn interrupt_behavior(&self) -> InterruptBehavior { InterruptBehavior::Block }
        async fn execute(&self, _: Value, _ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
            // Block 工具不检查 cancel，直接完成
            tokio::time::sleep(Duration::from_millis(50)).await;
            *self.completed.lock().unwrap() = true;
            Ok(ToolResult::new("block_aware", "completed", None))
        }
    }

    let cancel_flag = Arc::new(Mutex::new(false));
    let block_flag = Arc::new(Mutex::new(false));

    let cancel_tool = Arc::new(CancelAwareTool { cancelled: cancel_flag.clone() });
    let block_tool = Arc::new(BlockAwareTool { completed: block_flag.clone() });

    let dispatcher = Arc::new(ToolDispatcher::allow_all());
    dispatcher.register(cancel_tool);
    dispatcher.register(block_tool);

    let engine = QueryEngine::with_dispatcher(dispatcher);
    let driver = ToolRoundDriver::new(engine);
    let bus = RuntimeEventBus::new();

    let mapping = IdentityMapping::from_legacy_conversation_id("test-interrupt");
    let mut turn = TurnState::new(mapping, RunId::new("r1"), "test".to_string());

    // 在工具执行开始后 30ms 触发 Interrupt cancel
    let cancel_token = turn.cancellation().clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        cancel_token.cancel_with_reason(CancellationReason::Interrupt);
    });

    let results = driver.execute_round(
        &turn,
        &bus,
        vec![
            RuntimeToolCallRequest {
                tool_call_id: "tc-cancel".into(),
                tool_name: "cancel_aware".into(),
                args: json!({}),
                purpose: None,
            },
            RuntimeToolCallRequest {
                tool_call_id: "tc-block".into(),
                tool_name: "block_aware".into(),
                args: json!({}),
                purpose: None,
            },
        ],
    ).await;

    assert_eq!(results.len(), 2);

    // Cancel 工具应被取消
    assert!(*cancel_flag.lock().unwrap(), "Cancel tool should have been interrupted");

    // Block 工具应完成
    assert!(*block_flag.lock().unwrap(), "Block tool should have completed despite interrupt");
}
```

**N2-T2: 确认失败**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test plan_n_interrupt_behavior_test 2>&1 | head -30
```

**N2-T3: 最小实现**

在 `src-tauri/src/runtime/tools/dispatcher.rs` 中：

```rust
/// 工具的用户中断行为
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptBehavior {
    /// 中断时立即取消（BashTool 类命令行工具）
    Cancel,
    /// 中断时等待完成（文件编辑类不可中断工具）
    Block,
}

#[async_trait]
pub trait RuntimeTool: Send + Sync {
    // ... 现有方法 ...

    /// 声明此工具在用户中断（新消息到来）时的行为。
    /// 默认 `Block`（等待完成）以保持向后兼容。
    fn interrupt_behavior(&self) -> InterruptBehavior {
        InterruptBehavior::Block
    }
}
```

在 `ToolRoundDriver::execute_round()` 并发分支中，利用 `interrupt_behavior` 决策：当 `turn.cancellation()` 的 reason 为 `Interrupt` 时，仅对 `InterruptBehavior::Cancel` 的工具的 child token 触发取消；`Block` 工具的 child token 不受影响。

实现方式（基于 N2-pre 的 `child_token_ignoring_reason`）：

```rust
// 在构建 tool_cancel token 时区分行为：
let interrupt_behavior = {
    let tools = self.query_engine.dispatcher_ref().tools.read().unwrap();
    tools.get(&call.tool_name)
        .map(|t| t.interrupt_behavior())
        .unwrap_or(InterruptBehavior::Block)
};

// Cancel 工具：受所有 cancel reason 影响（包括 Interrupt）
// Block 工具：只受 SiblingError 影响，不受 Interrupt 影响
let tool_cancel = match interrupt_behavior {
    InterruptBehavior::Cancel => sibling_cancel.child_token(),
    InterruptBehavior::Block => sibling_cancel.child_token_ignoring_reason(
        crate::runtime::cancellation::CancellationReason::Interrupt
    ),
};
```

注：`sibling_cancel` 本身是 `turn_cancel.child_token()`，即 `sibling_cancel` 会因 `turn_cancel` 触发 `Interrupt` 而被 cancel。对 `Cancel` 工具，其 child token 继承此传播；对 `Block` 工具，通过 `child_token_ignoring_reason(Interrupt)` 屏蔽 `Interrupt` 传播，只保留 `SiblingError` 的传播。

**N2-T4: 确认通过**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test plan_n_interrupt_behavior_test -- --nocapture
```

**N2-T5: commit**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/src/runtime/tools/dispatcher.rs src-tauri/src/runtime/chat/tool_round_driver.rs src-tauri/tests/plan_n_interrupt_behavior_test.rs && git commit -m "$(cat <<'EOF'
feat(tool-round): add interrupt_behavior() trait method and selective interrupt - N2

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task N3 — `context_modifier` 回调

**文件**
- Modify: `src-tauri/src/runtime/tools/executor.rs`（`ToolResult` 添加字段）
- Modify: `src-tauri/src/runtime/tools/dispatcher.rs`（`RuntimeTool` trait 新增方法，`ToolDispatchOutcome` 透传）
- Modify: `src-tauri/src/runtime/chat/tool_round_driver.rs` 或 `tool_round_types.rs`（收集 modifier）
- Test: `src-tauri/tests/plan_n_context_modifier_test.rs`

### 设计

`context_modifier` 的用途：工具执行后，允许工具向后续 LLM 请求注入额外上下文（如"文件已被修改，注意以下 diff…"）。只对非并发安全（`is_concurrency_safe = false`）的工具生效（对标 Tool.ts 第 329 行注释："contextModifier is only honored for tools that aren't concurrency safe"）。

在 lotus-app 中，工具直接声明"完成后想注入什么 context message"，返回 `Option<serde_json::Value>`（一条额外的 user message）。这避免了返回闭包的跨线程生命周期复杂性，语义更清晰：`Some(value)` 表示工具完成后要注入此消息，`None` 表示不注入。`ToolRoundDriver` 在 round 结束后收集所有非 None 的 `context_modifier_message` 并注入到 `state.messages`，再由 `chat_turn_driver.rs` 传给下一次 LLM 调用。

类型定义（无闭包，直接返回 Value）：
```rust
fn context_modifier(&self) -> Option<serde_json::Value> {
    None
}
```

### TDD 步骤

**N3-T1: 写失败测试**

创建 `src-tauri/tests/plan_n_context_modifier_test.rs`：

```rust
//! Plan-N Task 3: context_modifier 回调测试
//!
//! cargo test --test plan_n_context_modifier_test

use std::sync::Arc;
use async_trait::async_trait;
use serde_json::{json, Value};
use lotus_app::runtime::tools::{
    ToolDefinition, ToolDispatcher, ToolError, ToolExecutionContext, ToolResult,
};
use lotus_app::runtime::tools::dispatcher::{RuntimeTool, ToolDispatchOutcome};

// 带 context_modifier 的工具
struct ModifyingTool;

#[async_trait]
impl RuntimeTool for ModifyingTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("modifying_tool", "modifies context")
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        false // 非并发安全 — context_modifier 生效
    }
    fn context_modifier(&self) -> Option<serde_json::Value> {
        Some(json!({
            "role": "user",
            "content": "<context-update>File was modified by tool.</context-update>"
        }))
    }
    async fn execute(&self, _: Value, _: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new("modifying_tool", "file written", None))
    }
}

// 无 context_modifier 的普通工具
struct PlainTool;

#[async_trait]
impl RuntimeTool for PlainTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("plain_tool", "no modifier")
    }
    async fn execute(&self, _: Value, _: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new("plain_tool", "ok", None))
    }
}

// N3-1: 未实现 context_modifier 的工具返回 None
#[test]
fn default_context_modifier_is_none() {
    let tool = PlainTool;
    assert!(tool.context_modifier().is_none());
}

// N3-2: ModifyingTool 的 context_modifier 返回 Some
#[test]
fn modifying_tool_context_modifier_returns_some() {
    let tool = ModifyingTool;
    let msg = tool.context_modifier();
    assert!(msg.is_some());
    let msg = msg.unwrap();
    assert_eq!(msg.get("role").and_then(|v| v.as_str()), Some("user"));
}

// N3-3: ToolDispatchOutcome::Completed 包含 context_modifier_message
#[tokio::test]
async fn dispatch_outcome_includes_context_modifier_message() {
    let tool = Arc::new(ModifyingTool);
    let dispatcher = Arc::new(ToolDispatcher::allow_all());
    dispatcher.register(tool);

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
    let outcome = dispatcher.dispatch("modifying_tool", json!({}), ctx).await.unwrap();

    match outcome {
        ToolDispatchOutcome::Completed { context_modifier_message, .. } => {
            assert!(
                context_modifier_message.is_some(),
                "non-concurrency-safe tool's context_modifier should be surfaced"
            );
            let msg = context_modifier_message.unwrap();
            assert_eq!(msg.get("role").and_then(|v| v.as_str()), Some("user"));
        }
        _ => panic!("expected Completed"),
    }
}

// N3-4: 并发安全工具的 context_modifier 不被采用（返回 None）
#[tokio::test]
async fn concurrent_safe_tool_modifier_ignored() {
    struct ConcurrentModifyingTool;
    #[async_trait]
    impl RuntimeTool for ConcurrentModifyingTool {
        fn definition(&self) -> ToolDefinition { ToolDefinition::new("conc_mod", "concurrent") }
        fn is_concurrency_safe(&self, _: &Value) -> bool { true } // 并发安全
        fn context_modifier(&self) -> Option<serde_json::Value> {
            Some(json!({"role": "user", "content": "should not appear"}))
        }
        async fn execute(&self, _: Value, _: ToolExecutionContext) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new("conc_mod", "ok", None))
        }
    }

    let tool = Arc::new(ConcurrentModifyingTool);
    let dispatcher = Arc::new(ToolDispatcher::allow_all());
    dispatcher.register(tool);

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
    let outcome = dispatcher.dispatch("conc_mod", json!({}), ctx).await.unwrap();

    match outcome {
        ToolDispatchOutcome::Completed { context_modifier_message, .. } => {
            assert!(
                context_modifier_message.is_none(),
                "concurrent-safe tool's context_modifier should be ignored"
            );
        }
        _ => panic!("expected Completed"),
    }
}

// N3-5: 无 context_modifier 的工具返回 None
#[tokio::test]
async fn plain_tool_no_context_modifier_in_outcome() {
    let tool = Arc::new(PlainTool);
    let dispatcher = Arc::new(ToolDispatcher::allow_all());
    dispatcher.register(tool);

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
    let outcome = dispatcher.dispatch("plain_tool", json!({}), ctx).await.unwrap();

    match outcome {
        ToolDispatchOutcome::Completed { context_modifier_message, .. } => {
            assert!(context_modifier_message.is_none());
        }
        _ => panic!("expected Completed"),
    }
}
```

**N3-T2: 确认失败**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test plan_n_context_modifier_test 2>&1 | head -30
```

**N3-T3: 最小实现**

1. `RuntimeTool` trait（`dispatcher.rs`）新增默认方法：

```rust
/// 工具完成后注入后续上下文的可选消息（可选）。
/// 仅对非并发安全工具（`is_concurrency_safe() == false`）生效。
/// 返回的 Value 应为格式 `{"role": "user", "content": "..."}` 的消息。
/// 直接返回 Value 而非闭包，避免跨线程生命周期复杂性。
fn context_modifier(&self) -> Option<serde_json::Value> {
    None
}
```

2. `ToolDispatchOutcome::Completed` 新增字段：

```rust
pub enum ToolDispatchOutcome {
    Completed {
        result: ToolResult,
        event_names: Vec<String>,
        max_result_size_chars: usize,
        prevent_continuation: bool,       // 来自 M4
        stop_reason: Option<String>,      // 来自 M4
        /// 非并发安全工具的上下文 modifier 结果（已求值）
        context_modifier_message: Option<serde_json::Value>,
    },
    AskRequired(PermissionDecision),
}
```

3. `dispatch()` 成功路径在构建 `Completed` 时获取 modifier 值：

```rust
let context_modifier_message = if !tool.is_concurrency_safe(&input) {
    tool.context_modifier()
} else {
    None
};
```

4. 更新所有 `ToolDispatchOutcome::Completed` 解构点（`chat_turn_driver.rs`、`query_engine.rs`、集成测试等），补充 `context_modifier_message: _`。

5. 在 `chat_turn_driver.rs` 的 `collect_results` 处，将 `context_modifier_message` 收集到 `history_batch` 中注入为额外 user message（在 tool result message 之后）。

**N3-T4: 确认通过**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test plan_n_context_modifier_test -- --nocapture
```

**N3-T5: commit**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/src/runtime/tools/dispatcher.rs src-tauri/src/runtime/tools/executor.rs src-tauri/src/runtime/chat/ src-tauri/tests/plan_n_context_modifier_test.rs && git commit -m "$(cat <<'EOF'
feat(tool-round): add context_modifier callback to RuntimeTool trait - N3

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task N4 — review 回归测试（架构约束）

**文件**
- Test: `src-tauri/tests/plan_n_review_tool_execution_test.rs`

```rust
//! Plan-N Task 4: 工具执行改进架构约束回归测试
//!
//! cargo test --test plan_n_review_tool_execution_test

// N4-1: InterruptBehavior 默认值是 Block（保持向后兼容）
#[test]
fn review_default_interrupt_behavior_is_block() {
    use lotus_app::runtime::tools::dispatcher::{InterruptBehavior, RuntimeTool};

    struct MinimalTool;
    #[async_trait::async_trait]
    impl RuntimeTool for MinimalTool {
        fn definition(&self) -> lotus_app::runtime::tools::ToolDefinition {
            lotus_app::runtime::tools::ToolDefinition::new("minimal", "minimal tool")
        }
        async fn execute(
            &self,
            _: serde_json::Value,
            _: lotus_app::runtime::tools::ToolExecutionContext,
        ) -> Result<lotus_app::runtime::tools::ToolResult, lotus_app::runtime::tools::ToolError> {
            Ok(lotus_app::runtime::tools::ToolResult::new("minimal", "ok", None))
        }
    }

    let tool = MinimalTool;
    assert!(
        matches!(tool.interrupt_behavior(), InterruptBehavior::Block),
        "default interrupt_behavior must be Block to maintain backward compatibility"
    );
}

// N4-2: context_modifier 默认返回 None（保持向后兼容）
#[test]
fn review_default_context_modifier_is_none() {
    use lotus_app::runtime::tools::dispatcher::RuntimeTool;

    struct MinimalTool2;
    #[async_trait::async_trait]
    impl RuntimeTool for MinimalTool2 {
        fn definition(&self) -> lotus_app::runtime::tools::ToolDefinition {
            lotus_app::runtime::tools::ToolDefinition::new("minimal2", "minimal tool 2")
        }
        async fn execute(
            &self,
            _: serde_json::Value,
            _: lotus_app::runtime::tools::ToolExecutionContext,
        ) -> Result<lotus_app::runtime::tools::ToolResult, lotus_app::runtime::tools::ToolError> {
            Ok(lotus_app::runtime::tools::ToolResult::new("minimal2", "ok", None))
        }
    }

    let tool = MinimalTool2;
    assert!(
        tool.context_modifier().is_none(),
        "default context_modifier must be None to maintain backward compatibility"
    );}

// N4-3: SiblingError 不传播到 turn 级别的 CancellationToken
#[test]
fn review_sibling_error_does_not_cancel_turn() {
    use lotus_app::runtime::cancellation::{CancellationReason, CancellationToken};

    let turn_cancel = CancellationToken::new();
    let sibling_cancel = turn_cancel.child_token();

    sibling_cancel.cancel_with_reason(CancellationReason::SiblingError);

    // sibling cancel 不应传播到 turn
    assert!(
        !turn_cancel.is_cancelled(),
        "SiblingError in sibling token must NOT propagate to the turn-level token"
    );
    assert_eq!(
        sibling_cancel.reason(),
        Some(CancellationReason::SiblingError)
    );
}

// N4-4: ToolRoundDriver 不依赖 tauri::*
#[test]
fn review_tool_round_driver_no_tauri() {
    // 编译通过即证明
    let _ = std::mem::size_of::<lotus_app::runtime::chat::tool_round_driver::ToolRoundDriver>();
}
```

**确认通过**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

**commit**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/tests/plan_n_review_tool_execution_test.rs && git commit -m "$(cat <<'EOF'
test(tool-round): add architecture constraint regression tests - N4

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## 实现注意事项

### N1（Sibling Cascade）
- `ToolRoundDriver` 需要访问 `ToolDispatcher` 以查询 `interrupt_behavior`（当前通过 `QueryEngine` 间接访问）。若 `QueryEngine` 不暴露 dispatcher，可将 `InterruptBehavior` 结果记录在工具 dispatch 后的 `ToolDispatchOutcome` 中，供 round driver 决策使用。
- Sibling cancel token 的生命周期与 `execute_round` 调用相同，每轮独立。
- 仅在工具 `is_error: true` 时触发 sibling cancel（与 StreamingToolExecutor 一致）；工具正常完成不触发。
- **重要**：`sibling_cancel.child_token()` 的 cancel 不得传播到 `turn.cancellation()`（即 sibling cancel token 是 turn token 的子 token，而非父子关系反转）。N4-3 已验证此约束。

### N2（InterruptBehavior）
- 当前 `CancellationToken` 已区分 `Interrupt` 和 `SiblingError`，N2 利用此 reason 分类。
- `Block` 工具需要自己的 cancel token，该 token 只受 `SiblingError` 影响，不受 `Interrupt` 影响。通过 N2-pre 新增的 `child_token_ignoring_reason(Interrupt)` 实现：`sibling_cancel.child_token_ignoring_reason(CancellationReason::Interrupt)`。

### N3（ContextModifier）
- `context_modifier` 直接返回 `Option<serde_json::Value>`（不返回闭包），在 `dispatch()` 返回 `Completed` 时直接调用 `tool.context_modifier()`，将结果存入 `ToolDispatchOutcome::Completed::context_modifier_message`。这避免了闭包跨线程的生命周期复杂性。
- `chat_turn_driver.rs` 中消费 `context_modifier_message`：在 `history_batch` 中 tool result message 之后注入，使下一次 LLM 调用能感知上下文变化。

<!-- reviewed: 2026-04-18, fixes applied -->
