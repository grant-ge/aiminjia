# Phase 4 Store Transport Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 完成领域化 Store 拆分、Tauri transport 解耦，并补齐 SubAgent 阶段 C（resume/worktree/team 最小模型）。

**Architecture:** runtime 层彻底不依赖 Tauri；transport 只做 adapter；一体化 `file_store` DAO 退场；resume/worktree/team 以 `AgentRuntime`/`TaskStore`/`AgentInvocationStore` 为底座。

**Tech Stack:** Rust, Tauri, cargo test, repository traits, file-based impl, worktree context abstraction

## 当前实际状态（2026-04-10）

- 状态：进行中 → 收尾中（4 个 subagent 并行推进 file_store 收缩、非 chat transport 化、PluginContext 标记、background bridge 真实接线）
- 已落地：`transport/mod.rs`、`transport/runtime_host.rs`、`transport/tauri_runtime_host.rs`、`transport/tauri_commands/chat.rs`
- 已落地：chat 相关 Tauri command 已重定向到 transport adapter；runtime 已可在无 `AppHandle` 测试场景执行
- 已落地：domain store façade 与 Stage C 最小 resume/worktree/team 测试已存在
- 新增推进：`runtime/conversation_service.rs` 已把基础 conversation CRUD / stop_streaming 下沉到 runtime
- 已验证：`domain_store_test`、`transport_adapter_test`、`agent_stage_c_test`、`conversation_runtime_service_test` 已通过
- 未完成：完整 store 领域拆分、`file_store` 退场、非 chat commands 全量 transport 化还没结束

---

**Phase constraints:**
- Store 拆分要先做 repository facade，再逐步削薄旧 `file_store/mod.rs`，不能一把梭删除所有旧实现。
- `transport` 适配层要证明 runtime 在无 `AppHandle` 场景也能执行。
- 阶段 C 的 resume 必须基于 `AgentInvocationStore` / `RunStore` 恢复，而不是靠 UI 猜状态。

---

### Task 1: 拆 Store 领域实现与 repository facade

**Files:**
- Create: `src-tauri/src/runtime/store/session_store.rs`
- Create: `src-tauri/src/runtime/store/settings_store.rs`
- Create: `src-tauri/src/runtime/store/memory_store.rs`
- Create: `src-tauri/src/runtime/store/audit_store.rs`
- Modify: `src-tauri/src/storage/file_store/mod.rs`
- Test: `src-tauri/tests/domain_store_test.rs`

- [x] **Step 1: 写失败测试，要求 file-store façade 能按领域暴露 repository**

```rust
use app_lib::runtime::store::{AuditStore, MemoryStore, SessionStore, SettingsStore};
use app_lib::storage::file_store::RuntimeRepositoryFacade;

#[test]
fn file_store_exposes_domain_repositories() {
    let facade = RuntimeRepositoryFacade::for_test();

    let _: &dyn SessionStore = facade.session_store();
    let _: &dyn SettingsStore = facade.settings_store();
    let _: &dyn MemoryStore = facade.memory_store();
    let _: &dyn AuditStore = facade.audit_store();
}
```

- [x] **Step 2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test domain_store_test -- --nocapture`
Expected: FAIL because domain stores / façade do not exist

- [x] **Step 3: 写最小 domain store shell 与 façade**

```rust
pub trait SessionStore {
    fn load_session(&self, session_id: &SessionId) -> anyhow::Result<SessionRecord>;
}

pub trait AuditStore {
    fn append_event(&self, run_id: &RunId, event: &str) -> anyhow::Result<()>;
}
```

- [ ] **Step 4: 把旧 `file_store/mod.rs` 收缩为实现细节，而不是业务入口**

```text
要求：
- runtime 只依赖 domain traits / façade
- command / tools 不再直接散落调用 file_store 内部细节
```

- [x] **Step 5: 运行测试确认通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test domain_store_test -- --nocapture`
Expected: PASS

