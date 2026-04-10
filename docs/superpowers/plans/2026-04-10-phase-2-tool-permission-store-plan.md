# Phase 2 Tool Permission Store Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 统一 `ToolDefinition`/`ToolDispatcher`/`PermissionPipeline`，落地 `RunStore`/`TaskStore`/`ToolCallStore`/`AgentInvocationStore` 最小持久化，并通过 `LegacyToolAdapter` 将旧工具接入 runtime 主链路。

**Architecture:** QueryEngine 不再直连工具实现，而是固定走 `ToolDispatcher -> PermissionPipeline -> ToolExecutor`。注意迁移顺序：先引入新的 runtime-facing tool contract 与 `LegacyToolAdapter`，再切 QueryEngine 主链路，最后再处理旧 `ToolPlugin` trait 的弃用与 `PluginContext` 收缩，避免中途把所有 builtin tools 编译打爆。

**Tech Stack:** Rust, cargo test, serde_json, file-based repository bridge, cancellation token

## 当前实际状态（2026-04-10）

- 状态：进行中
- 已落地：`runtime/store/*` 最小契约、`runtime/tools/*`、`LegacyToolAdapter`、`ToolDispatcher`、`PermissionPipeline`
- 已落地：`QueryEngine` 已能通过 dispatcher 触发工具调用；已有 `EchoRuntimeTool` 作为增量迁移样板
- 已落地：`plugin/context.rs` 与 `plugin/registry.rs` 已开始接入 runtime 身份与 run/agent 语义
- 已验证：`runtime_store_contract_test`、`tool_dispatcher_test`、`tool_runtime_integration_test`、`tool_trait_migration_test` 已通过
- 未完成：`PluginContext` 仍偏 service locator，旧 `ToolPlugin` trait 仍未真正标记为 legacy 并逐步退出

---

**Phase constraints:**
- 第 2 期必须保留 legacy 工具可运行，不允许先改 shared trait 再逼所有旧工具同时迁移。
- `ToolExecutionContext` 必须 task-aware，至少携带 `session_id`、`run_id`、`agent_id`、`tool_call_id`、`CancellationToken`、`EventSink`。
- `PluginContext` 必须开始拆分，禁止新的 runtime tool 直接拿到 service locator 式全量上下文。

---

### Task 1: 建立最小 runtime store 契约

**Files:**
- Create: `src-tauri/src/runtime/store/mod.rs`
- Create: `src-tauri/src/runtime/store/run_store.rs`
- Create: `src-tauri/src/runtime/store/task_store.rs`
- Create: `src-tauri/src/runtime/store/tool_call_store.rs`
- Create: `src-tauri/src/runtime/store/agent_invocation_store.rs`
- Test: `src-tauri/tests/runtime_store_contract_test.rs`

- [x] **Step 1: 写失败测试，要求最小 store trait 可被 runtime 聚合使用**

```rust
use app_lib::runtime::store::{
    AgentInvocationStore, RunStore, RuntimeStores, TaskStore, ToolCallStore,
};

#[test]
fn exposes_minimal_runtime_store_contracts() {
    fn assert_runtime_store_bundle<
        R: RunStore,
        T: TaskStore,
        C: ToolCallStore,
        A: AgentInvocationStore,
    >() {}

    let _ = RuntimeStores::builder();
    assert_runtime_store_bundle::<
        app_lib::runtime::store::InMemoryRunStore,
        app_lib::runtime::store::InMemoryTaskStore,
        app_lib::runtime::store::InMemoryToolCallStore,
        app_lib::runtime::store::InMemoryAgentInvocationStore,
    >();
}
```

- [x] **Step 2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test runtime_store_contract_test -- --nocapture`
Expected: FAIL with unresolved module `runtime::store`

- [x] **Step 3: 写最小 trait、record 与 in-memory test doubles**

```rust
pub struct AgentInvocationRecord {
    pub agent_id: AgentId,
    pub parent_run_id: RunId,
    pub child_run_id: RunId,
    pub status: AgentStatus,
    pub background: bool,
    pub summary_or_output_ref: Option<String>,
}

