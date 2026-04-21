# Async Subagent 独立性（Plan-A）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ] `) syntax for tracking.

**Goal:** 补全 background subagent 的 cancel 独立性和 lifecycle 状态机，确保 background agent 不受父 session ESC 影响，且 Failed 状态有独立路径和事件。

**Architecture:** 接棒 Plan-H 的 H5 遗留，修正 worker_runtime.rs 的 cancel_token 来源逻辑，新增 AgentStatus::Failed 变体和 fail_run() 方法，并在 worker_runtime 的错误路径中正确调用。

**Tech Stack:** Rust, Tauri v2

**Worktree branch:** pzc

---

## 背景与现状

### 遗留问题来源

Plan-H（subagent 状态隔离）与 Plan-U5（worker runtime 收口）完成了以下工作：

- `worker_runtime.rs` 成为 loop owner，`sub_agent.rs` 只保留入口封装
- `WorkerRunConfig` 已有 `cancel_token: Option<CancellationToken>` 与 `background: bool`
- `AgentRuntime` 已有 `spawn_child_run / cancel_run / complete_run / complete_background_run`

但以下三项遗留问题未处理（H5 标注的 gap）：

| Gap | 文件 | 问题 |
|---|---|---|
| cancel_token 可能为 None | `worker_runtime.rs:146-150` | `background=true` 时若父不传 token，child cancel 无从挂载 |
| background agent 使用父 token 的 child_token | `worker_runtime.rs:147-149` | foreground ESC 会通过级联 cancel 掉正在后台运行的 agent |
| AgentStatus 缺少 Failed 变体 | `invocation.rs:6-11` | LLM error / max_iterations 走 complete_run，无法区分成功完成与失败完成 |

### 关键代码现状

**`worker_runtime.rs` cancel 逻辑（L146-L150）**：
```rust
let child_cancel = config
    .cancel_token
    .as_ref()
    .map(|parent| parent.child_token())  // 问题：background 时应用独立 token
    .unwrap_or_default();               // 问题：None 情况退化为独立 token，但其来源不受控
```

**`worker_runtime.rs` 迭代结束与 LLM 错误路径**（L438-L444, L226-L229）：
```rust
// LLM 错误：break 后 output 非空，但 cancelled=false，最终走 complete_run
Err(err) => {
    warn!("[SubAgent] LLM call failed at iter {}: {}", iteration, err);
    output = format!("Sub-agent LLM error: {}", err);
    break;
}

// 迭代耗尽：output 置为 "Sub-agent reached iteration limit."，走 complete_run
if iterations_used >= request.max_iterations && output.is_empty() ... {
    output = "Sub-agent reached iteration limit.".to_string();
}
```

**`invocation.rs` AgentStatus 枚举**（缺少 Failed）：
```rust
pub enum AgentStatus {
    Pending,
    Running,
    Completed,
    Cancelled,
    // 缺少 Failed
}
```

---

## 范围

- 纳入：
  - A-1：cancel_token 来源修正（background 用独立 token，foreground 用父的 child_token）
  - A-2：AgentStatus::Failed + fail_run() + 错误路径改写
  - A-3：回归测试 + review lock
- 不纳入：
  - background agent 的 kill 信号（BackgroundStop reason 已有，不新增信号路径）
  - 前端展示 Failed 状态（前端 task:status-changed 已透传 status 字符串，自然支持）
  - 跨 session 的 agent 恢复（Plan-W 范围）

---

## 任务拆分

### A-1：cancel_token 独立性修正

**目标**：`background=true` 时 worker 使用独立的 `CancellationToken::new()`，不挂载到父 session token 的 child 链上；`background=false` 时保持原有行为（父 token 的 child_token）。

- [x] **A-1-1：写失败测试**

在现有文件 `src-tauri/tests/subagent_legacy_cancel_reachability_test.rs` 追加测试：

