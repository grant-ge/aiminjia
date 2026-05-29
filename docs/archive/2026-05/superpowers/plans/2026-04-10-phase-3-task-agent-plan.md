# Phase 3 Task Agent Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 task 和 sub-agent 升级为一等 runtime 模型，完成 child run、sub-agent 阶段 A/B、`AgentInvocationStore` 接线，以及 Python `RunId` scope + recovery input 切换。

**Architecture:** `AgentRuntime` 接管旧 `sub_agent.rs`；`TaskRuntime` 接管后台任务状态；Skill 继续作为 `QueryEngine` 的策略插件，不成为独立 supervisor。Python 状态按 `RunId` scoped，并通过 artifact/snapshot/cache 恢复。

**Tech Stack:** Rust, cargo test, runtime stores, Python bridge, cancellation token

## 当前实际状态（2026-04-10）

- 状态：大部分完成
- 已落地：`runtime/task/*`、`runtime/agent/*`、`llm/sub_agent.rs` 接入 child run、取消链路与最小 agent runtime
- 已落地：Python session 已优先切到 `RunId` scope，recovery input builder 已存在
- 已落地：对应测试 `task_agent_runtime_test`、`sub_agent_runtime_integration_test`、`python_recovery_input_test`、`python_run_scope_test` 已通过
- 已验证：child run cancel、Python recovery、legacy loaded key 迁移都已有自动化测试覆盖
- 未完成：background/message bridge 与更彻底的 workflow/skill 归属清理还需要继续推进

---

**Phase constraints:**
- Phase 3 先完成 sub-agent 阶段 A/B：child run、受限工具集、真正可取消、可后台、可回传摘要；resume/worktree/team 延后到 Phase 4。
- Python recovery 不能只改 session key；必须同时明确恢复输入来自哪些运行产物。
- 旧 `loaded:{conversation_id}:*` 键的迁移策略必须在本期定清楚，否则 RunId scope 会把已加载状态直接打丢。

---

### Task 1: 建立 `TaskRuntime` 与 `AgentRuntime` 最小模型

**Files:**
- Create: `src-tauri/src/runtime/task/mod.rs`
- Create: `src-tauri/src/runtime/task/task_runtime.rs`
- Create: `src-tauri/src/runtime/task/task_models.rs`
- Create: `src-tauri/src/runtime/agent/mod.rs`
- Create: `src-tauri/src/runtime/agent/agent_runtime.rs`
- Create: `src-tauri/src/runtime/agent/invocation.rs`
- Test: `src-tauri/tests/task_agent_runtime_test.rs`

- [x] **Step 1: 写失败测试，要求能显式创建 child run invocation**

```rust
use app_lib::runtime::agent::invocation::AgentInvocation;
use app_lib::runtime::ids::{AgentId, RunId};

#[test]
fn creates_agent_invocation_with_child_run() {
    let invocation = AgentInvocation::new(
        AgentId::new("agent-1".into()),
        RunId::new("run-parent".into()),
        RunId::new("run-child".into()),
    );

    assert_eq!(invocation.child_run_id().as_str(), "run-child");
}
```

- [x] **Step 2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test task_agent_runtime_test -- --nocapture`
Expected: FAIL because runtime task/agent modules do not exist

- [x] **Step 3: 写最小模型与 store 接线**

```rust
pub struct TaskRecord {
    pub task_id: TaskId,
    pub parent_run_id: RunId,
    pub owner_agent_id: Option<AgentId>,
    pub subject: String,
    pub status: TaskStatus,
}