- [x] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/store/*.rs src-tauri/src/storage/file_store/mod.rs src-tauri/tests/domain_store_test.rs
git commit -m "feat: split runtime stores by domain"
```

### Task 2: 拆 Tauri transport adapter

**Files:**
- Create: `src-tauri/src/transport/mod.rs`
- Create: `src-tauri/src/transport/tauri_runtime_host.rs`
- Create: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: `src-tauri/src/commands/chat.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/tests/transport_adapter_test.rs`

- [x] **Step 1: 写失败测试，要求 runtime 可在无 `AppHandle` 情况下执行**

```rust
use app_lib::runtime::session_runtime::SessionRuntime;
use app_lib::transport::testing::NoopRuntimeHost;

#[tokio::test]
async fn runtime_core_executes_without_tauri_app_handle() {
    let host = NoopRuntimeHost::default();
    let runtime = SessionRuntime::for_test(host);

    let trace = runtime.run_for_test("conv-1", "run-1", "hello").await.unwrap();
    assert_eq!(trace.event_names(), vec!["streaming:delta", "message:updated", "streaming:done"]);
}
```

- [x] **Step 2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test transport_adapter_test -- --nocapture`
Expected: FAIL because runtime still imports Tauri types transitively

- [x] **Step 3: 写最小 host trait 与 transport adapter**

```rust
pub trait RuntimeHost {
    fn emit_legacy_event(&self, name: &str, payload: serde_json::Value) -> anyhow::Result<()>;
}

pub struct TauriRuntimeHost {
    pub app: tauri::AppHandle,
}
```

- [x] **Step 4: 将 chat command 重定向到 `transport/tauri_commands/chat.rs`**

```text
`src-tauri/src/commands/chat.rs` 变成兼容 re-export 或薄 wrapper。
真实 transport 装配放到 `transport/tauri_commands/chat.rs`。
```

- [x] **Step 5: 运行测试确认通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test transport_adapter_test -- --nocapture`
Expected: PASS

- [x] **Step 6: Commit**

```bash
git add src-tauri/src/transport/*.rs src-tauri/src/transport/tauri_commands/chat.rs src-tauri/src/commands/chat.rs src-tauri/src/lib.rs src-tauri/tests/transport_adapter_test.rs
git commit -m "refactor: move tauri command flow into transport adapters"
```

### Task 3: 完成 resume/worktree/team 最小模型（SubAgent 阶段 C）

**Files:**
- Create: `src-tauri/src/runtime/agent/resume.rs`
- Create: `src-tauri/src/runtime/agent/worktree.rs`
- Create: `src-tauri/src/runtime/agent/team.rs`
- Modify: `src-tauri/src/runtime/agent/agent_runtime.rs`
- Test: `src-tauri/tests/agent_stage_c_test.rs`

- [x] **Step 1: 写失败测试，要求可基于 invocation store 恢复 child run**

```rust
use app_lib::runtime::agent::{AgentRuntime, ResumeChildRunRequest};
use app_lib::runtime::store::InMemoryAgentInvocationStore;

#[tokio::test]
async fn restores_child_run_from_agent_invocation_store() {
    let store = InMemoryAgentInvocationStore::with_child_run("agent-1", "run-parent", "run-child");
    let runtime = AgentRuntime::for_resume_test(store);

    let restored = runtime
        .resume_child_run(ResumeChildRunRequest::new("agent-1"))
        .await
        .unwrap();

    assert_eq!(restored.child_run_id().as_str(), "run-child");
}
```

- [x] **Step 2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test agent_stage_c_test -- --nocapture`
Expected: FAIL because `resume` / `worktree` / `team` modules do not exist

- [x] **Step 3: 写最小 resume/worktree/team 实现**

```rust
pub struct WorktreeContext {
    pub root: String,
    pub branch: Option<String>,
}

pub struct TeamContext {
    pub team_id: String,
    pub agent_ids: Vec<AgentId>,
}
```

- [x] **Step 4: 把恢复来源写死到 store，而不是 UI patch**

```text
恢复最少依赖：
- AgentInvocationStore
- RunStore
- TaskStore
- Python recovery input

UI 只能消费恢复结果，不能再自己拼状态。
```

- [x] **Step 5: 运行 Phase 4 总回归**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test domain_store_test transport_adapter_test agent_stage_c_test -- --nocapture`
Expected: PASS

- [x] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/agent/resume.rs src-tauri/src/runtime/agent/worktree.rs src-tauri/src/runtime/agent/team.rs src-tauri/src/runtime/agent/agent_runtime.rs src-tauri/tests/agent_stage_c_test.rs
git commit -m "feat: add stage-c agent runtime capabilities"
```

## Definition of Done

- runtime 核心模块不再依赖 Tauri 类型。
- `file_store` 已退化为 domain repository façade 背后的实现。
- Tauri 只是 transport adapter。
- SubAgent 阶段 C 的 resume/worktree/team 有明确的数据来源与测试约束。