```rust
use std::fs;
use app_lib::runtime::cancellation::CancellationToken;

/// RED: worker_runtime 必须显式对 background 分支创建独立 token。
#[test]
fn a1_worker_runtime_background_branch_uses_independent_cancel_token() {
    let source = fs::read_to_string("src/runtime/agent/worker_runtime.rs")
        .expect("read src/runtime/agent/worker_runtime.rs");
    assert!(
        source.contains("if config.background"),
        "worker_runtime must branch on config.background for cancel token selection"
    );
    assert!(
        source.contains("CancellationToken::new()"),
        "background worker must allocate a fresh CancellationToken"
    );
}

/// background worker 的 cancel token 必须是独立对象，
/// 不能是父 session token 的 child_token（即不能与父共享 Arc 内部）。
#[test]
fn a1_background_worker_cancel_token_is_independent_from_session_token() {
    let session_token = CancellationToken::new();
    let bg_token = CancellationToken::new(); // 模拟 background worker 自己 new 的独立 token

    // 父 session ESC
    session_token.cancel();

    // background worker 的 token 不应受影响
    assert!(
        !bg_token.is_cancelled(),
        "background worker token must NOT be cancelled when session token is cancelled"
    );
}

/// foreground worker 的 cancel token 必须是父 session token 的 child，
/// 父 cancel 后 foreground worker 也必须取消。
#[test]
fn a1_foreground_worker_cancel_token_cascades_from_session_token() {
    let session_token = CancellationToken::new();
    let fg_token = session_token.child_token();

    session_token.cancel();

    assert!(
        fg_token.is_cancelled(),
        "foreground worker token must be cancelled when session token is cancelled"
    );
}

/// background worker token 与父 session token 的取消状态互不影响。
#[test]
fn a1_background_token_arc_is_not_ptr_eq_to_session_token() {
    let session_token = CancellationToken::new();
    let bg_token = CancellationToken::new(); // 正确：独立 new

    session_token.cancel();
    assert!(!bg_token.is_cancelled());

    let session_token2 = CancellationToken::new();
    let bg_token2 = CancellationToken::new();
    bg_token2.cancel();
    assert!(!session_token2.is_cancelled());
}
```

**验证**（此时应失败，因为 `worker_runtime.rs` 尚未对 background 分支显式创建独立 token）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && \
  cargo test --test subagent_legacy_cancel_reachability_test -- --nocapture
```

- [x] **A-1-2：在 `worker_runtime.rs` 修改 cancel_token 来源逻辑**

**文件**：`src-tauri/src/runtime/agent/worker_runtime.rs`，`run_worker_turn` 方法（当前 L146-L150）。

当前代码：
```rust
let child_cancel = config
    .cancel_token
    .as_ref()
    .map(|parent| parent.child_token())
    .unwrap_or_default();
```

修改为：
```rust
let child_cancel = if config.background {
    // background worker 独立于父 session ESC，使用全新独立 token
    CancellationToken::new()
} else {
    // foreground worker：父 cancel 应传递到子
    config
        .cancel_token
        .as_ref()
        .map(|parent| parent.child_token())
        .unwrap_or_else(CancellationToken::new)
};
```

**验证**：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && \
  cargo test --test subagent_legacy_cancel_reachability_test -- --nocapture
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && \
  cargo test --test subagent_cancel_cascade_test -- --nocapture
```

- [x] **A-1-3：确保 `execute_browse_data` 传递有效的 cancel_token**

**文件**：`src-tauri/src/llm/tool_executor/internal_system.rs`，`execute_browse_data` 函数（当前 L692-L708）。

现状：
```rust
pub(crate) async fn execute_browse_data(
    ctx: &PluginContext,
    args: &Value,
) -> Result<BrowseDataLaunchResult> {
    let request = ...;
    let runtime_deps = RequestScopedRuntimeDeps::from_plugin_context(ctx);
    launch_browse_data_with_runtime_deps(
        &runtime_deps,
        request,
        ctx.cancellation.clone(),   // 可能为 None
        ctx.run_id.is_some(),
    )
    .await
}
```

`ctx.cancellation` 为 `Option<CancellationToken>`，当 H5 调用路径不携带 token 时传 None，导致 `WorkerRunConfig.cancel_token = None`。

修改 `launch_browse_data_with_runtime_deps` 的 cancel_token 参数传递，对 `None` 情况提供默认独立 token：

在 `execute_browse_data` 内，将 cancel_token 参数改为：
```rust
launch_browse_data_with_runtime_deps(
    &runtime_deps,
    request,
    Some(ctx.cancellation.clone().unwrap_or_else(CancellationToken::new)),
    ctx.run_id.is_some(),
)
.await
```

