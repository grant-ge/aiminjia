# 架构闭环 S1-S3 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 完成架构闭环前三期：S1 取消模型统一、S2 权限 bypass 消除、S3 P1 tool round ownership 收尾。每期可独立编译、测试、合并。

**Architecture:** S1 统一所有生产路径的 CancellationToken 来源到 TurnState/AgentContext（消除 4 处孤立 `new()`）；S2 消除 `allow_all()` bypass，统一到 StorePolicyPipeline；S3 沿用已有 P1 计划 Task 4，让 TauriLegacyTurnExecutor 实现 run_llm_step。

**Tech Stack:** Rust, Tauri v2, tokio, async_trait

**Design Spec:** `docs/superpowers/specs/2026-04-15-architecture-closure-phased-design.md`

---

## 文件结构

| 文件 | 变更类型 | 职责 |
|------|----------|------|
| `src-tauri/src/plugin/context.rs` | 修改 | PluginContext 新增 `cancel_token` 字段 |
| `src-tauri/src/plugin/registry.rs` | 修改 | S1: execute() 透传 token；S2: legacy 回退用 StorePolicyPipeline |
| `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` | 修改 | S1: PluginContext 构造处填入 cancel_token |
| `src-tauri/src/llm/tool_executor/python.rs` | 修改 | S1: 切换到 execute_for_run_with_cancel |
| `src-tauri/src/llm/sub_agent.rs` | 修改 | S1: execute() 调用处传入 cancel_token |
| `src-tauri/src/runtime/tools/dispatcher.rs` | 修改 | S2: allow_all() 标记 #[cfg(test)] |
| `src-tauri/tests/` | 修改/新增 | S1: cancel 透传测试；S2: 权限回归测试 |

---

# S1：取消模型统一

## Task 1: PluginContext 新增 cancel_token 字段

**Files:**
- Modify: `src-tauri/src/plugin/context.rs:74-102`

- [ ] **Step 1: 在 PluginContext struct 中新增字段**

在 `src-tauri/src/plugin/context.rs` 的 `PluginContext` struct 末尾（line 101 `authorized_workspace` 之后）添加：

```rust
    /// Cancellation token for the current turn/run. Propagated to tool
    /// executors so that upstream cancel signals reach long-running operations.
    pub cancel_token: Option<crate::runtime::cancellation::CancellationToken>,
```

- [ ] **Step 2: 修复所有 PluginContext 构造点的编译错误**

运行编译查找所有缺失字段的构造点：

Run: `cd src-tauri && cargo check 2>&1 | grep "missing field"`
Expected: 多个错误，列出所有需要补 `cancel_token` 的位置

对每个构造点添加 `cancel_token: None`（默认值），除了 `chat_runtime_impl.rs` 的两处（Task 2 会填入真实 token）。

- [ ] **Step 3: 编译验证**

Run: `cd src-tauri && cargo check 2>&1 | grep "^error"`
Expected: 无 error

- [ ] **Step 4: Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/src/plugin/context.rs
git commit -m "feat(S1): add cancel_token field to PluginContext"
```

---

## Task 2: chat_runtime_impl 填入真实 cancel_token

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:1081,1812,2768`

- [ ] **Step 1: line 1081 — AgentContext 的 cancel_token 已经是真实 token**

确认 line 1081 的 `cancel_token` 在 AgentContext 中通过 `cancel_rx` 被正确 cancel。这个 token 是 S1 的 canonical source，不需要修改。记录：`agent_ctx.cancel_token` 是本 turn 的唯一 cancel source。

- [ ] **Step 2: line ~1812 — precompute auto_load PluginContext 填入 cancel_token**

找到 `chat_runtime_impl.rs` 中 precompute 阶段构造 `PluginContext` 的位置（约 line 1812），将 `cancel_token: None` 改为：

```rust
cancel_token: Some(agent_ctx.cancel_token.clone()),
```

- [ ] **Step 3: line ~2768 — tool round PluginContext 填入 cancel_token**

找到 `chat_runtime_impl.rs` 中 tool round 入口构造 `PluginContext` 的位置（约 line 2768），同样改为：

```rust
cancel_token: Some(agent_ctx.cancel_token.clone()),
```

- [ ] **Step 4: 编译验证**

Run: `cd src-tauri && cargo check 2>&1 | grep "^error"`
Expected: 无 error