pub trait AgentInvocationStore {
    fn create_invocation(&self, record: AgentInvocationRecord) -> anyhow::Result<()>;
    fn update_invocation_status(&self, agent_id: &AgentId, status: AgentStatus) -> anyhow::Result<()>;
}
```

- [x] **Step 4: 运行测试确认通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test runtime_store_contract_test -- --nocapture`
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/store/*.rs src-tauri/tests/runtime_store_contract_test.rs
git commit -m "feat: add runtime store contracts"
```

### Task 2: 先建立新的 runtime tool contract 与 `LegacyToolAdapter`

**Files:**
- Create: `src-tauri/src/runtime/tools/mod.rs`
- Create: `src-tauri/src/runtime/tools/definition.rs`
- Create: `src-tauri/src/runtime/tools/context.rs`
- Create: `src-tauri/src/runtime/tools/dispatcher.rs`
- Create: `src-tauri/src/runtime/tools/permission.rs`
- Create: `src-tauri/src/runtime/tools/executor.rs`
- Create: `src-tauri/src/runtime/tools/legacy_adapter.rs`
- Test: `src-tauri/tests/tool_dispatcher_test.rs`

- [x] **Step 1: 写失败测试，要求旧 builtin tool 可先通过 `LegacyToolAdapter` 进入新 contract**

```rust
use app_lib::runtime::tools::{LegacyToolAdapter, RuntimeTool, ToolExecutionContext};
use serde_json::json;

#[tokio::test]
async fn legacy_tool_adapter_executes_builtin_tool_through_runtime_contract() {
    let tool = LegacyToolAdapter::for_test("python_exec");
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tool-1");

    let result = RuntimeTool::execute(&tool, json!({"code":"print(1)"}), ctx)
        .await
        .expect("legacy adapter should bridge builtin tool");

    assert_eq!(result.tool_name, "python_exec");
}
```

- [x] **Step 2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test tool_dispatcher_test -- --nocapture`
Expected: FAIL because `runtime::tools` / `LegacyToolAdapter` / `RuntimeTool` do not exist

- [x] **Step 3: 写最小 `ToolExecutionContext`、`ToolDefinition`、`RuntimeTool`**

```rust
pub struct ToolExecutionContext {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub agent_id: Option<AgentId>,
    pub tool_call_id: ToolCallId,
    pub cancellation: CancellationToken,
    pub event_sink: Arc<dyn ToolEventSink>,
}

#[async_trait::async_trait]
pub trait RuntimeTool: Send + Sync {
    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError>;
}
```

- [x] **Step 4: 写 `LegacyToolAdapter`，但不要马上改 `plugin/tool_trait.rs`**

```rust
pub struct LegacyToolAdapter {
    definition: ToolDefinition,
    inner: Arc<dyn LegacyTool>,
}
```

- [x] **Step 5: 让测试通过，确保此时 builtin tools 仍沿用旧 trait 编译**

```text
此步结束后：
- 新 runtime tool contract 已存在
- 旧 builtin tools 仍走旧 trait
- 只有 adapter 负责桥接
```

- [x] **Step 6: 运行测试确认通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test tool_dispatcher_test -- --nocapture`
Expected: PASS

- [x] **Step 7: Commit**

```bash
git add src-tauri/src/runtime/tools/*.rs src-tauri/tests/tool_dispatcher_test.rs
git commit -m "feat: add runtime tool contract and legacy adapter"
```

### Task 3: 切 QueryEngine 主链路到 `ToolDispatcher -> PermissionPipeline`

**Files:**
- Modify: `src-tauri/src/runtime/query_engine.rs`
- Modify: `src-tauri/src/llm/tools.rs`
- Modify: `src-tauri/src/plugin/registry.rs`
- Modify: `src-tauri/src/plugin/context.rs`
- Modify: `src-tauri/src/commands/chat.rs`
- Test: `src-tauri/tests/tool_runtime_integration_test.rs`

- [x] **Step 1: 写失败测试，要求 QueryEngine 通过 dispatcher 调用工具并产出 legacy 事件**

```rust
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::tools::ToolDispatcher;
use app_lib::runtime::tools::permission::AllowAllPermissionPipeline;
use app_lib::runtime::tools::testing::single_legacy_tool_dispatcher;

#[tokio::test]
async fn query_engine_routes_tool_calls_through_dispatcher_and_permission_pipeline() {
    let dispatcher = single_legacy_tool_dispatcher("python_exec");
    let engine = QueryEngine::for_test(ToolDispatcher::new(dispatcher, AllowAllPermissionPipeline));

    let trace = engine
        .run_single_tool_turn("conv-1", "run-1", "python_exec")
        .await
        .expect("tool turn should succeed");

    assert_eq!(trace.event_names(), vec!["tool:executing", "tool:completed", "streaming:done"]);
}
```

- [x] **Step 2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test tool_runtime_integration_test -- --nocapture`
Expected: FAIL because QueryEngine 仍绕过 dispatcher

- [x] **Step 3: 将 `PluginContext` 拆为 runtime-facing capability context**

```text
至少拆出：
- ToolExecutionContext：run/session/tool_call/cancel/event
- ToolCapabilityContext：文件、存储、auth、python、browser 等按能力域注入

禁止：
- 让 runtime tool 继续直接拿到全量 PluginContext
```

- [x] **Step 4: 让旧工具通过 `LegacyToolAdapter` 注册进 `ToolDispatcher`**

```text
registry -> legacy adapter -> runtime dispatcher
此时旧 trait 仍存在，但主链路已经不再直接依赖它
```

- [x] **Step 5: 运行回归测试确认通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test runtime_store_contract_test tool_dispatcher_test tool_runtime_integration_test -- --nocapture`
Expected: PASS

- [x] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/query_engine.rs src-tauri/src/llm/tools.rs src-tauri/src/plugin/registry.rs src-tauri/src/plugin/context.rs src-tauri/src/commands/chat.rs src-tauri/tests/tool_runtime_integration_test.rs
git commit -m "refactor: route tool execution through dispatcher"
```

### Task 4: 收紧旧 `ToolPlugin` trait，并为迁移期留下兼容层

**Files:**
- Modify: `src-tauri/src/plugin/tool_trait.rs`
- Modify: `src-tauri/src/plugin/builtin/tools/*`
- Modify: `src-tauri/src/runtime/tools/legacy_adapter.rs`
- Test: `src-tauri/tests/tool_trait_migration_test.rs`

- [x] **Step 1: 写失败测试，要求新 builtin tool 可以直接实现 runtime contract**

```rust
use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
use serde_json::json;

#[tokio::test]
async fn builtin_tool_can_migrate_off_legacy_trait_incrementally() {
    let tool = app_lib::plugin::builtin::tools::echo::EchoRuntimeTool::default();
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tool-1");

    let result = RuntimeTool::execute(&tool, json!({"text":"hi"}), ctx).await.unwrap();
    assert_eq!(result.output_text(), "hi");
}
```

- [x] **Step 2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test tool_trait_migration_test -- --nocapture`
Expected: FAIL because builtin runtime tool migration path does not exist yet

- [x] **Step 3: 将 `plugin/tool_trait.rs` 标记为 legacy，并补一层向后兼容说明**

```text
这一步只做：
- deprecated 标记
- 迁移注释
- adapter 继续可用

不做：
- 一次性要求所有 builtin tools 同时改签名
```

- [x] **Step 4: 优先迁移 1 个简单 builtin tool 作为样板**

```text
目标是验证增量迁移路径，而不是在 Phase 2 一口气改完整个工具目录。
```

- [x] **Step 5: 运行总回归**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test runtime_store_contract_test tool_dispatcher_test tool_runtime_integration_test tool_trait_migration_test -- --nocapture`
Expected: PASS

- [x] **Step 6: Commit**

```bash
git add src-tauri/src/plugin/tool_trait.rs src-tauri/src/plugin/builtin/tools src-tauri/src/runtime/tools/legacy_adapter.rs src-tauri/tests/tool_trait_migration_test.rs
git commit -m "refactor: prepare incremental tool trait migration"
```

## Definition of Done

- `ToolDispatcher` 成为唯一工具入口。
- `PermissionPipeline` 成为唯一权限判定位置。
- `RunStore` / `TaskStore` / `ToolCallStore` / `AgentInvocationStore` 最小契约可用。
- 编译链不中断：在切换完成前，旧 builtin tools 始终能通过 `LegacyToolAdapter` 进入新链路。