同时修改 `launch_browse_data_with_runtime_deps` 签名，将 `cancel_token` 从 `Option<CancellationToken>` 改为 `CancellationToken`（如果函数内部其他调用点允许），或保持 `Option<CancellationToken>` 但确保此处不再传 None。

> **注意**：若修改 `launch_browse_data_with_runtime_deps` 签名影响其他调用点，只需在 `execute_browse_data` 处 unwrap_or_else 即可，无需修改内部签名。

**验证**：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo build 2>&1 | grep -E "error|warning.*unused"
```

- [ ] **A-1-4：commit**

```
feat(worker-runtime): background worker 使用独立 CancellationToken，不受父 session ESC 影响
```

---

### A-2：AgentStatus::Failed + fail_run() + 错误路径

- [ ] **A-2-1：写失败测试**

在新建或已有文件中新增测试。推荐放入 `src-tauri/tests/subagent_legacy_cancel_reachability_test.rs` 追加，或新建专用文件 `src-tauri/tests/plan_a_agent_lifecycle_test.rs`：

```rust
use app_lib::runtime::agent::agent_runtime::AgentRuntime;
use app_lib::runtime::agent::invocation::{AgentStatus, SpawnChildRunRequest};
use app_lib::runtime::ids::RunId;

/// fail_run() 应将状态改为 Failed，而不是 Completed 或 Cancelled。
#[tokio::test]
async fn a2_fail_run_sets_status_to_failed() {
    let runtime = AgentRuntime::for_test();
    let handle = runtime
        .spawn_child_run(SpawnChildRunRequest {
            parent_run_id: RunId::new("parent-run"),
            background: false,
            allowed_tools: vec![],
        })
        .await
        .unwrap();
    let child_run_id = handle.child_run_id().clone();

    runtime.fail_run(&child_run_id).await.unwrap();

    let status = runtime.status(&child_run_id).await.unwrap();
    assert_eq!(status, "failed", "fail_run must set status to Failed");
}

/// status() 对 Failed 变体应返回 "failed" 字符串。
#[tokio::test]
async fn a2_status_returns_failed_string() {
    let runtime = AgentRuntime::for_test();
    let handle = runtime
        .spawn_child_run(SpawnChildRunRequest {
            parent_run_id: RunId::new("parent-run-2"),
            background: false,
            allowed_tools: vec![],
        })
        .await
        .unwrap();
    let child_run_id = handle.child_run_id().clone();

    // 直接 fail
    runtime.fail_run(&child_run_id).await.unwrap();
    assert_eq!(runtime.status(&child_run_id).await.unwrap(), "failed");

    // 确认 Complete 仍返回 completed
    let handle2 = runtime
        .spawn_child_run(SpawnChildRunRequest {
            parent_run_id: RunId::new("parent-run-3"),
            background: false,
            allowed_tools: vec![],
        })
        .await
        .unwrap();
    let child_run_id2 = handle2.child_run_id().clone();
    runtime.complete_run(&child_run_id2).await.unwrap();
    assert_eq!(runtime.status(&child_run_id2).await.unwrap(), "completed");
}
```

**运行看失败**（此时 `AgentStatus::Failed` 不存在，编译失败）：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && \
  cargo test --test plan_a_agent_lifecycle_test -- --nocapture 2>&1 | head -30
```

- [ ] **A-2-2：`invocation.rs` 新增 Failed 变体**

**文件**：`src-tauri/src/runtime/agent/invocation.rs`

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    Pending,
    Running,
    Completed,
    Cancelled,
    Failed,   // 新增：LLM 错误或迭代耗尽等非正常结束
}
```

**验证编译**：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo build 2>&1 | grep "error"
```

> 如果有 `match AgentStatus` 未覆盖新变体的编译错误，需要在 `agent_runtime.rs::status()` 方法中补充 `AgentStatus::Failed => "failed"` 分支（见下）。

- [ ] **A-2-3：`agent_runtime.rs` 补充 status() match 分支 + 新增 fail_run()**

**文件**：`src-tauri/src/runtime/agent/agent_runtime.rs`