- [ ] **Step 5: Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs
git commit -m "feat(S1): propagate cancel_token through PluginContext in chat_runtime_impl"
```

---

## Task 3: registry.execute() 透传 cancel_token

**Files:**
- Modify: `src-tauri/src/plugin/registry.rs:303-310,345-353`

- [ ] **Step 1: line 308 — RuntimeTool 路径用 ctx.cancel_token**

将 `registry.rs` line 303-310 的 `ToolExecutionContext::new(...)` 调用中的 `CancellationToken::new()` 替换为：

```rust
let exec_ctx = crate::runtime::tools::ToolExecutionContext::new(
    ctx.session_id.clone(),
    run_id,
    ctx.agent_id.clone(),
    format!("tool-{}", name),
    ctx.cancel_token.clone().unwrap_or_default(),  // was: CancellationToken::new()
)
.with_capability(capability);
```

- [ ] **Step 2: line 352 — legacy ToolPlugin 路径用 ctx.cancel_token**

将 `registry.rs` line 345-353 的 `ToolExecutionContext::new(...)` 中的 `CancellationToken::new()` 替换为：

```rust
let runtime_ctx = crate::runtime::tools::ToolExecutionContext::new(
    ctx.session_id.clone(),
    ctx.run_id.clone().unwrap_or_else(|| {
        crate::runtime::ids::RunId::new(format!("run-{}", ctx.conversation_id))
    }),
    ctx.agent_id.clone(),
    format!("tool-{}", name),
    ctx.cancel_token.clone().unwrap_or_default(),  // was: CancellationToken::new()
);
```

- [ ] **Step 3: 编译验证**

Run: `cd src-tauri && cargo check 2>&1 | grep "^error"`
Expected: 无 error

- [ ] **Step 4: Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/src/plugin/registry.rs
git commit -m "feat(S1): registry.execute() propagates cancel_token from PluginContext"
```

---

## Task 4: python executor 切换到 cancel-aware 路径

**Files:**
- Modify: `src-tauri/src/llm/tool_executor/python.rs:281-295`

- [ ] **Step 1: 将 execute_for_run 替换为 execute_for_run_with_cancel**

在 `python.rs` 约 line 285-288，将：

```rust
ctx.session_manager
    .execute_for_run(run_id, &final_code, timeout, &sandbox)
    .await?
```

替换为：

```rust
ctx.session_manager
    .execute_for_run_with_cancel(
        run_id,
        &final_code,
        timeout,
        &sandbox,
        ctx.cancel_token.clone(),
    )
    .await?
```

- [ ] **Step 2: 同样处理非 analysis 模式的 execute 调用**

检查同一函数中是否有其他 `session_manager.execute(...)` 调用（非 run-scoped），如果有，同样切换到 cancel-aware 变体（如果存在 `execute_with_cancel`）。如果没有 cancel-aware 变体，保持原样并添加 `// TODO(S1): add cancel-aware variant` 注释。

- [ ] **Step 3: 编译验证**

Run: `cd src-tauri && cargo check 2>&1 | grep "^error"`
Expected: 无 error

- [ ] **Step 4: Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/src/llm/tool_executor/python.rs
git commit -m "feat(S1): python executor uses cancel-aware execute_for_run_with_cancel"
```

---

## Task 5: sub_agent.rs 透传 cancel_token

**Files:**
- Modify: `src-tauri/src/llm/sub_agent.rs:259`

- [ ] **Step 1: 确认 sub_agent 的 PluginContext 构造处已有 cancel_token**

读取 `sub_agent.rs` 中构造 `sub_plugin_ctx` 的代码，确认 `cancel_token` 字段被正确从上游传入。如果上游 `PluginContext` 已有 `cancel_token`，则 `sub_plugin_ctx` 应 clone 它：

```rust
cancel_token: parent_ctx.cancel_token.clone(),
```

- [ ] **Step 2: 编译验证**

Run: `cd src-tauri && cargo check 2>&1 | grep "^error"`
Expected: 无 error

- [ ] **Step 3: Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/src/llm/sub_agent.rs
git commit -m "feat(S1): sub_agent propagates cancel_token to sub_plugin_ctx"
```

---

## Task 6: S1 验证 — 生产路径 CancellationToken::new() 归零

**Files:**
- No code changes, verification only

- [ ] **Step 1: grep 验证生产路径无孤立 new()**

Run: `cd src-tauri && grep -rn "CancellationToken::new()" src/ --include="*.rs" | grep -v "test\|for_test\|#\[cfg(test)\]\|tests/"`
Expected: 只剩 `runtime/cancellation.rs:10`（CancellationToken 的 `new()` 定义本身）和 `runtime/state.rs:27`（TurnState::new 的合法构造）。不应有其他生产路径调用。

- [ ] **Step 2: 全量 review_ 回归测试**