pub struct AgentInvocation {
    pub agent_id: AgentId,
    pub parent_run_id: RunId,
    pub child_run_id: RunId,
    pub status: AgentStatus,
    pub background: bool,
    pub summary_or_output_ref: Option<String>,
}
```

- [x] **Step 4: 运行测试确认通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test task_agent_runtime_test -- --nocapture`
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/task/*.rs src-tauri/src/runtime/agent/*.rs src-tauri/tests/task_agent_runtime_test.rs
git commit -m "feat: add task and agent runtime models"
```

### Task 2: 将 `sub_agent` 主路径切到 `AgentRuntime`，完成阶段 A/B

**Files:**
- Modify: `src-tauri/src/llm/sub_agent.rs`
- Create: `src-tauri/src/runtime/agent/child_run.rs`
- Create: `src-tauri/src/runtime/agent/background.rs`
- Create: `src-tauri/src/runtime/agent/message_bridge.rs`
- Test: `src-tauri/tests/sub_agent_runtime_integration_test.rs`

- [x] **Step 1: 写失败测试，要求取消能真正中断 child run**

```rust
use app_lib::runtime::agent::{AgentRuntime, SpawnChildRunRequest};
use app_lib::runtime::ids::RunId;

#[tokio::test]
async fn cancelling_parent_run_marks_child_run_cancelled() {
    let runtime = AgentRuntime::for_test();
    let request = SpawnChildRunRequest::for_test(RunId::new("run-parent".into()));

    let handle = runtime.spawn_child_run(request).await.unwrap();
    runtime.cancel_run(handle.child_run_id().clone()).await.unwrap();

    assert_eq!(runtime.status(handle.child_run_id()).await.unwrap(), "cancelled");
}
```

- [x] **Step 2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test sub_agent_runtime_integration_test -- --nocapture`
Expected: FAIL because child run 仍走旧 mini loop，cancel 没进入统一 runtime

- [x] **Step 3: 实现阶段 A：child run + 受限工具集 + cancel 生效**

```text
至少完成：
- child run 有独立 RunId
- tool allowlist 来自 parent invocation
- parent cancel 会传递到 child cancellation token
- child 生命周期写入 AgentInvocationStore / TaskStore
```

- [x] **Step 4: 实现阶段 B：background run + 摘要桥接**

```text
至少完成：
- child run 可切 background=true
- child 完成后可回传 summary/event 给 parent run
- UI 继续消费兼容的 agent:* 事件
```

- [x] **Step 5: 运行测试确认通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test sub_agent_runtime_integration_test -- --nocapture`
Expected: PASS

- [x] **Step 6: Commit**

```bash
git add src-tauri/src/llm/sub_agent.rs src-tauri/src/runtime/agent/child_run.rs src-tauri/src/runtime/agent/background.rs src-tauri/src/runtime/agent/message_bridge.rs src-tauri/tests/sub_agent_runtime_integration_test.rs
git commit -m "refactor: route sub-agent execution through agent runtime"
```

### Task 3: 明确 Python recovery 输入模型

**Files:**
- Modify: `src-tauri/src/python/session.rs`
- Create: `src-tauri/src/runtime/agent/python_recovery.rs`
- Test: `src-tauri/tests/python_recovery_input_test.rs`

- [x] **Step 1: 写失败测试，要求 recovery input 能从运行产物构建**

```rust
use app_lib::runtime::agent::python_recovery::{
    build_python_recovery_input_from_run_artifacts, PythonRunArtifacts,
};

#[test]
fn builds_python_recovery_input_from_run_artifacts() {
    let artifacts = PythonRunArtifacts {
        loaded_manifest_path: Some("artifacts/run-1/loaded-files.json".into()),
        analysis_snapshot_path: Some("artifacts/run-1/analysis.json".into()),
        precompute_cache_paths: vec!["artifacts/run-1/precompute.bin".into()],
        generated_artifact_refs: vec!["artifacts/run-1/output.csv".into()],
    };

    let recovery = build_python_recovery_input_from_run_artifacts(&artifacts).unwrap();

    assert_eq!(
        recovery.loaded_manifest_path.as_deref(),
        Some("artifacts/run-1/loaded-files.json")
    );
    assert_eq!(recovery.generated_artifact_refs.len(), 1);
}
```

- [x] **Step 2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test python_recovery_input_test -- --nocapture`
Expected: FAIL because recovery builder and artifact model do not exist

- [x] **Step 3: 固化 recovery 来源**

```text
PythonRecoveryInput 至少来自：
- loaded file manifest
- analysis snapshot / artifact
- precompute cache
- generated artifact refs

如果这些来源不完整，必须显式记录 degraded restore，而不是静默丢状态。
```

- [x] **Step 4: 写最小 recovery builder**

```rust
pub struct PythonRecoveryInput {
    pub loaded_manifest_path: Option<String>,
    pub analysis_snapshot_path: Option<String>,
    pub precompute_cache_paths: Vec<String>,
    pub generated_artifact_refs: Vec<String>,
}
```

- [x] **Step 5: 运行测试确认通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test python_recovery_input_test -- --nocapture`
Expected: PASS

- [x] **Step 6: Commit**

```bash
git add src-tauri/src/python/session.rs src-tauri/src/runtime/agent/python_recovery.rs src-tauri/tests/python_recovery_input_test.rs
git commit -m "feat: add python recovery input builder"
```

### Task 4: 切换 Python session 到 `RunId` scope，并补 legacy loaded key 迁移

**Files:**
- Modify: `src-tauri/src/python/session.rs`
- Modify: `src-tauri/src/runtime/agent/agent_runtime.rs`
- Test: `src-tauri/tests/python_run_scope_test.rs`

- [x] **Step 1: 写失败测试，要求 child run 使用独立 Python session 且可从 legacy key 恢复**

```rust
use app_lib::python::session::{migrate_loaded_keys_to_run_scope, session_key_for_run};
use app_lib::runtime::ids::RunId;

#[test]
fn python_sessions_are_scoped_by_run_id_and_legacy_loaded_keys_are_migrated() {
    let parent_key = session_key_for_run(&RunId::new("run-parent".into()));
    let child_key = session_key_for_run(&RunId::new("run-child".into()));

    let migrated = migrate_loaded_keys_to_run_scope("conv-1", &RunId::new("run-child".into()));

    assert_ne!(parent_key, child_key);
    assert_eq!(migrated.source_prefix, "loaded:conv-1");
    assert_eq!(migrated.target_prefix, "loaded:run-child");
}
```

- [x] **Step 2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test python_run_scope_test -- --nocapture`
Expected: FAIL because session manager still keys by `conversation_id`

- [x] **Step 3: 写最小按 `RunId` 作用域实现**

```rust
pub fn session_key_for_run(run_id: &RunId) -> String {
    format!("python-run:{}", run_id.as_str())
}
```

- [x] **Step 4: 补全 legacy loaded key 迁移策略**

```text
迁移规则：
- 旧 key 前缀：loaded:{conversation_id}:*
- 新 key 前缀：loaded:{run_id}:*
- 首次恢复时读取旧 key，生成 recovery input，并懒迁移到新 key
- child run 永远不复用 parent conversation scope key
```

- [x] **Step 5: 运行 Phase 3 回归**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test task_agent_runtime_test sub_agent_runtime_integration_test python_recovery_input_test python_run_scope_test -- --nocapture`
Expected: PASS

- [x] **Step 6: Commit**

```bash
git add src-tauri/src/python/session.rs src-tauri/src/runtime/agent/agent_runtime.rs src-tauri/tests/python_run_scope_test.rs
git commit -m "refactor: scope python sessions by run id"
```

## Definition of Done

- Task / Agent 已成为一等 runtime 模型。
- Sub-agent 阶段 A/B 可用：child run、cancel、background、summary bridge。
- Python session 已切到 `RunId` scope。
- Python recovery 输入的来源和 legacy loaded key 迁移策略已经明确并有测试约束。