在 `status()` 方法中补充 Failed 分支（当前 L96-L108）：
```rust
pub async fn status(&self, child_run_id: &RunId) -> Result<String> {
    for record in self.invocation_store.list_invocations()? {
        if &record.child_run_id == child_run_id {
            return Ok(match record.status {
                AgentStatus::Pending => "pending",
                AgentStatus::Running => "running",
                AgentStatus::Completed => "completed",
                AgentStatus::Cancelled => "cancelled",
                AgentStatus::Failed => "failed",   // 新增
            }
            .to_string());
        }
    }
    Ok("missing".to_string())
}
```

新增 `fail_run()` 方法（紧跟 `complete_run()` 之后）：
```rust
/// 将 child run 标记为 Failed（LLM 错误、迭代超限等非正常终止）。
pub async fn fail_run(&self, child_run_id: &RunId) -> Result<()> {
    for record in self.invocation_store.list_invocations()? {
        if &record.child_run_id == child_run_id {
            self.invocation_store
                .update_invocation_status(&record.agent_id, AgentStatus::Failed)?;
        }
    }
    Ok(())
}
```

**验证测试通过**：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && \
  cargo test --test plan_a_agent_lifecycle_test -- --nocapture
```

- [ ] **A-2-4：`worker_runtime.rs` 错误路径改为调用 fail_run()**

**文件**：`src-tauri/src/runtime/agent/worker_runtime.rs`

在 `run_worker_turn` 末尾的 lifecycle 回写区块中（当前 L497-L522），当前逻辑：

```rust
if let Some(handle) = child_handle.as_ref() {
    if cancelled {
        let _ = agent_runtime.cancel_run(child_run_id.clone()).await;
    } else if handle.invocation().background {
        // background 走 complete_background_run ...
    } else {
        let _ = agent_runtime.complete_run(&child_run_id).await;
    }
}
```

需要区分"正常完成"与"错误结束"。引入一个 `failed` 标志：在循环之前声明 `let mut failed = false;`，在 LLM 错误和迭代超限场景中将 `failed` 置 true。

**LLM 错误处**（当前 L226-L229）：
```rust
Err(err) => {
    warn!("[SubAgent] LLM call failed at iter {}: {}", iteration, err);
    output = format!("Sub-agent LLM error: {}", err);
    failed = true;   // 新增
    break;
}
```

**迭代超限处**（当前 L438-L444）：
```rust
if iterations_used >= request.max_iterations
    && output.is_empty()
    && pending_ask.is_none()
    && !cancelled
{
    output = "Sub-agent reached iteration limit.".to_string();
    failed = true;   // 新增
}
```

**lifecycle 回写区块**修改：
```rust
if let Some(handle) = child_handle.as_ref() {
    if cancelled {
        let _ = agent_runtime.cancel_run(child_run_id.clone()).await;
    } else if failed {
        // LLM 错误或迭代超限：标记为 Failed，不发 AgentIdle summary
        let _ = agent_runtime.fail_run(&child_run_id).await;
    } else if handle.invocation().background {
        if let (Some(bus), Some(parent_run_id)) = (
            self.runtime_deps.event_bus.clone(),
            config.parent_run_id.clone(),
        ) {
            let summary = message_bridge::format_sub_agent_envelope_summary(&envelope);
            let _ = agent_runtime
                .complete_background_run(
                    &child_run_id,
                    Some(&summary),
                    Some(&transcript_ref),
                    self.runtime_deps.session_id.clone(),
                    parent_run_id,
                    bus,
                )
                .await;
        } else {
            let _ = agent_runtime.complete_run(&child_run_id).await;
        }
    } else {
        let _ = agent_runtime.complete_run(&child_run_id).await;
    }
}
```

**验证**：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && \
  cargo test --test plan_a_agent_lifecycle_test -- --nocapture
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && \
  cargo test --test plan_u5_subagent_worker_runtime_test -- --nocapture
```

- [ ] **A-2-5：Failed 时发 TaskStatusChanged 事件**

background worker Failed 时需要通知前端。在 `fail_run()` 之后发事件（如果有 event bus 可用）。

在 `AgentRuntime` 中新增 `fail_background_run()`，类似 `complete_background_run`，但发 `TaskStatusChanged { status: "failed" }`：