Run: `cd src-tauri && cargo test review_ --tests --no-fail-fast -- --nocapture`
Expected: 全绿

- [ ] **Step 3: Rust 全量测试**

Run: `cd src-tauri && cargo test`
Expected: 全绿

- [ ] **Step 4: 如有残余，修复并 commit**

---

## Task 7: S1 新增 cancel 透传回归测试

**Files:**
- Create: `src-tauri/tests/cancel_token_propagation_test.rs`

- [ ] **Step 1: 写测试 — registry.execute() 透传 cancelled token 到 RuntimeTool**

创建文件 `src-tauri/tests/cancel_token_propagation_test.rs`：

```rust
//! Verify that a cancelled CancellationToken propagated through PluginContext
//! reaches the RuntimeTool via ToolExecutionContext.

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use serde_json::json;
use async_trait::async_trait;

use lotus_tauri::runtime::cancellation::CancellationToken;
use lotus_tauri::runtime::tools::{RuntimeTool, ToolExecutionContext};
use lotus_tauri::runtime::tools::definition::ToolDefinition;
use lotus_tauri::runtime::tools::executor::{ToolResult, ToolError};
use lotus_tauri::plugin::registry::ToolRegistry;

/// Spy tool that records whether the received CancellationToken was cancelled.
struct CancelSpyTool {
    saw_cancelled: Arc<AtomicBool>,
}

#[async_trait]
impl RuntimeTool for CancelSpyTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("cancel_spy", "test tool")
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        self.saw_cancelled.store(
            ctx.cancellation.is_cancelled(),
            Ordering::SeqCst,
        );
        Ok(ToolResult::new("cancel_spy", "ok", None))
    }
}

#[tokio::test]
async fn registry_execute_propagates_cancelled_token() {
    let saw_cancelled = Arc::new(AtomicBool::new(false));
    let spy = Arc::new(CancelSpyTool { saw_cancelled: saw_cancelled.clone() });

    let registry = ToolRegistry::new();
    registry.register_runtime(spy).await;

    // Create a PluginContext with a pre-cancelled token.
    // PluginContext requires many fields — construct with defaults where possible.
    // The key field is cancel_token: Some(cancelled_token).
    let token = CancellationToken::new();
    token.cancel();

    // Build minimal PluginContext (adjust fields to compile — most can be
    // dummy/default values since CancelSpyTool doesn't use them).
    // NOTE: The exact struct literal depends on post-Task-1 field set.
    // If this doesn't compile, add the minimum required dummy values.
    let ctx = lotus_tauri::plugin::context::PluginContext {
        storage: Arc::new(lotus_tauri::storage::AppStorage::new_temp().unwrap()),
        file_manager: Arc::new(lotus_tauri::storage::FileManager::new_temp().unwrap()),
        workspace_path: std::path::PathBuf::from("/tmp/test"),
        conversation_id: "test-conv".into(),
        session_id: lotus_tauri::runtime::ids::SessionId::new("test-session".into()),
        run_id: None,
        agent_id: None,
        tavily_api_key: None,
        bocha_api_key: None,
        app_handle: None,
        session_manager: Arc::new(lotus_tauri::python::session::PythonSessionManager::new()),
        auth_manager: None,
        connector_engine: None,
        use_cloud: false,
        model: "test".into(),
        gateway: None,
        tool_registry: None,
        app_settings: None,
        agent_runtime: None,
        event_bus: None,
        authorized_workspace: None,
        cancel_token: Some(token),
    };

    let result = registry.execute("cancel_spy", &ctx, json!({})).await;
    assert!(result.is_ok(), "tool execution should succeed");
    assert!(
        saw_cancelled.load(Ordering::SeqCst),
        "RuntimeTool should see cancelled token via ToolExecutionContext"
    );
}
```

注意：`AppStorage::new_temp()` 和 `FileManager::new_temp()` 等测试辅助方法可能不存在。如果编译失败，根据错误信息调整构造方式——核心断言逻辑不变，只是 PluginContext 的 dummy 字段需要适配。

- [ ] **Step 2: 跑测试验证通过**

Run: `cd src-tauri && cargo test --test cancel_token_propagation_test -- --nocapture`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/tests/cancel_token_propagation_test.rs
git commit -m "test(S1): cancel token propagation regression test"
```

---

# S2：权限 bypass 消除

## Task 8: registry.execute() legacy 回退用 StorePolicyPipeline

**Files:**
- Modify: `src-tauri/src/plugin/registry.rs:340`

- [ ] **Step 1: 写失败测试 — legacy tool 经 execute() 时 unknown scope 被 deny**

创建 `src-tauri/tests/permission_bypass_elimination_test.rs`：

```rust
//! Verify that legacy tools dispatched via registry.execute() go through
//! StorePolicyPipeline (or CapabilityPermissionPipeline), NOT AllowAllPermissionPipeline.
//! A tool declaring an unknown capability_scope must be denied.

use std::sync::Arc;
use serde_json::{json, Value};
use async_trait::async_trait;

use lotus_tauri::plugin::tool_trait::{ToolPlugin, ToolOutput};
use lotus_tauri::plugin::registry::ToolRegistry;
use lotus_tauri::runtime::store::permission_store::PermissionStore;

/// Fake legacy tool that declares an unknown capability scope.
struct UnknownScopeLegacyTool;

#[async_trait]
impl ToolPlugin for UnknownScopeLegacyTool {
    fn name(&self) -> &str { "unknown_scope_tool" }
    fn description(&self) -> &str { "test tool with unknown scope" }
    fn parameters_schema(&self) -> Value { json!({}) }

    // The key: this tool declares a scope that no pipeline recognizes
    fn capability_scope(&self) -> Vec<String> {
        vec!["custom:nonexistent".to_string()]
    }

    async fn execute(
        &self,
        _ctx: &lotus_tauri::plugin::context::PluginContext,
        _input: Value,
    ) -> Result<ToolOutput, Box<dyn std::error::Error + Send + Sync>> {
        Ok(ToolOutput::success("should not reach here"))
    }
}

#[tokio::test]
async fn registry_execute_legacy_denies_unknown_scope() {
    let registry = ToolRegistry::new();

    // Inject an empty PermissionStore so StorePolicyPipeline is used
    let store = Arc::new(PermissionStore::in_memory());
    registry.set_permission_store(store).await;

    // Register the legacy tool
    registry.register(Arc::new(UnknownScopeLegacyTool), "test").await;

    // Build a minimal PluginContext (same pattern as cancel test)
    let ctx = lotus_tauri::plugin::context::PluginContext {
        storage: Arc::new(lotus_tauri::storage::AppStorage::new_temp().unwrap()),
        file_manager: Arc::new(lotus_tauri::storage::FileManager::new_temp().unwrap()),
        workspace_path: std::path::PathBuf::from("/tmp/test"),
        conversation_id: "test-conv".into(),
        session_id: lotus_tauri::runtime::ids::SessionId::new("test-session".into()),
        run_id: None,
        agent_id: None,
        tavily_api_key: None,
        bocha_api_key: None,
        app_handle: None,
        session_manager: Arc::new(lotus_tauri::python::session::PythonSessionManager::new()),
        auth_manager: None,
        connector_engine: None,
        use_cloud: false,
        model: "test".into(),
        gateway: None,
        tool_registry: None,
        app_settings: None,
        agent_runtime: None,
        event_bus: None,
        authorized_workspace: None,
        cancel_token: None,
    };

    let result = registry.execute("unknown_scope_tool", &ctx, json!({})).await;
    assert!(
        result.is_err(),
        "Legacy tool with unknown capability scope should be denied, got: {:?}",
        result
    );
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("Unknown capability scope") || err_msg.contains("Permission denied"),
        "Error should mention unknown scope or permission denied, got: {}",
        err_msg
    );
}
```

注意：与 Task 7 同样的约束 — PluginContext dummy 字段需要适配编译。`ToolPlugin` trait 的 `capability_scope()` 方法可能没有默认实现或方法名不同，需要根据实际 trait 定义调整。如果 `ToolPlugin` 没有 `capability_scope()` 方法（scope 由 `ToolDefinition` 而非 plugin 声明），则需要通过 `LegacyToolAdapter` 的 `definition()` 来设置 scope。

- [ ] **Step 2: 跑测试确认它在当前代码下 PASS（因为 allow_all 放行了）**

Run: `cd src-tauri && cargo test --test <test_file> -- registry_execute_legacy_denies_unknown_scope --nocapture`
Expected: FAIL — 当前 `allow_all()` 放行了 unknown scope，测试应期望 deny 但实际得到 allow。

注意：如果测试框架不方便构造完整的 legacy ToolPlugin + PluginContext，可以改为在 `registry.rs` 中写 `#[cfg(test)] mod tests` 的单元测试，利用模块内部可见性直接测试 `execute()` 路径。

- [ ] **Step 3: 替换 allow_all() 为 StorePolicyPipeline**

在 `registry.rs` line 340 替换：