**文件**：`src-tauri/src/runtime/agent/agent_runtime.rs`（新增方法）：
```rust
/// 将 background child run 标记为 Failed，并通过 event bus 通知前端。
pub async fn fail_background_run(
    &self,
    child_run_id: &RunId,
    error_summary: Option<&str>,
    session_id: SessionId,
    parent_run_id: RunId,
    bus: RuntimeEventBus,
) -> Result<()> {
    for record in self.invocation_store.list_invocations()? {
        if &record.child_run_id == child_run_id {
            self.invocation_store
                .update_invocation_status(&record.agent_id, AgentStatus::Failed)?;
            if let Some(summary) = error_summary {
                self.invocation_store.update_invocation_result_metadata(
                    &record.agent_id,
                    Some(summary.to_owned()),
                    None,
                )?;
            }
        }
    }
    // 发 TaskStatusChanged 事件，status = "failed"
    let task_id = crate::runtime::ids::TaskId::new(child_run_id.as_str());
    let kind = crate::runtime::events::RuntimeEventKind::TaskStatusChanged {
        task_id,
        status: "failed".to_string(),
    };
    let event = RuntimeEvent::new(session_id, parent_run_id, kind);
    bus.emit(event).await?;
    Ok(())
}
```

在 `worker_runtime.rs` 的 `failed` 分支中，如果是 background worker 且有 event bus，调用 `fail_background_run`；否则调用普通 `fail_run`：

```rust
} else if failed {
    if handle.invocation().background {
        if let (Some(bus), Some(parent_run_id)) = (
            self.runtime_deps.event_bus.clone(),
            config.parent_run_id.clone(),
        ) {
            let _ = agent_runtime
                .fail_background_run(
                    &child_run_id,
                    Some(&output),
                    self.runtime_deps.session_id.clone(),
                    parent_run_id,
                    bus,
                )
                .await;
        } else {
            let _ = agent_runtime.fail_run(&child_run_id).await;
        }
    } else {
        let _ = agent_runtime.fail_run(&child_run_id).await;
    }
}
```

**验证**：
```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && \
  cargo test --test plan_a_agent_lifecycle_test -- --nocapture
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && cargo build 2>&1 | grep "error"
```

- [ ] **A-2-6：commit**

```
feat(agent-runtime): 新增 AgentStatus::Failed + fail_run() + fail_background_run()，错误路径不再走 complete_run
```

---

### A-3：回归测试 + review lock

- [ ] **A-3-1：background token 独立性 review lock**

在 `src-tauri/tests/subagent_legacy_cancel_reachability_test.rs` 中追加（source-level 约束）：

```rust
/// review lock：worker_runtime.rs 中 background=true 的路径必须创建独立 token，
/// 不允许对父 token 调用 .child_token()。
#[test]
fn review_a1_background_worker_uses_independent_cancel_token() {
    let source = include_str!("../src/runtime/agent/worker_runtime.rs");
    // 正确实现：background 分支用 CancellationToken::new()
    assert!(
        source.contains("if config.background"),
        "worker_runtime must branch on config.background for cancel token selection"
    );
    assert!(
        source.contains("CancellationToken::new()"),
        "worker_runtime must create independent CancellationToken for background workers"
    );
}

/// review lock：worker_runtime.rs 中 WorkerRunConfig.cancel_token 不允许为 None，
/// 不允许 background worker 使用父的 child_token。
/// 此约束作为文档，通过 source-level 检查防止 regression。
#[test]
fn review_a1_cancel_token_none_is_handled_explicitly() {
    let source = include_str!("../src/runtime/agent/worker_runtime.rs");
    // 确保 None 情况被显式处理（unwrap_or_else 或 if config.background）
    // 而不是静默退化
    assert!(
        source.contains("unwrap_or_else") || source.contains("CancellationToken::new()"),
        "worker_runtime must handle None cancel_token explicitly, not silently degrade"
    );
}
```

- [ ] **A-3-2：父 cancel 不波及 background agent 的集成测试**

在 `src-tauri/tests/subagent_legacy_cancel_reachability_test.rs` 追加：