```rust
// BEFORE:
let dispatcher = ToolDispatcher::allow_all();

// AFTER:
let pipeline: Arc<dyn PermissionPipeline> = match self.permission_store.read().await.as_ref() {
    Some(store) => Arc::new(StorePolicyPipeline::new(store.clone())),
    None => Arc::new(CapabilityPermissionPipeline),
};
let dispatcher = ToolDispatcher::new(pipeline);
```

需要在文件头部添加 `use crate::runtime::store::permission_store::StorePolicyPipeline;`（如果尚未导入）和 `use crate::runtime::tools::permission::CapabilityPermissionPipeline;`。

这与同一文件 line 313-316（RuntimeTool 路径）和 line 380-384（`to_runtime_dispatcher`）已有的模式完全一致。

- [ ] **Step 4: 跑测试确认变绿**

Run: `cd src-tauri && cargo test --test <test_file> -- registry_execute_legacy_denies_unknown_scope --nocapture`
Expected: PASS

- [ ] **Step 5: 编译验证**

Run: `cd src-tauri && cargo check 2>&1 | grep "^error"`
Expected: 无 error

- [ ] **Step 6: Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/src/plugin/registry.rs src-tauri/tests/
git commit -m "feat(S2): registry.execute() legacy path uses StorePolicyPipeline instead of allow_all"
```

---

## Task 9: allow_all() 限制为 test-only

**Files:**
- Modify: `src-tauri/src/runtime/tools/dispatcher.rs:41-43`

- [ ] **Step 1: 标记 allow_all() 为 cfg(test)**

```rust
#[cfg(test)]
pub fn allow_all() -> Self {
    Self::new(Arc::new(AllowAllPermissionPipeline))
}
```

- [ ] **Step 2: 编译验证**

Run: `cd src-tauri && cargo check 2>&1 | grep "^error"`
Expected: 无 error（如果生产代码还在调用 `allow_all()` 则会报错，说明 Task 8 的替换不完整）

- [ ] **Step 3: 如果有报错，修复残留调用**

检查报错位置，全部替换为 StorePolicyPipeline 模式。

- [ ] **Step 4: 跑全量回归**

Run: `cd src-tauri && cargo test review_ --tests --no-fail-fast -- --nocapture`
Expected: 全绿

- [ ] **Step 5: Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/src/runtime/tools/dispatcher.rs
git commit -m "feat(S2): restrict allow_all() to #[cfg(test)], prevent production bypass"
```

---

## Task 10: S2 最终验证

- [ ] **Step 1: grep 验证**

Run: `cd src-tauri && grep -rn "allow_all" src/ --include="*.rs" | grep -v "#\[cfg(test)\]\|tests/\|test\b"`
Expected: 无匹配（生产代码中不再有 allow_all 调用）

- [ ] **Step 2: Rust 全量测试**

Run: `cd src-tauri && cargo test`
Expected: 全绿

- [ ] **Step 3: Commit docs 更新**

更新 `docs/superpowers/plans/README.md` 中 P4 状态，记录 S1/S2 完成。

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add docs/
git commit -m "docs: mark S1 cancel unification and S2 permission bypass as complete"
```

---

# S3：P1 收尾 — tool round ownership

## Task 11-15: 沿用已有计划

**计划文件**: `docs/superpowers/plans/2026-04-15-p1-tool-round-ownership-plan.md` Task 4 及 Task 5。

S3 不重新定义 Task，直接执行已有计划的：
- **Task 4**：TauriLegacyTurnExecutor 实现 run_llm_step（Step 1-8）
- **Task 5**：更新文档和最终验证（Step 1-5）

**验收标准**：
1. T1-T6 review 测试在 production path 全绿
2. `legacy_send_message_impl` 不再持有 tool dispatch 逻辑
3. P1 状态标记为已关闭

---

## 风险与注意事项

1. **S1 Task 1（PluginContext 新增字段）的编译波及面**：PluginContext 在 36 个文件中使用，新增字段需要修复所有构造点。大多数只需加 `cancel_token: None`，但要逐一检查。
2. **S2 对已存储 permission 的影响**：如果 `PermissionStore` 为空（首次运行），legacy 工具会经过 `CapabilityPermissionPipeline`（而非 `AllowAllPermissionPipeline`），未知 scope 会被 deny 而非放行。需确认所有内置 legacy 工具的 `capability_scope` 都在已知范围内。
3. **S3 是最大风险项**：3900 行 agent_loop 拆分，见已有计划的风险分析。
4. **S1 和 S2 可以分别独立合并**：即使 S3 延期，S1+S2 的价值也是独立的。