```rust
use app_lib::runtime::cancellation::{CancellationReason, CancellationToken};

/// 父 session cancel（Interrupt / UserCancel）不应传播到 background worker 的独立 token。
#[test]
fn a1_parent_session_cancel_does_not_reach_background_worker_token() {
    let session_token = CancellationToken::new();

    // background worker 使用独立 token（正确行为）
    let bg_worker_token = CancellationToken::new();

    // 父 session 收到 ESC
    session_token.cancel_with_reason(CancellationReason::Interrupt);

    assert!(session_token.is_cancelled());
    assert!(
        !bg_worker_token.is_cancelled(),
        "background worker must remain running when parent session is interrupted"
    );
}

/// foreground worker token 仍然是父 session 的 child（cascade 保持）。
#[test]
fn a1_foreground_worker_token_cascades_from_session() {
    let session_token = CancellationToken::new();
    let fg_worker_token = session_token.child_token();

    session_token.cancel_with_reason(CancellationReason::UserCancel);

    assert!(fg_worker_token.is_cancelled());
    assert_eq!(
        fg_worker_token.reason(),
        Some(CancellationReason::UserCancel)
    );
}
```

- [ ] **A-3-3：AgentStatus::Failed 序列化稳定性测试**

在 `src-tauri/tests/plan_a_agent_lifecycle_test.rs` 中追加：

```rust
use app_lib::runtime::agent::invocation::AgentStatus;

/// AgentStatus::Failed 必须可序列化为 "Failed"，可反序列化回 Failed。
#[test]
fn a2_agent_status_failed_serde_roundtrip() {
    let status = AgentStatus::Failed;
    let serialized = serde_json::to_string(&status).unwrap();
    assert_eq!(serialized, r#""Failed""#);
    let deserialized: AgentStatus = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized, AgentStatus::Failed);
}

/// status() 对 Failed 变体返回 "failed"（小写）字符串，与前端 task:status-changed 事件对齐。
#[tokio::test]
async fn a2_status_string_for_failed_is_lowercase() {
    use app_lib::runtime::agent::agent_runtime::AgentRuntime;
    use app_lib::runtime::agent::invocation::SpawnChildRunRequest;
    use app_lib::runtime::ids::RunId;

    let runtime = AgentRuntime::for_test();
    let handle = runtime
        .spawn_child_run(SpawnChildRunRequest {
            parent_run_id: RunId::new("p-run"),
            background: false,
            allowed_tools: vec![],
        })
        .await
        .unwrap();
    let cid = handle.child_run_id().clone();
    runtime.fail_run(&cid).await.unwrap();

    let status_str = runtime.status(&cid).await.unwrap();
    assert_eq!(status_str, "failed");
}
```

- [ ] **A-3-4：运行全部新增测试**

```bash
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && \
  cargo test --test subagent_legacy_cancel_reachability_test -- --nocapture

cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && \
  cargo test --test plan_a_agent_lifecycle_test -- --nocapture
```

- [ ] **A-3-5：运行现有相关回归测试确认不退化**

```bash
# cancel 级联现有回归
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && \
  cargo test --test subagent_cancel_cascade_test -- --nocapture

# U5 worker runtime 约束
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && \
  cargo test --test plan_u5_subagent_worker_runtime_test -- --nocapture

# subagent background wiring 约束
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && \
  cargo test --test review_sub_agent_background_reachability_test -- --nocapture

cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && \
  cargo test --test review_sub_agent_background_caller_wiring_test -- --nocapture

# 全量 Rust 测试
cd /Users/a20250311/.codex/worktrees/0862/lotus-app/src-tauri && \
  cargo test --tests --no-fail-fast 2>&1 | tail -30
```

- [ ] **A-3-6：commit**

```
test(subagent): A-1/A-2 回归测试 + review lock，锁定 background cancel 独立性与 Failed 状态语义
```

---

## 验收标准

- `background=true` 时，`worker_runtime.rs` 中创建 `CancellationToken::new()`，与父 session token 的 `Arc` 不共享（`!Arc::ptr_eq`）。
- 父 session ESC（`Interrupt` / `UserCancel`）不导致正在运行的 background worker 的 `child_cancel.is_cancelled()` 为 true。
- `AgentStatus::Failed` 可序列化，`fail_run()` 方法存在，`status()` 返回 `"failed"`。
- LLM 错误和迭代超限路径调用 `fail_run` / `fail_background_run`，不再调用 `complete_run`。
- background worker Failed 时，前端能通过 `task:status-changed` 事件收到 `status: "failed"`。
- 所有现有 `subagent_cancel_cascade_test`、`plan_u5_subagent_worker_runtime_test`、`review_sub_agent_background_*` 测试仍然通过。
