---
title: Lotus Team Runtime (LTR) 实施计划
status: draft
date: 2026-05-11
references:
  - docs/superpowers/plans/2026-05-10-lotus-team-runtime-plan.md  # 架构方案 v4
for-agentic-workers: |
  本文档是 v4 架构方案的执行细则。每个 Task 包含:文件路径、代码草稿、step-by-step、测试命令。
  执行时强烈建议配合 superpowers:executing-plans 或 subagent-driven-development 跑 task。
---

# Lotus Team Runtime (LTR) 实施计划

> **REQUIRED SUB-SKILL**:用 superpowers:executing-plans 或 superpowers:subagent-driven-development 跑这份计划。Step 用 `- [ ]` 标记。
>
> **架构源**:`docs/superpowers/plans/2026-05-10-lotus-team-runtime-plan.md` v4。任何分歧以 v4 为准。

**Goal**:让任何数字员工(Employee)面对复杂任务时能开 Team,派 2-4 个常驻 Teammate 并行干活、相互通信、共享 task list 协作,Lead 综合产出。

**Architecture**:能力面 1:1 对齐 claude-code-best Agent Teams(B 方案);实现层用 Rust 原生(tokio mpsc / Mutex / select! / watch)替代它的 500ms 文件轮询 + proper-lockfile。

**Tech Stack**:Rust(tokio / async-trait / serde_json / uuid v4)、Tauri 2.x runtime 层(不引入新外部 crate)。

---

## 0. 现状速查表(实施前必读)

v4 §8 列了"需要新增/改动"的模块,但**很多模块已经有部分实现了**。下表给出每个模块的当前状态和实施动作,避免重复造轮子。

| v4 提到的模块 | 当前文件 / 行数 | 现状 | 实施动作 |
|---|---|---|---|
| `runtime/agent/team.rs` | 21 行 stub(只占位 `TeamContext`) | 几乎空 | **P1 重写** |
| `runtime/agent/team_store.rs` | 不存在 | — | **P1 新建** |
| `runtime/agent/name_registry.rs` | 不存在 | — | **P1 新建** |
| `runtime/agent/shared_task_list.rs` | 不存在(逻辑现在散在 `runtime/task/`) | — | **P1 新建**(承载 tokio Mutex claim + blocks 环检测) |
| `runtime/agent/pending_mailbox.rs` | 不存在 | — | **P2 新建**(per-Member tokio mpsc) |
| `runtime/agent/worker_runtime.rs` | 1073 行 | subagent worker 主体,**有 `tauri::AppHandle` 直挂**(`worker_runtime.rs:72` 是当前 review_agent_b_constraints 唯一豁免) | **P1 扩展**:新增 Teammate idle loop 入口,不动 subagent 路径 |
| `runtime/tools/builtin/spawn_subagent.rs` | 461 行 | 已支持 `name` 字段(async path) | **P1 扩展**:加 `team_name` / `employee_id`,判断 `team_name && name` → 派 Teammate;description 动态拼接 EmployeeStore 清单 |
| `runtime/tools/builtin/task_tools.rs` | 387 行 | TaskCreate / TaskUpdate / TaskList / TaskGet 已有,**有 owner 字段**,**有 addBlocks/addBlockedBy 字段但无环检测**,**无 tokio Mutex claim** | **P1 增强**:加 DFS 环检测 + 同进程 claim 原子性 + uuid v4 替换 .highwatermark |
| `runtime/tools/builtin/task_stop.rs` | 79 行 | 已支持按 `task_id`(= agent_id) 取消 async subagent | **P2 复用**:Teammate 强制关闭走它 |
| `runtime/tools/builtin/task_output.rs` | 229 行 | 已支持读 async subagent transcript | 不动 |
| `runtime/task/task_v2_store.rs` | 156 行 | FileTaskV2Store,**用 .highwatermark + integer id**,根目录是 AiJiaHome `tasks/<listId>/` | **P1 重写存储路径**:根目录改为 conversation `tasks/`;id 改用 uuid v4;删 .highwatermark |
| `runtime/agent/async_task_store.rs` | 256 行 | 内存 store,已有 `queue_pending_message` / `drain_pending_messages` per AgentId | **P2 复用 + 扩展**:承载 Teammate 的 pending channel(或换成 tokio mpsc 后保留磁盘审计) |
| `runtime/agent/task_notification.rs` | 275 行 | `TaskNotificationQueue`,**per-process,按 session 分区 drain**,已挂在 chat_turn_driver | **P2 复用**:Teammate → Lead 的 user message 注入也走它,扩展 entry 类型 |
| `runtime/agent/tool_whitelist.rs` | 202 行 | `ALL_AGENT_DISALLOWED` 含 "Agent"(防递归 spawn) | **P1 扩展**:派 Teammate 时禁用 Agent(对齐"Teammate 不可递归 spawn");派 Lead 不禁用 |
| `runtime/agent/output_writer.rs` | 163 行 | append-only JSONL,role / content / error 三字段 | **P2 升级**:加 entry 类型(system_init / user / assistant / tool_use / tool_result / abort) |
| `runtime/employee/store.rs` | 894 行 | EmployeeStore CRUD 完整 | **P1 加只读接口**(对齐 v4 §3.F.1 的 `list_employees` / `get_employee` 语义)+ tool_whitelist 强制校验 |
| `runtime/employee/dispatch_prompt.rs` | 224 行 | 现有派活 prompt 组装 | **P1 加 TEAMMATE_ADDENDUM**(仅 role=Teammate 注入);Lead 不叠 |
| `runtime/chat/chat_turn_driver.rs` | 2396 行 | `run_chat_turn_s4` 主驱动;`drain_and_inject_task_notifications` 已在 turn 起手调用 | **P2 加 Lead idle 触发 A 路径**(turn 结束自检 pending 非空 → 自动续 turn) |
| `runtime/session_runtime.rs` | ~1100 行 | `cancel_session` 已实现 | **P1 加 cleanupSessionTeams 钩子**(对齐 v4 §7.3 4 触发点) |
| `runtime/events.rs` | 167 行 | 17 个 RuntimeEventKind | **P1 加 3 个 variant**:`TeamCreated` / `MemberJoined` / `MemberPhaseChanged`(只发不接,Phase 3 前端再接) |
| `runtime/mcp/runtime_tool.rs` | 65 行 | `McpRuntimeTool::execute` **不感知 ctx.cancellation**(`call_tool` 阻塞) | **P0 加 select cancel + connection cleanup** |
| `RuntimeLlmExecutor` trait | `chat/chat_turn_driver.rs:145` | `compact_summary` 作为 trait method 之一 | **P0 抽到独立 `CompactSummaryClient` trait**,trait 瘦身 |
| `UserScopedPaths::subagent_transcripts_dir` | `storage/user_scoped_paths.rs:62` | user 级扁平路径,async subagent 当前写这里 | **P1 弃用并改写到 conversation 级**(旧路径下数据**不迁移**,直接 deprecated;无产品兼容负担) |

**核心原则**:
1. 每个新文件先写测试再写实现(TDD)。
2. 每个 task 完成立即 commit,commit message 用 conventional commits(`feat:` / `refactor:` / `test:`)。
3. P0 完成前不开 P1;P1 完成前不开 P2(三阶段强依赖)。
4. 改完任何 trait / 类型签名,**全量跑** `cd src-tauri && cargo test review_ --tests --no-fail-fast` 确认架构约束不破。

---

## Phase P0 — MCP 取消接通 + compact_summary 解耦

**目标**:扫清架构债,为 P1 引入 Teammate idle loop 铺路。两个独立子任务,可并行执行。

**完成标志**:
1. MCP 工具在父 token cancel 时 3 秒内中止,子进程进入 disconnect 流程。
2. `RuntimeLlmExecutor` trait 不再包含 `compact_summary`;新增 `CompactSummaryClient` 独立 trait,chat_turn_driver 通过它调用,生产 / 测试两条路径都通。
3. `cargo test --tests --no-fail-fast` 整体不退化。

---

### Task P0.1 — `McpRuntimeTool::execute` 接 `ctx.cancellation`

**Files**:
- Modify:`src-tauri/src/runtime/mcp/runtime_tool.rs`(目前 65 行)
- Modify:`src-tauri/src/runtime/mcp/connection.rs`(`StdioMcpConnection::call_tool` 周边,~330-450 行附近)
- Test(新建):`src-tauri/tests/review_mcp_cancel_propagation_test.rs`

**背景**:目前 `McpRuntimeTool::execute` 直接 `await connection.call_tool(...)`,父 `CancellationToken` cancel 时这个 future 不会被打断,直到 MCP 子进程自然回包(可能数十秒)。要把它包到 `tokio::select!` 里,cancel 分支立即返回 `ToolError::ExecutionFailed("cancelled")` 并触发该 connection 的 best-effort cleanup。

- [ ] **Step 1**:先写失败的回归测试

```rust
// src-tauri/tests/review_mcp_cancel_propagation_test.rs
use std::sync::Arc;
use std::time::Duration;
use serde_json::Value;
use async_trait::async_trait;
use anyhow::Result;

use aijia::runtime::cancellation::{CancellationToken, CancellationReason};
use aijia::runtime::mcp::connection::{McpConnection, McpResult, McpServerConfig, SharedMcpConnection};
use aijia::runtime::mcp::types::McpToolDefinition;
use aijia::runtime::mcp::runtime_tool::McpRuntimeTool;
use aijia::runtime::tools::context::ToolExecutionContext;
use aijia::runtime::tools::RuntimeTool;

/// Stub connection whose `call_tool` future never resolves until the test
/// finishes — simulates an MCP server that hangs.
struct HangingConnection {
    config: McpServerConfig,
}

#[async_trait]
impl McpConnection for HangingConnection {
    async fn connect(&self) -> McpResult<()> { Ok(()) }
    async fn disconnect(&self) -> McpResult<()> { Ok(()) }
    fn is_connected(&self) -> bool { true }
    fn server_name(&self) -> &str { "stub" }
    async fn list_tools(&self) -> McpResult<Vec<McpToolDefinition>> { Ok(vec![]) }
    async fn call_tool(&self, _tool_name: &str, _arguments: Value) -> McpResult<Value> {
        // hang forever
        std::future::pending::<()>().await;
        unreachable!()
    }
    fn config(&self) -> &McpServerConfig { &self.config }
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_runtime_tool_aborts_within_one_second_when_token_cancelled() {
    let config = McpServerConfig {
        name: "stub".into(),
        command: "noop".into(),
        args: vec![],
        env: Default::default(),
        managed_runtime: None,
    };
    let conn: SharedMcpConnection = Arc::new(HangingConnection { config });
    let tool_def = McpToolDefinition {
        server_name: "stub".into(),
        tool_name: "echo".into(),
        description: None,
        input_schema: serde_json::json!({}),
        default_read_only: false,
        default_destructive: false,
    };
    let tool = McpRuntimeTool::new(tool_def, conn);

    let cancel = CancellationToken::new();
    let mut ctx = ToolExecutionContext::for_test("c1", "r1", "tc1");
    ctx.cancellation = cancel.clone();

    let exec = tokio::spawn(async move {
        tool.execute(serde_json::json!({}), ctx).await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    cancel.cancel_with_reason(CancellationReason::UserCancel);

    let result = tokio::time::timeout(Duration::from_secs(1), exec)
        .await
        .expect("must abort within 1s of cancel")
        .expect("task should not panic");

    assert!(result.is_err(), "expected ToolError after cancel");
}
```

- [ ] **Step 2**:跑测试,确认它失败

```bash
cd src-tauri && cargo test --test review_mcp_cancel_propagation_test -- --nocapture
```

Expected:挂 1 秒后 timeout fail,因为现在 `call_tool` 不会被取消。

- [ ] **Step 3**:改造 `McpRuntimeTool::execute` 引入 select cancel

完整替换 `src-tauri/src/runtime/mcp/runtime_tool.rs::execute`:

```rust
async fn execute(
    &self,
    input: Value,
    ctx: ToolExecutionContext,
) -> Result<ToolResult, ToolError> {
    if !self.connection.is_connected() {
        return Err(ToolError::ExecutionFailed(format!(
            "MCP server '{}' is not connected",
            self.connection.server_name()
        )));
    }

    let call_fut = self.connection.call_tool(&self.tool.tool_name, input);
    let cancel = ctx.cancellation.clone();

    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            // Best-effort: tell the connection to drop in-flight request so the
            // subprocess can clean up.  Failures are swallowed — cancel must
            // succeed regardless of cleanup outcome.
            let _ = self.connection.disconnect_on_cancel().await;
            return Err(ToolError::ExecutionFailed(format!(
                "MCP tool '{}' cancelled before completion",
                self.tool.tool_name
            )));
        }
        r = call_fut => r,
    };

    let value = result.map_err(|err| ToolError::ExecutionFailed(err.to_string()))?;

    let content = value.as_str().map(ToOwned::to_owned).unwrap_or_else(|| {
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
    });

    Ok(ToolResult::new(self.definition().id, content, Some(value)))
}
```

并在 `McpConnection` trait 加一个默认实现的 `disconnect_on_cancel`(`connection.rs`):

```rust
#[async_trait]
pub trait McpConnection: Send + Sync {
    // ... 现有方法 ...

    /// Best-effort cleanup called when the in-flight tool call was cancelled.
    /// Default impl just calls `disconnect()`.  Concrete connections may
    /// override to e.g. cancel a pending request id without tearing the
    /// whole subprocess down.
    async fn disconnect_on_cancel(&self) -> McpResult<()> {
        self.disconnect().await
    }
}
```

- [ ] **Step 4**:再跑测试,确认通过

```bash
cd src-tauri && cargo test --test review_mcp_cancel_propagation_test -- --nocapture
```

Expected:PASS,result 是 `Err(ExecutionFailed("MCP tool 'echo' cancelled before completion"))`。

- [ ] **Step 5**:跑全量 MCP 测试,确保没破坏现有行为

```bash
cd src-tauri && cargo test mcp_ --tests --no-fail-fast
cd src-tauri && cargo test review_mcp_ --tests --no-fail-fast
```

Expected:全绿。

- [ ] **Step 6**:commit

```bash
git add src-tauri/src/runtime/mcp/runtime_tool.rs src-tauri/src/runtime/mcp/connection.rs src-tauri/tests/review_mcp_cancel_propagation_test.rs
git commit -m "feat(mcp): propagate ctx.cancellation into McpRuntimeTool::execute

McpRuntimeTool used to await call_tool without listening for cancel,
leaving the parent run waiting until the subprocess finally responded.
Add tokio::select! over (cancel, call_fut) so cancel triggers in-flight
disconnect_on_cancel cleanup and returns a precise error.

Refs: docs/superpowers/plans/2026-05-10-lotus-team-runtime-plan.md §7.2 (MCP subprocess row)"
```

---

### Task P0.2 — 把 `compact_summary` 从 `RuntimeLlmExecutor` 抽到独立 trait

**Files**:
- Modify:`src-tauri/src/runtime/chat/chat_turn_driver.rs`(`RuntimeLlmExecutor` trait 定义在 145 行附近;`compact_summary` 当前是 trait method,323 行附近的 default impl;两个调用点在 1350 / 1474)
- Create:`src-tauri/src/runtime/chat/compact_client.rs`(新文件,放 `CompactSummaryClient` trait)
- Modify:`src-tauri/src/runtime/chat/mod.rs`(暴露新模块)
- Modify:所有 `impl RuntimeLlmExecutor for ...` 处(grep `impl RuntimeLlmExecutor` 找到全部点位)
- Test:`src-tauri/tests/review_compact_summary_trait_isolation_test.rs`(新建)

**背景**:v4 把 compact 与 LLM streaming 解耦,让 Teammate 可以走 compact 而不必绑死同一个 streaming executor。当前 `RuntimeLlmExecutor::compact_summary` 是 trait 方法,把它独立成 `CompactSummaryClient`,chat_turn_driver 通过新字段持有它。

- [ ] **Step 1**:写架构约束测试

```rust
// src-tauri/tests/review_compact_summary_trait_isolation_test.rs
//
// Asserts that `compact_summary` is *not* a method on `RuntimeLlmExecutor`
// — it lives on the standalone `CompactSummaryClient` trait so callers
// that only stream don't have to implement compaction.

use std::path::Path;

#[test]
fn runtime_llm_executor_does_not_contain_compact_summary() {
    let path = Path::new("src/runtime/chat/chat_turn_driver.rs");
    let content = std::fs::read_to_string(path).expect("read chat_turn_driver.rs");

    // Find the trait definition block.
    let trait_start = content
        .find("pub trait RuntimeLlmExecutor")
        .expect("RuntimeLlmExecutor trait must exist");
    // The trait ends at the next top-level `}` after the opening `{`.
    // For a robust check, just assert no `compact_summary` between the
    // trait keyword and a sentinel comment we'll add right after the trait.
    let trait_end_marker = "// END_TRAIT_RuntimeLlmExecutor";
    let trait_end = content[trait_start..]
        .find(trait_end_marker)
        .expect("must mark the end of RuntimeLlmExecutor trait with `// END_TRAIT_RuntimeLlmExecutor`")
        + trait_start;
    let trait_body = &content[trait_start..trait_end];

    assert!(
        !trait_body.contains("fn compact_summary"),
        "compact_summary must be removed from RuntimeLlmExecutor; \
         move it to CompactSummaryClient (runtime/chat/compact_client.rs)"
    );
}

#[test]
fn compact_summary_client_trait_exists() {
    let path = Path::new("src/runtime/chat/compact_client.rs");
    let content = std::fs::read_to_string(path)
        .expect("runtime/chat/compact_client.rs must exist after P0.2");
    assert!(
        content.contains("pub trait CompactSummaryClient"),
        "compact_client.rs must declare `pub trait CompactSummaryClient`"
    );
    assert!(
        content.contains("fn compact_summary"),
        "CompactSummaryClient must declare `compact_summary` method"
    );
}
```

- [ ] **Step 2**:确认测试失败

```bash
cd src-tauri && cargo test --test review_compact_summary_trait_isolation_test -- --nocapture
```

Expected:两个 test 都失败(因为 marker 没加,文件没建)。

- [ ] **Step 3**:新建 `runtime/chat/compact_client.rs`

```rust
//! `CompactSummaryClient` trait — pure compaction interface, isolated from
//! the streaming-oriented `RuntimeLlmExecutor`.
//!
//! Rationale: Teammate idle loops want compaction capability without
//! requiring a full streaming executor.  Splitting the trait lets
//! `chat_turn_driver` and future Teammate runners share the same
//! compaction backend while only the chat driver needs streaming.

use async_trait::async_trait;

use crate::runtime::chat::turn_outcome::TurnError;

#[async_trait]
pub trait CompactSummaryClient: Send + Sync {
    /// Produce a single-string summary that replaces the older half of the
    /// transcript when context is approaching the model's window.
    async fn compact_summary(
        &self,
        conversation_id: &str,
        messages: &[serde_json::Value],
    ) -> Result<String, TurnError>;
}
```

(注:`TurnError` 的实际路径以 `turn_outcome.rs` / `turn_config.rs` 中 `pub use` 为准;实施时如果是 `runtime::chat::TurnError` 这种 re-export,直接走 re-export。)

- [ ] **Step 4**:更新 `runtime/chat/mod.rs`

```rust
// 在 mod 列表合适位置加:
pub mod compact_client;
pub use compact_client::CompactSummaryClient;
```

- [ ] **Step 5**:从 `RuntimeLlmExecutor` 删除 `compact_summary` + 在 trait 结尾加 marker

打开 `runtime/chat/chat_turn_driver.rs`:
1. 找到 `pub trait RuntimeLlmExecutor`(约 145 行),把里面的 `compact_summary` 方法(约 323 行那个 default impl + trait method 签名)整段删掉。
2. 在 trait 闭合 `}` 后**立刻**加一行 marker:
   ```rust
   // END_TRAIT_RuntimeLlmExecutor — sentinel for review_compact_summary_trait_isolation_test
   ```

- [ ] **Step 6**:改 `RuntimeChatTurnDriver` 持有可选的 `CompactSummaryClient`

```rust
// chat_turn_driver.rs:354 附近
#[derive(Clone)]
pub struct RuntimeChatTurnDriver {
    query_engine: QueryEngine,
    event_bus: RuntimeEventBus,
    llm_executor: Option<Arc<dyn RuntimeLlmExecutor>>,
    pending_permission_control_plane: Option<Arc<dyn PendingPermissionControlPlane>>,
    pending_interaction_control_plane: Option<Arc<dyn PendingInteractionControlPlane>>,
    task_notification_queue: Option<Arc<TaskNotificationQueue>>,
    /// Compaction backend.  Decoupled from llm_executor (P0.2) so
    /// Teammate idle loops can share the same compactor without
    /// requiring streaming.  `None` falls back to a no-op (legacy / tests).
    compact_client: Option<Arc<dyn CompactSummaryClient>>,
}
```

加 builder:

```rust
impl RuntimeChatTurnDriver {
    pub fn with_compact_client(mut self, client: Arc<dyn CompactSummaryClient>) -> Self {
        self.compact_client = Some(client);
        self
    }
}
```

- [ ] **Step 7**:改两处 `compact_summary` 调用点

`chat_turn_driver.rs` 约 1350 行和 1474 行,把:

```rust
executor
    .compact_summary(conversation_id.as_str(), &messages)
    .await?;
```

改为:

```rust
let summary = match self.compact_client.as_ref() {
    Some(client) => client.compact_summary(conversation_id.as_str(), &messages).await?,
    None => {
        log::warn!("[chat_turn_driver] no CompactSummaryClient configured; skipping compaction");
        String::new()
    }
};
```

(具体上下文以代码现场为准 —— 替换 `executor.compact_summary(...)` 这一类调用,executor 不再承担此职责。)

- [ ] **Step 8**:改所有 `impl RuntimeLlmExecutor` 实现处

grep 找全:

```bash
cd src-tauri && grep -rn "impl RuntimeLlmExecutor" src/ tests/
```

每处删掉 `compact_summary` 的 impl 方法体(它已不在 trait 上)。

如果有些 impl 实际**也实现了** compact_summary(生产 executor 是其中一个),把那段实现搬到新 struct `OpenAiCompactSummaryClient`(或类似名字,具体见 `llm/`)上,impl `CompactSummaryClient` for it,并在 `lib.rs` 或 `runtime/dependencies/` 装配处把它 inject 给 driver。

> **执行提示**:如果发现某个 executor 的 compact_summary 实际就是调同一个 LLM gateway,做法是:
> 1. 把那段实现复制到 `llm/compact_summary_client.rs`(新建)做成 `OpenAiCompactSummaryClient { gateway: Arc<LlmGateway> }`。
> 2. 删 executor impl 里的对应方法体。
> 3. 在 `lib.rs` 中 `SessionRuntime` / `RuntimeChatTurnDriver` 装配处 `.with_compact_client(Arc::new(OpenAiCompactSummaryClient::new(gateway.clone())))`。

- [ ] **Step 9**:跑新测试 + 全 chat 测试

```bash
cd src-tauri && cargo test --test review_compact_summary_trait_isolation_test -- --nocapture
cd src-tauri && cargo test review_autocompact --tests --no-fail-fast
cd src-tauri && cargo test chat_runtime --tests --no-fail-fast
```

Expected:全绿。

- [ ] **Step 10**:跑全量 review_ 与 cargo build,确认没破坏

```bash
cd src-tauri && cargo build
cd src-tauri && cargo test review_ --tests --no-fail-fast
```

Expected:全绿,无 warning 新增。

- [ ] **Step 11**:commit

```bash
git add -A
git commit -m "refactor(chat): split compact_summary into its own CompactSummaryClient trait

RuntimeLlmExecutor mixed streaming (the hot path) with compaction (a
periodic rewrite).  Teammate idle loops want compaction without
necessarily holding a streaming executor — split the trait so callers
opt into each capability independently.

- New trait: runtime/chat/compact_client.rs::CompactSummaryClient
- RuntimeChatTurnDriver gains optional compact_client field + builder
- All existing impl RuntimeLlmExecutor sites drop the method
- Added review_compact_summary_trait_isolation_test as architecture guard

Refs: docs/superpowers/plans/2026-05-10-lotus-team-runtime-plan.md §2.1 (P0 row 2)"
```

---

### P0 完成检查

- [ ] `cd src-tauri && cargo test review_ --tests --no-fail-fast` 全绿
- [ ] `cd src-tauri && cargo test --tests --no-fail-fast` 整体不退化(允许 unrelated flaky test 提前标记,但不能由本期引入新 failure)
- [ ] `pnpm test` 前端不退化
- [ ] 两个 review_ 新测试在 `tests/` 下存在并 PASS
- [ ] git log 看到至少 2 个 P0 commit

P0 通过 → 进入 P1。

---

## P1 阶段 — Team 实体 + Teammate 派活 + 共享 Task 增强

> 目标:把 v4 §3.A(Team 生命周期)+ §3.B(Agent 创建)+ §3.D(共享 Task)+ §3.F(Subagent 派活扩展)从能力面落到代码层。结束时 LLM 能调 `TeamCreate / spawn_subagent(team_name, employee_id) / TaskCreate(claim) / TeamDelete`,但**还没有** SendMessage / 结构化消息 / Lead idle 续 turn —— 这部分留 P2。
>
> P1 不做:SendMessage 工具、StructuredMessage、广播、Lead idle 触发、is_async flag、TaskStop、task-notification、TEAMMATE_ADDENDUM。这 8 项全留 P2。

### P1 Task 总览

| # | 任务 | 估时 | 依赖 |
|---|---|---|---|
| P1.1 | `runtime/agent/team.rs` 重写:Team 实体 + TeamRegistry(per-Session) | 1.5h | P0.2 |
| P1.1b | `team.json` 持久化(Team 内存态磁盘镜像) | 1h | P1.1 |
| P1.2 | `AgentNameRegistry`(name → AgentId,per-SessionId) | 1h | P1.1 |
| P1.3 | 扩展 `spawn_subagent`:加 `name` / `team_name` / `employee_id` 入参 + Employee 解析桥 | 2h | P1.1 / P1.2 |
| P1.4 | 强制必含工具校验:`SendMessage / TaskList / TaskGet` 缺一即派活失败 | 0.5h | P1.3 |
| P1.5 | 共享 Task 增强:`TaskClaim` 工具 + `addBlocks` 环检测 + Task V2 根目录迁到 `users/{scope}/conversations/{conv}/tasks/` | 2h | (无,与 P1.1 并行) |
| P1.6 | Teammate idle loop:扩展 `worker_runtime.rs` + transcript 路径分 `teammates/` vs `subagents/` + `.meta.json` sidecar | 3h | P1.3 / P1.5 |
| P1.7 | `TeamCreate` / `TeamDelete` 工具 + Lead 进入"Team 模式"判定 | 1h | P1.1 / P1.1b |
| P1.8 | Session 生命周期 cleanup:`lib.rs` 加 app-close hook,清空 TeamRegistry / AgentNameRegistry / pending channel + 删 team.json | 1h | P1.1b / P1.2 |

P1 阶段总估时:13h;关键路径:P1.1 → P1.1b → P1.3 → P1.6 → P1.7 → P1.8。P1.5 可并行。

---

### P1.1 — `runtime/agent/team.rs` 重写

**目标**:把 21 行 stub 换成生产实现:`Team` 实体(SessionId 唯一,members:Vec<Member>,limit=4,name index)+ `TeamRegistry`(`HashMap<SessionId, Arc<Mutex<Team>>>`,跟 SessionRuntime 同生命周期)。

**文件**:
- 重写 `src-tauri/src/runtime/agent/team.rs`
- 新增 `src-tauri/tests/review_team_registry_session_isolation_test.rs`

**代码草稿**:

```rust
// runtime/agent/team.rs
use crate::runtime::ids::{AgentId, SessionId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub const MAX_TEAMMATES: usize = 4;

#[derive(Debug, Clone)]
pub enum MemberRole {
    Lead,
    Teammate {
        employee_id: String,
        spawned_by: AgentId,    // 总是 Lead
    },
}

#[derive(Debug, Clone)]
pub struct Member {
    pub agent_id: AgentId,
    pub name: String,           // team-lead / 用户起的 Teammate 名
    pub role: MemberRole,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_active_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug)]
pub struct Team {
    pub session_id: SessionId,
    pub team_name: String,                // dispatch 时由 Lead LLM 指定;default `team-{session8}`
    pub lead: Member,
    pub teammates: Vec<Member>,           // 硬限 4
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(thiserror::Error, Debug)]
pub enum TeamError {
    #[error("max teammate limit reached (4)")]
    MaxTeammateLimitReached,
    #[error("name already taken in this team: {0}")]
    NameAlreadyTaken(String),
    #[error("team not found for session {0:?}")]
    TeamNotFound(SessionId),
    #[error("team already exists for session {0:?}")]
    TeamAlreadyExists(SessionId),
}

impl Team {
    pub fn new(session_id: SessionId, lead: Member, team_name: String) -> Self {
        let now = chrono::Utc::now();
        Self { session_id, team_name, lead, teammates: Vec::new(), created_at: now }
    }

    pub fn add_teammate(&mut self, m: Member) -> Result<(), TeamError> {
        if self.teammates.len() >= MAX_TEAMMATES {
            return Err(TeamError::MaxTeammateLimitReached);
        }
        if self.lead.name == m.name || self.teammates.iter().any(|t| t.name == m.name) {
            return Err(TeamError::NameAlreadyTaken(m.name));
        }
        self.teammates.push(m);
        Ok(())
    }

    pub fn members(&self) -> impl Iterator<Item = &Member> {
        std::iter::once(&self.lead).chain(self.teammates.iter())
    }

    pub fn find_by_name(&self, name: &str) -> Option<&Member> {
        self.members().find(|m| m.name == name)
    }
}

#[derive(Debug, Default)]
pub struct TeamRegistry {
    teams: Mutex<HashMap<SessionId, Arc<Mutex<Team>>>>,
}

impl TeamRegistry {
    pub fn new() -> Arc<Self> { Arc::new(Self::default()) }

    pub async fn create(&self, session_id: SessionId, lead: Member, team_name: String) -> Result<Arc<Mutex<Team>>, TeamError> {
        let mut g = self.teams.lock().await;
        if g.contains_key(&session_id) {
            return Err(TeamError::TeamAlreadyExists(session_id));
        }
        let team = Arc::new(Mutex::new(Team::new(session_id.clone(), lead, team_name)));
        g.insert(session_id, team.clone());
        Ok(team)
    }

    pub async fn get(&self, session_id: &SessionId) -> Option<Arc<Mutex<Team>>> {
        self.teams.lock().await.get(session_id).cloned()
    }

    pub async fn delete(&self, session_id: &SessionId) -> Option<Arc<Mutex<Team>>> {
        self.teams.lock().await.remove(session_id)
    }
}
```

**Step-by-step**:

- [ ] **Step 1**:删 stub,粘上面的草稿(包含 `MAX_TEAMMATES = 4`、Team / Member / TeamRegistry 三个类型)。
- [ ] **Step 2**:在 `runtime/agent/mod.rs` 里 `pub mod team;` 暴露,确认 `pub use team::{Team, TeamRegistry, Member, MemberRole, TeamError, MAX_TEAMMATES};`。
- [ ] **Step 3**:在 `RuntimeHost`(或等价 host trait)上加 `fn team_registry(&self) -> Arc<TeamRegistry>`。在 `lib.rs` 启动时 `app.manage(TeamRegistry::new())`,host 实现层注入。
- [ ] **Step 4**:写 `tests/review_team_registry_session_isolation_test.rs` 包含 4 个意图:
  - 不同 SessionId 之间 Team 互不可见
  - 同 SessionId 重复 create → TeamAlreadyExists
  - 第 5 个 add_teammate → MaxTeammateLimitReached
  - 同名 add_teammate → NameAlreadyTaken
- [ ] **Step 5**:`cargo build` + `cargo test review_team_registry --no-fail-fast`,全绿。
- [ ] **Step 6**:commit。

```bash
git add -A
git commit -m "feat(team): introduce Team / Member / TeamRegistry with hard limit of 4 teammates

Replaces the 21-line stub.  Team is per-SessionId, with one Lead +
≤4 Teammates.  Names are unique inside a team.  TeamRegistry is held
on RuntimeHost; sessions delete their team on shutdown (P1.8).

Refs: 2026-05-10 plan §4.1, §4.4"
```

---

### P1.1b — `team.json` 持久化(Team 内存态磁盘镜像)

**目标**:对齐 v4 §8.4 持久化布局,Team 创建后必须把成员列表写到 `~/.renlijia/users/{scope_key}/conversations/{conv_id}/team.json`,作为 Teammate 第一 turn `team_context` attachment(P2.3b)读取的入口,以及 LLM 自己 Read 的入口。**主路径仍是 TeamRegistry 内存态**,team.json 是从动态镜像。

**写时机**:
- TeamCreate(P1.7)→ 写一次
- spawn_subagent(team_name=...) 成功 add_teammate(P1.3)→ 重写
- TeamDelete(P1.7)→ 删文件
- Teammate cleanup(idle loop 退出)→ 重写

**`team.json` schema**(对齐 §4.1 + §8.4):

```json
{
  "team_name": "research-team",
  "session_id": "...",
  "created_at": "2026-05-11T10:00:00Z",
  "lead": {
    "agent_id": "...",
    "name": "team-lead",
    "role": "lead",
    "created_at": "..."
  },
  "teammates": [
    {
      "agent_id": "...",
      "name": "researcher",
      "role": "teammate",
      "employee_id": "xiaoyan",
      "spawned_by": "{lead_agent_id}",
      "created_at": "...",
      "last_active_at": "..."
    }
  ]
}
```

**文件**:
- 改 `src-tauri/src/runtime/agent/team.rs`(加 `persist_to_disk` / `delete_from_disk` 方法)
- 新增 `src-tauri/tests/team_json_persistence_test.rs`

**Step-by-step**:

- [ ] **Step 1**:`Team` 加 `serde::Serialize`(注意 `Mutex<Team>` 序列化时要 lock 取快照,不能直接 derive)。建议加独立 `TeamSnapshot` DTO:

```rust
#[derive(Serialize, Deserialize)]
pub struct TeamSnapshot {
    pub team_name: String,
    pub session_id: SessionId,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub lead: MemberSnapshot,
    pub teammates: Vec<MemberSnapshot>,
}

impl From<&Team> for TeamSnapshot { /* ... */ }
```

- [ ] **Step 2**:`TeamRegistry` 加 `persist(session_id, scope_paths)` 方法:
  - 锁 Team → snapshot → `runtime::employee::store::write_atomic(team_json_path, &snapshot)`
  - 路径:`scope_paths.conversations_dir().join(&conv_id).join("team.json")`
- [ ] **Step 3**:在 P1.7 TeamCreate / TeamDelete 末尾、P1.3 add_teammate 成功后、P1.6 cleanup_teammate 末尾,各调一次 `persist`(或 `delete`)。
- [ ] **Step 4**:写 `team_json_persistence_test.rs`,4 case:
  - TeamCreate 后 team.json 在期望路径下且含 lead.name == "team-lead"
  - add_teammate 后 team.json teammates 数组 +1
  - Teammate cleanup 后 team.json teammates 数组 -1
  - TeamDelete 后 team.json 不存在
- [ ] **Step 5**:cargo test 绿。
- [ ] **Step 6**:commit。

```bash
git add -A
git commit -m "feat(team): persist Team snapshot to conversations/{id}/team.json

Memory-of-record stays the TeamRegistry/Mutex<Team>; team.json is a
write-through mirror used by Teammate boot attachments (P2.3b) and
audit tooling.  Atomic write via write_atomic.  Updated on every
membership change and deleted on TeamDelete.

Refs: 2026-05-10 plan §8.4"
```

---

### P1.2 — `AgentNameRegistry`(name → AgentId,per-SessionId)

**目标**:LTR 的 SendMessage 按 name 寻址,需要一份 SessionId 范围内的 `name → AgentId` 映射。Team.find_by_name 已经能从 Team 里找,但 Team 里只有 Lead+Teammate;async subagent 也要能被点对点寻址,所以单独维护一份 registry。

**文件**:
- 新增 `src-tauri/src/runtime/agent/name_registry.rs`
- 改 `runtime/agent/mod.rs` 暴露

**代码草稿**:

```rust
// runtime/agent/name_registry.rs
use crate::runtime::ids::{AgentId, SessionId};
use std::collections::HashMap;
use tokio::sync::Mutex;

#[derive(thiserror::Error, Debug)]
pub enum NameRegistryError {
    #[error("name `{0}` already registered in session")]
    Duplicate(String),
}

#[derive(Debug, Default)]
pub struct AgentNameRegistry {
    by_session: Mutex<HashMap<SessionId, HashMap<String, AgentId>>>,
}

impl AgentNameRegistry {
    pub fn new() -> std::sync::Arc<Self> { std::sync::Arc::new(Self::default()) }

    pub async fn register(&self, session: &SessionId, name: &str, id: AgentId) -> Result<(), NameRegistryError> {
        let mut g = self.by_session.lock().await;
        let m = g.entry(session.clone()).or_default();
        if m.contains_key(name) {
            return Err(NameRegistryError::Duplicate(name.into()));
        }
        m.insert(name.into(), id);
        Ok(())
    }

    pub async fn resolve(&self, session: &SessionId, name: &str) -> Option<AgentId> {
        self.by_session.lock().await.get(session).and_then(|m| m.get(name).cloned())
    }

    pub async fn unregister(&self, session: &SessionId, name: &str) {
        if let Some(m) = self.by_session.lock().await.get_mut(session) {
            m.remove(name);
        }
    }

    /// Session 结束时清整张表,P1.8 调用。
    pub async fn drop_session(&self, session: &SessionId) {
        self.by_session.lock().await.remove(session);
    }

    pub async fn names_in_session(&self, session: &SessionId) -> Vec<String> {
        self.by_session.lock().await.get(session).map(|m| m.keys().cloned().collect()).unwrap_or_default()
    }
}
```

**Step-by-step**:

- [ ] **Step 1**:新增文件,粘草稿。
- [ ] **Step 2**:`runtime/agent/mod.rs` 暴露 `pub mod name_registry; pub use name_registry::*;`。
- [ ] **Step 3**:`RuntimeHost` 加 `fn agent_names(&self) -> Arc<AgentNameRegistry>`。`lib.rs` 启动时 `app.manage(AgentNameRegistry::new())`。
- [ ] **Step 4**:写 `tests/agent_name_registry_test.rs`,3 个 case:
  - 不同 session 同名互不冲突
  - 同 session 重复注册 → Duplicate
  - drop_session 后 resolve 返回 None
- [ ] **Step 5**:`cargo test agent_name_registry --no-fail-fast` 绿。
- [ ] **Step 6**:commit。

```bash
git add -A
git commit -m "feat(agent): per-session AgentNameRegistry for name-addressed routing

Required by SendMessage (P2) and by Team.find_by_name in dispatch.
Session-scoped so the same friendly name (e.g. \"researcher\") can be
reused across unrelated chats.

Refs: 2026-05-10 plan §3.C, §4.4"
```

---

### P1.3 — 扩展 `spawn_subagent`:加 name / team_name / employee_id + Employee 桥

**目标**:让 Lead LLM 通过 `Agent(name=..., team_name=..., employee_id=...)` 派 Teammate。已有 `spawn_subagent` 工具(支持 `subagent_type` 走 AgentRegistry),要补两条解析路径:

1. `employee_id` 入参非空 → 走 `EmployeeStore::get(id)`,取该员工的 system prompt + tool_whitelist 当 Teammate 起 idle loop。
2. `team_name` 入参非空 → 该 spawn 是"派 Teammate",必须先存在 Team(SessionId 维度);否则报错"请先 TeamCreate"。
3. `name` 入参非空 → 写入 AgentNameRegistry;为空时走旧 anonymous AgentId 路径(保持向后兼容)。

**文件**:
- 改 `src-tauri/src/runtime/tools/builtin/spawn_subagent.rs`
- 改 `src-tauri/src/runtime/employee/store.rs`(只读接口,见 P1.3-Step 4)
- 新增 `src-tauri/tests/spawn_teammate_via_employee_test.rs`

**入参 schema 变更**:

```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SpawnSubagentArgs {
    // 已有
    pub subagent_type: Option<String>,    // 显式 agent type;或留空走 employee_id
    pub prompt: String,
    pub description: Option<String>,
    pub model: Option<String>,
    // 新增
    pub name: Option<String>,             // 友好名;Teammate 必填,async subagent 选填
    pub team_name: Option<String>,        // 非空 → 派 Teammate(必须 Team 已存在)
    pub employee_id: Option<String>,      // 非空 → 用 Employee 的 prompt + 白名单
}
```

**冲突规则**:

| subagent_type | employee_id | 处理 |
|---|---|---|
| Some | Some | 入参冲突,直接报错 `ConflictingAgentSource` |
| Some | None | 走 AgentRegistry(向后兼容老路径) |
| None | Some | 走 EmployeeStore |
| None | None | 入参缺失,报错 `MissingAgentSource` |

**Step-by-step**:

- [ ] **Step 1**:在 `runtime/employee/store.rs` 加只读接口(纯读 EmployeeRecord;不要碰加载机制 — 这是独立工作流的事):

```rust
impl EmployeeStore {
    pub fn get_readonly(&self, id: &str) -> Option<EmployeeRecord> { /* clone from cache */ }
    pub fn list_readonly(&self) -> Vec<EmployeeRecord> { /* clone */ }
}
```

- [ ] **Step 2**:在 `RuntimeHost` 暴露 `fn employees(&self) -> Arc<EmployeeStore>`(已有的话跳过)。
- [ ] **Step 3**:在 `spawn_subagent.rs::execute` 入口处先做 source 解析:

```rust
let source = match (&args.subagent_type, &args.employee_id) {
    (Some(_), Some(_)) => return Err(ToolError::msg("subagent_type and employee_id are mutually exclusive")),
    (None, None) => return Err(ToolError::msg("either subagent_type or employee_id is required")),
    (Some(t), None) => AgentSource::Registry(t.clone()),
    (None, Some(eid)) => AgentSource::Employee(eid.clone()),
};
```

- [ ] **Step 4**:`team_name` 解析:

```rust
let team_handle: Option<Arc<Mutex<Team>>> = match args.team_name.as_deref() {
    Some(_) => {
        let reg = ctx.team_registry();
        let session_id = ctx.session_id().clone();
        let t = reg.get(&session_id).await
            .ok_or_else(|| ToolError::msg("no team in this session — call TeamCreate first"))?;
        Some(t)
    }
    None => None,
};
```

- [ ] **Step 5**:解析 system prompt + tool_whitelist:

```rust
let (sys_prompt, tool_whitelist, model) = match &source {
    AgentSource::Registry(t) => /* 走老路径 */,
    AgentSource::Employee(eid) => {
        let e = ctx.employees().get_readonly(eid).ok_or_else(|| ToolError::msg(format!("employee not found: {eid}")))?;
        (e.system_prompt.clone(), e.tool_whitelist.clone(), e.model.clone())
    }
};
```

- [ ] **Step 6**:`name` 注册到 AgentNameRegistry(team Teammate 必填,async subagent 选填):

```rust
let agent_id = AgentId::new();
if let Some(name) = &args.name {
    ctx.agent_names().register(ctx.session_id(), name, agent_id.clone()).await
        .map_err(|e| ToolError::msg(e.to_string()))?;
}
// 派 Teammate 走 idle loop;否则走老的 fire-and-forget async 路径
if let Some(team) = team_handle {
    spawn_teammate_idle_loop(team, agent_id.clone(), name.clone().unwrap_or_else(|| agent_id.short()), source, sys_prompt, tool_whitelist, model, ctx).await?;
} else {
    spawn_async_subagent(/* 老路径 */).await?;
}
```

> `spawn_teammate_idle_loop` 在 P1.6 实现,这里先留 stub `unimplemented!("see P1.6")` 让 P1.3 单独可编译——但**测试**要等 P1.6 才能跑。
>
> 可选:这一 step 直接调一个 `runtime/agent/spawn_teammate.rs::stub_spawn_teammate_idle_loop` 占位函数;P1.6 把内部填实即可。建议这么做以保持 P1.3 自身可独立 commit。

- [ ] **Step 7**:Tool description 更新(对齐 §3.B 与 §5.9 的协作引导文案,中文版,1:1 对齐 claude-code-best 风格):

> 文案要点:`name` 必填于 Team Teammate 派活;`team_name` 用于把这个 subagent 加入指定 Team;`employee_id` 用于派一个数字员工当 Teammate;3 项配合表 + 何时该派 Teammate 而不是 async subagent 的判断指南。

- [ ] **Step 8**:写 `tests/spawn_teammate_via_employee_test.rs`,4 个 case:
  - 互斥校验:`subagent_type=foo + employee_id=bar` 返回 ConflictingAgentSource
  - Team 不存在:`team_name=foo + employee_id=bar` 但没先 TeamCreate → 报错
  - 名字重复:同 session 同名两次 → 第二次报 NameRegistry::Duplicate
  - happy path:TeamCreate 后用 employee_id 派一个 Teammate,Team.teammates.len() == 1 + AgentNameRegistry 中可 resolve

> P1.6 完成前,这些测试可能因 idle_loop stub 而 panic;暂时给测试加 `#[ignore = "depends on P1.6"]`,P1.6 完成时再去掉。

- [ ] **Step 9**:`cargo build` 全绿。
- [ ] **Step 10**:commit。

```bash
git add -A
git commit -m "feat(spawn_subagent): support name/team_name/employee_id for Teammate dispatch

The Lead LLM creates a Teammate via Agent(name=..., team_name=...,
employee_id=...).  Source resolution distinguishes:
  - subagent_type → AgentRegistry (legacy async path)
  - employee_id   → EmployeeStore (Teammate path, idle-loop)
The two are mutually exclusive.  team_name is non-empty → idle loop;
empty → legacy fire-and-forget.

Idle loop body itself is stubbed in this commit and filled in P1.6.

Refs: 2026-05-10 plan §3.B, §3.F"
```

---

### P1.4 — 强制必含工具校验:`SendMessage / TaskList / TaskGet`

**目标**:Employee 在 LTR 模式下,如果其 `tool_whitelist` 缺这 3 个,P1.3 派 Teammate 时直接拒绝。这是 v4 §8.2 的强制项。注意 `SendMessage` 工具本身在 P2 才实装,但**校验逻辑** P1 就要写好(校验代码不依赖工具实现存在,只比较字符串)。

**文件**:
- 改 `src-tauri/src/runtime/tools/builtin/spawn_subagent.rs`(在 P1.3 解析 source=Employee 之后插入校验)
- 新增常量 `src-tauri/src/runtime/agent/required_tools.rs`(暴露 3 个名字以便测试 + 将来如增加可统一改)

**代码草稿**:

```rust
// runtime/agent/required_tools.rs
pub const REQUIRED_TEAMMATE_TOOLS: &[&str] = &[
    "SendMessage",
    "TaskList",
    "TaskGet",
];

pub fn missing_required(whitelist: &[String]) -> Vec<&'static str> {
    REQUIRED_TEAMMATE_TOOLS.iter()
        .copied()
        .filter(|t| !whitelist.iter().any(|w| w == t))
        .collect()
}
```

**Step-by-step**:

- [ ] **Step 1**:新增 required_tools.rs,在 mod.rs 暴露。
- [ ] **Step 2**:在 spawn_subagent 解析 Employee 后:

```rust
let missing = required_tools::missing_required(&tool_whitelist);
if !missing.is_empty() {
    return Err(ToolError::msg(format!(
        "employee `{}` cannot be a teammate — missing required tools: {:?}. \
         Add these to its tool_whitelist (or fix the employee profile).",
        eid, missing
    )));
}
```

- [ ] **Step 3**:写 `tests/teammate_required_tools_test.rs`,3 case:
  - whitelist 缺 SendMessage → 报错且错误消息含 "SendMessage"
  - whitelist 缺 TaskList + TaskGet → 报错且消息含两者
  - whitelist 含 3 个 → 通过
- [ ] **Step 4**:cargo test 绿。
- [ ] **Step 5**:commit。

```bash
git add -A
git commit -m "feat(spawn_subagent): enforce required-tool whitelist for Teammate dispatch

A Teammate must have SendMessage, TaskList and TaskGet in its
tool_whitelist; otherwise dispatch fails with a clear remediation
message.  Validation is name-based and does not depend on the tools
actually being registered yet (P2 ships SendMessage).

Refs: 2026-05-10 plan §8.2"
```

---

### P1.5 — 共享 Task 增强:`TaskClaim` + `addBlocks` 环检测 + Task V2 根目录迁到 conversation 级

**目标**:对齐 v4 §3.D,共享 Task list 是 Lead/Teammate 协作的核心载体。当前 task_tools.rs 已有 TaskCreate/Update/List/Get(含 owner/addBlocks/addBlockedBy),但缺:

1. `TaskClaim`:Teammate 自主认领一条 owner 为空 / 或 owner = "*" 的 task,把 owner 改成自己。
2. `addBlocks` / `addBlockedBy` 环检测:DAG 防御,防止 LLM 串环死锁。
3. Task V2 当前根目录是 `~/.renlijia/tasks/`(全局),需要迁到 `~/.renlijia/users/{scope_key}/conversations/{conv_id}/tasks/`(per-conversation)以做到 Session 删除时连带 GC。`scope_key = t_{tenant}__u_{user}`,通过现有 `UserScopedPaths::conversations_dir()` 取。**之前没什么数据,直接改路径,不做 migration。**

**文件**:
- 改 `src-tauri/src/runtime/tools/builtin/task_tools.rs`
- 改 `src-tauri/src/runtime/tasks/store.rs`(若是这个名字;实际名以仓库为准)
- 新增 `src-tauri/tests/task_claim_test.rs`
- 新增 `src-tauri/tests/task_blocks_cycle_detection_test.rs`
- 改 `src-tauri/tests/review_task_storage_per_conv_test.rs`(确认新存储路径)

**Step-by-step**:

- [ ] **Step 1**:**路径迁移**(独立 commit):
  - 在 task store 里把根目录从 `home.tasks_dir()` 改成 `user_scoped.conversations_dir().join(&conv_id).join("tasks")`(走 `UserScopedPaths` 自动按租户隔离)。
  - 老目录 `~/.renlijia/tasks/` 不再读不再写;**不写迁移代码**。
  - 写 `review_task_storage_per_conv_test.rs`:create task → 文件应出现在 `conv_dir/{conv}/tasks/`。

```bash
git add -A
git commit -m "refactor(tasks): scope V2 task storage to per-conversation directory

Tasks are conversation-scoped artifacts; storing them under a global
~/.renlijia/tasks/ blocked Session-level GC and confused multi-session
debugging.  Old path is dropped without migration (no production data
yet)."
```

- [ ] **Step 2**:**TaskClaim 工具**:
  - 新增 `task_claim.rs`(或在 task_tools.rs 加一个 RuntimeTool impl)。
  - 入参 `task_id`。
  - 行为:读 task → 若 `owner` 为 None 或为 `"*"`,把 owner 改成 `ctx.agent_name().or_else(|| ctx.agent_id().to_string())`,持久化,返回新 task。否则报 `AlreadyClaimed`。
  - 在 `register_builtin_tools` 注册。
  - 写 `task_claim_test.rs`:owner=None happy → claim 成功;owner=已被认领 → AlreadyClaimed;owner="*" → 可被任何 agent 抢占。

- [ ] **Step 3**:**addBlocks 环检测**:
  - 在 task_tools::TaskUpdate 里 `addBlocks` / `addBlockedBy` 写入前,做一次 DFS:从新 block 关系出发,检查是否回到自己。
  - 检测到 → 拒绝该 update,返回 `CyclicBlockingDependency { task_id, cycle: Vec<TaskId> }`。
  - 写 `task_blocks_cycle_detection_test.rs`:A blocks B,B blocks C,C blocks A → 第三步报错;线性 A→B→C 通过;自指 A blocks A 报错。

- [ ] **Step 4**:`cargo test --tests --no-fail-fast` 全绿。
- [ ] **Step 5**:commit(分两条更清晰):

```bash
git add -A
git commit -m "feat(tasks): add TaskClaim tool for Teammate-driven task pull

Teammates can claim tasks whose owner is None or '*'.  Owner once set
is sticky (subsequent claims fail).  Lead can also claim its own tasks
to make ownership explicit.

Refs: 2026-05-10 plan §3.D"
git add -A
git commit -m "feat(tasks): cycle detection in addBlocks/addBlockedBy

Naive DFS from the proposed new edge.  Rejects with structured
CyclicBlockingDependency error including the cycle path.  Prevents the
LLM from wedging its own pipeline.

Refs: 2026-05-10 plan §3.D"
```

---

### P1.6 — Teammate idle loop:扩展 `worker_runtime.rs`

**目标**:把 P1.3 stub 里的 `spawn_teammate_idle_loop` 填实。**不新建 `teammate_runtime.rs`** — 在已有的 `worker_runtime.rs` 里加一个新 mode(`WorkerMode::TeammateIdle`)。这是 v4 §8.1 显式的决定。

**Idle loop 行为**:

```
spawn 完成 → 进 select! 等:
  - 自己 inbox 收到 SendMessage          → 注入 LLM history,跑 turn,回收事件
  - shutdown_request 结构化消息          → LLM 决定是否关闭(P2)
  - TaskStop 来的强 cancel               → drop transcript writer,unregister name,delete from team
  - 共享 task 投递的 task-notification   → 注入 user message(P2)
  - cancellation token tripped           → 同 TaskStop
  - 60s 心跳                             → 更新 last_active_at(team.rs)
```

P1 阶段只做"骨架 select! + cancellation + heartbeat + 占位 inbox channel"。SendMessage / shutdown_request / task-notification 在 P2 接入。

**文件**:
- 改 `src-tauri/src/runtime/agent/worker_runtime.rs`
- 改 `src-tauri/src/runtime/agent/inbox.rs`(已存在,加 channel kind 即可,不新建文件)
- 改 `src-tauri/src/runtime/agent/output_writer.rs`(transcript 写路径按 mode 分目录 + 写 `.meta.json` sidecar)
- 改 `src-tauri/src/storage/user_scoped_paths.rs`(或等价文件,加 `teammates_dir(conv_id)` / `subagents_dir(conv_id)` getter,弃用旧的 `subagent_transcripts_dir`)
- 新增 `src-tauri/tests/teammate_idle_loop_skeleton_test.rs`
- 新增 `src-tauri/tests/transcript_path_routing_test.rs`(verify teammates/ vs subagents/ 路径区分)

**Transcript 路径规范**(对齐 v4 §8.4):

| WorkerMode | transcript 路径 | sidecar 路径 |
|---|---|---|
| `AsyncOneShot` | `users/{scope}/conversations/{conv}/subagents/agent-{id}.jsonl` | `subagents/agent-{id}.meta.json` |
| `TeammateIdle` | `users/{scope}/conversations/{conv}/teammates/agent-{id}.jsonl` | `teammates/agent-{id}.meta.json` |

**`.meta.json` sidecar schema**(对齐 v4 §8.4):

```json
{
  "agent_id": "...",
  "agent_name": "researcher",
  "kind": "teammate",
  "employee_id": "xiaoyan",
  "team_id": "{conv_id}",
  "spawned_by": "{lead_agent_id}",
  "spawned_at": "...",
  "model": "sonnet",
  "is_async": true,
  "tool_whitelist": ["Read", "Edit", "SendMessage", ...]
}
```


**代码草稿**:

```rust
// worker_runtime.rs
pub enum WorkerMode {
    /// 老路径:async subagent,跑完即死。
    AsyncOneShot,
    /// 新路径:Teammate,常驻 idle 直到被显式终止。
    TeammateIdle {
        team_handle: Arc<Mutex<Team>>,
        agent_name: String,
    },
}

pub async fn run_worker(
    mode: WorkerMode,
    ctx: WorkerCtx,
    initial_prompt: Option<String>,
) -> Result<(), WorkerError> {
    match mode {
        WorkerMode::AsyncOneShot => run_async_oneshot(ctx, initial_prompt).await,
        WorkerMode::TeammateIdle { team_handle, agent_name } => {
            run_teammate_idle(ctx, team_handle, agent_name, initial_prompt).await
        }
    }
}

async fn run_teammate_idle(
    ctx: WorkerCtx,
    team_handle: Arc<Mutex<Team>>,
    agent_name: String,
    initial_prompt: Option<String>,
) -> Result<(), WorkerError> {
    // 1. 跑 first turn(initial_prompt 是 spawn_subagent 调用方给的 dispatch prompt)
    if let Some(p) = initial_prompt {
        ctx.inbox().push(InboxItem::ChatMessage(p, MessageSource::Lead)).await;
    }

    let mut heartbeat = tokio::time::interval(Duration::from_secs(60));
    heartbeat.tick().await; // 跳过第一次立刻 fire

    loop {
        tokio::select! {
            _ = ctx.cancellation().cancelled() => {
                tracing::info!(agent = %agent_name, "teammate cancelled, exiting idle loop");
                cleanup_teammate(&ctx, &team_handle, &agent_name).await;
                return Ok(());
            }
            item = ctx.inbox().recv() => {
                match item {
                    Some(InboxItem::ChatMessage(text, src)) => {
                        run_one_turn(&ctx, text, src).await?;
                    }
                    Some(InboxItem::Shutdown(req)) => {
                        // P2 实装;P1 占位:直接退出
                        tracing::warn!(agent = %agent_name, "shutdown_request received (P1 stub: exit)");
                        cleanup_teammate(&ctx, &team_handle, &agent_name).await;
                        return Ok(());
                    }
                    Some(InboxItem::TaskNotification(_)) => {
                        // P2 实装
                    }
                    None => {
                        tracing::info!(agent = %agent_name, "inbox closed, exiting idle loop");
                        cleanup_teammate(&ctx, &team_handle, &agent_name).await;
                        return Ok(());
                    }
                }
            }
            _ = heartbeat.tick() => {
                // 更新 last_active_at
                let mut team = team_handle.lock().await;
                if let Some(m) = team.teammates.iter_mut().find(|m| m.name == agent_name) {
                    m.last_active_at = chrono::Utc::now();
                }
            }
        }
    }
}

async fn cleanup_teammate(ctx: &WorkerCtx, team: &Arc<Mutex<Team>>, name: &str) {
    // unregister AgentNameRegistry
    ctx.agent_names().unregister(ctx.session_id(), name).await;
    // 从 Team 里删
    let mut team = team.lock().await;
    team.teammates.retain(|m| m.name != name);
    // transcript flush(已有 writer 自带 flush_on_drop;此处显式触发以让 cleanup 端到端)
    drop(ctx.transcript_writer());
}
```

**Step-by-step**:

- [ ] **Step 1**:在 `inbox.rs` 加 `enum InboxItem`,把现在的 `String` 单 channel 升级成 enum(若现有用法是单 String 也补一个 Text variant 保持向后兼容)。
- [ ] **Step 1b**:`user_scoped_paths.rs` 加 `teammates_dir(conv_id) -> PathBuf` 与 `subagents_dir(conv_id) -> PathBuf`(返回 `conversations_dir().join(conv).join("teammates")` 等);标记旧的 `subagent_transcripts_dir()` 为 `#[deprecated]` 或直接删除调用方。
- [ ] **Step 1c**:`output_writer.rs` 改写:接收 `WorkerMode`(或简化为一个 `kind: TranscriptKind { Subagent, Teammate }` 枚举),按 kind 选 `teammates_dir` / `subagents_dir`;同时写一份 `.meta.json` sidecar(spawn 时一次性写,不再 append)。schema 见上面的"`.meta.json` sidecar schema"。
- [ ] **Step 2**:`worker_runtime.rs` 加 `WorkerMode` 枚举 + `run_teammate_idle` 函数,粘草稿。
- [ ] **Step 3**:回到 P1.3 的 stub `spawn_teammate_idle_loop`,把 stub 替换成真正调用 `tokio::spawn(async move { run_worker(WorkerMode::TeammateIdle { ... }, ctx, Some(prompt)).await })`。
- [ ] **Step 4**:写 `teammate_idle_loop_skeleton_test.rs`,4 case:
  - spawn 后 cancel → 返回 + Team 中 Teammate 已被 retain 删除 + AgentNameRegistry 已 unregister
  - inbox 收 1 条 ChatMessage(走 mock LLM executor)→ 跑了 1 个 turn,transcript JSONL 多了一行(且路径在 `teammates/` 下)
- [ ] **Step 4b**:写 `transcript_path_routing_test.rs`,3 case:
  - AsyncOneShot spawn → transcript 写到 `subagents/agent-{id}.jsonl` + `.meta.json` 含 `"kind": "subagent"`
  - TeammateIdle spawn → transcript 写到 `teammates/agent-{id}.jsonl` + `.meta.json` 含 `"kind": "teammate"` + `"team_id"` + `"employee_id"` 非空
  - 两路径同一 conversation 共存不互相覆盖
  - 60s heartbeat(测试用 1s 加速版的 mocked clock)→ last_active_at 被更新
  - inbox channel 被显式关闭 → 优雅退出
- [ ] **Step 5**:回到 P1.3 的 4 个 happy/edge tests,把 `#[ignore]` 去掉,跑全绿。
- [ ] **Step 6**:`cargo test --tests --no-fail-fast` 整体不退化。
- [ ] **Step 7**:commit。

```bash
git add -A
git commit -m "feat(worker_runtime): add TeammateIdle mode with select! over cancel/inbox/heartbeat

Teammate dispatched via spawn_subagent(team_name=...) now runs as a
long-lived idle loop instead of one-shot.  P1 ships the skeleton:
ChatMessage routing, cancellation, heartbeat, cleanup.
Shutdown_request and task-notification handlers are stubs filled in P2.

Refs: 2026-05-10 plan §5.6, §6.1, §8.1"
```

---

### P1.7 — `TeamCreate` / `TeamDelete` 工具 + Lead 进入"Team 模式"判定

**目标**:Lead LLM 显式调 `TeamCreate(team_name?)` 把当前 Session 升格成"Team 模式";调 `TeamDelete()` 显式退出。**不做隐式 Team 模式判定** —— 这是 v4 §3.A 的明确决定(由 LLM 自主决定建不建 Team)。

**文件**:
- 新增 `src-tauri/src/runtime/tools/builtin/team_tools.rs`
- 在 `plugin/builtin/tools/mod.rs::register_builtin_tools` 注册
- 新增 `src-tauri/tests/team_tools_test.rs`

**代码草稿**:

```rust
// team_tools.rs
pub struct TeamCreate;
#[derive(Deserialize, JsonSchema)]
pub struct TeamCreateArgs {
    pub team_name: Option<String>,
    pub description: Option<String>,
}
#[derive(Serialize)]
pub struct TeamCreateResult { pub team_name: String, pub session_id: String }

#[async_trait]
impl RuntimeTool for TeamCreate {
    fn name(&self) -> &str { "TeamCreate" }
    fn description(&self) -> &str { /* §3.A 文案 */ }
    async fn execute(&self, ctx: ToolExecutionContext, args: Value) -> ToolResult {
        let args: TeamCreateArgs = serde_json::from_value(args)?;
        let session = ctx.session_id().clone();
        let lead_name = "team-lead".to_string();
        let lead_id = ctx.agent_id().clone();           // Lead 是当前调用者
        let lead = Member {
            agent_id: lead_id.clone(),
            name: lead_name.clone(),
            role: MemberRole::Lead,
            created_at: chrono::Utc::now(),
            last_active_at: chrono::Utc::now(),
        };
        let team_name = args.team_name.unwrap_or_else(|| format!("team-{}", session.short8()));
        ctx.team_registry().create(session.clone(), lead, team_name.clone()).await
            .map_err(|e| ToolError::msg(e.to_string()))?;
        // 同时把 Lead 注册到 AgentNameRegistry,允许 Teammate 用 to:"team-lead" 寻址
        ctx.agent_names().register(&session, &"team-lead", lead_id).await
            .map_err(|e| ToolError::msg(e.to_string()))?;
        Ok(ToolOutput::json(TeamCreateResult { team_name, session_id: session.to_string() }))
    }
}

pub struct TeamDelete;
// 实现略;读 TeamRegistry,逐个 cancel teammate(送 cancellation),然后 registry.delete + name registry drop_session
```

**Step-by-step**:

- [ ] **Step 1**:新增 team_tools.rs,实现 TeamCreate / TeamDelete。
- [ ] **Step 2**:在 register_builtin_tools 注册两个工具。
- [ ] **Step 3**:Tool description 严格按 §3.A 写(中文版,1:1 对齐 claude-code-best teamCreate.ts):
  - TeamCreate:何时建 Team(任务需要 ≥2 个独立 Worker 并行,或者跨 domain 协作)/ 不该建(纯 chat / 1 步即可完成)/ 4 人硬限。
  - TeamDelete:何时退(任务收尾)/ 副作用(所有 Teammate 收到 cancel,inbox 清空)。
- [ ] **Step 4**:写 `team_tools_test.rs`,5 case:
  - Lead 调 TeamCreate → registry 出现一条 + AgentNameRegistry 出现 "team-lead"
  - 已有 Team 时再 TeamCreate → TeamAlreadyExists
  - TeamDelete:先 TeamCreate,spawn 2 个 Teammate,然后 TeamDelete → 2 个 Teammate cancellation 被触发 + Team 被 drop + name registry 清空
  - TeamDelete 无 Team → 静默 noop(ok 返回)
  - 默认 team_name 是 `team-{session8}`
- [ ] **Step 5**:cargo test 绿。
- [ ] **Step 6**:commit。

```bash
git add -A
git commit -m "feat(tools): add TeamCreate / TeamDelete to mark explicit Team mode boundaries

Team mode is opt-in by the Lead LLM, not implicit.  TeamCreate seeds
the Lead member as 'team-lead' (per claude-code-best convention) so
teammates can address it with SendMessage(to: 'team-lead').
TeamDelete cascades cancellation to every teammate and clears name
registrations.

Refs: 2026-05-10 plan §3.A"
```

---

### P1.8 — Session 生命周期 cleanup hook

**目标**:对齐 v4 §7.3 cleanup 触发点表里"app close"那一项,确保 app 关闭/Session 正常终止时,TeamRegistry / AgentNameRegistry / pending channels / transcript writers 全部被清理。

**4 个触发点**:

| 触发点 | 接入位置 | P0/P1 状态 |
|---|---|---|
| `cancel_session` | `SessionRuntime::cancel_session` | 已有,P1.8 在尾部加 cleanup 调用 |
| TeamDelete 工具 | `team_tools::TeamDelete::execute` | P1.7 已加 |
| Session GC(60s tick 检测无 owner) | `runtime/session_runtime.rs` 内置 tick | **本期不做**,留为后续优化项 |
| App close | `lib.rs::on_window_event(CloseRequested)` 或 RunEvent::ExitRequested | **P1.8 接入** |

**文件**:
- 改 `src-tauri/src/runtime/session_runtime.rs`(在 cancel_session 尾部 cleanup)
- 改 `src-tauri/src/lib.rs`(app close hook)
- 新增 `src-tauri/tests/session_cleanup_test.rs`

**Step-by-step**:

- [ ] **Step 1**:`session_runtime::cancel_session` 内,在已有 cancel cascade 之后加:

```rust
// cleanup LTR per-session state
self.team_registry.delete(&session_id).await;
self.agent_names.drop_session(&session_id).await;
// inbox / transcript writers are dropped naturally as worker tasks exit
```

- [ ] **Step 2**:`lib.rs` 接 `tauri::RunEvent::ExitRequested` 或 `WindowEvent::CloseRequested`(确认 Tauri 2.x API 名称),内部:

```rust
let team_reg = app.state::<Arc<TeamRegistry>>().inner().clone();
let name_reg = app.state::<Arc<AgentNameRegistry>>().inner().clone();
tauri::async_runtime::block_on(async move {
    let sessions: Vec<SessionId> = /* 取所有活跃 SessionId */;
    for s in sessions {
        team_reg.delete(&s).await;
        name_reg.drop_session(&s).await;
    }
});
```

- [ ] **Step 3**:写 `session_cleanup_test.rs`,3 case:
  - cancel_session 触发后,registry get 返回 None
  - cancel_session 触发后,name registry resolve 返回 None
  - 一个 Session 有 2 个 Teammate;cancel 后,两个 Teammate 的 cancellation token 被 trip,worker_runtime 异步 cleanup 执行(通过 channel 收回 cleanup 信号验证)
- [ ] **Step 4**:cargo test 绿。
- [ ] **Step 5**:commit。

```bash
git add -A
git commit -m "feat(session): cleanup TeamRegistry/AgentNameRegistry on cancel and app exit

Per v4 §7.3, all four cleanup triggers (cancel_session, TeamDelete,
session GC, app close) must release per-session state.  This commit
covers cancel_session and app close — TeamDelete already handled in
P1.7 and session GC is deferred.

Refs: 2026-05-10 plan §7.3"
```

---

### P1 完成检查

- [ ] `cd src-tauri && cargo test review_ --tests --no-fail-fast` 全绿(含新加的 review_team_registry_session_isolation_test)
- [ ] `cd src-tauri && cargo test --tests --no-fail-fast` 整体不退化
- [ ] 新增 test 文件全 PASS:`review_team_registry_session_isolation_test`、`agent_name_registry_test`、`spawn_teammate_via_employee_test`、`teammate_required_tools_test`、`task_claim_test`、`task_blocks_cycle_detection_test`、`review_task_storage_per_conv_test`、`teammate_idle_loop_skeleton_test`、`team_tools_test`、`session_cleanup_test`
- [ ] `pnpm test` 前端不退化
- [ ] git log 看到 P1 期 ≥ 8 个 commit
- [ ] **手测**:启动 dev,在一个真实 Session 中让 Lead LLM 走 TeamCreate → spawn_subagent(employee_id=小研, team_name=research-team, name=researcher) → 看 Team.teammates.len() == 1 + transcript JSONL 出现 + AgentNameRegistry 中可 resolve `researcher`。**这一步证明 P1 骨架打通**(虽然 Teammate 还没法被 SendMessage 唤起,那是 P2)。

P1 通过 → 进入 P2。

---

## P2 阶段 — 通信链路:SendMessage + 结构化消息 + Lead idle + is_async

> 目标:把 v4 §3.C(通信)+ §5(交互机制 5.6/5.7/5.8/5.9)+ §7.4(is_async flag)落到代码。结束时端到端可演:Lead → SendMessage 给 Teammate → Teammate 跑 turn → SendMessage 回复 → Lead idle 被唤起续 turn → 最终 plan_approval_request / shutdown_request 走完整生命周期。
>
> P2 不做:G3 broadcast 的 fan-in coalescing(MVP 用最朴素 fan-out 即可);跨进程 / 跨机器 SendMessage(永远不做)。

### P2 Task 总览

| # | 任务 | 估时 | 依赖 |
|---|---|---|---|
| P2.1 | `runtime/messaging/mod.rs` + `StructuredMessage` discriminated union(text / shutdown_request / shutdown_response / plan_approval_request / plan_approval_response) | 1h | P1 全部 |
| P2.2 | `SendMessage` 工具:按 `to` 名解析 + 广播 `to:"*"` 路由 + 写对方 inbox + `inboxes/{name}.json` 磁盘审计备份 | 2.5h | P2.1 |
| P2.3 | TEAMMATE_ADDENDUM 中文版定稿 + Teammate boot prompt 注入(对齐 §5.9) | 1h | P2.1 |
| P2.3b | 第一 turn `team_context` `<system-reminder>` attachment(对齐 §5.8) | 1h | P2.3 / P1.1b |
| P2.4 | Lead idle 触发机制:turn 结束自检 pending(路径 A,chat_turn_driver)+ 消息入队 kick(路径 C,async_task_store) | 2.5h | P2.2 |
| P2.5 | task-notification 投递:共享 Task 状态变化时把 XML 包装 user message 注入 Lead inbox | 1.5h | P2.4 |
| P2.6 | shutdown_request / shutdown_response 处理:Teammate 收到 shutdown → LLM 决定 → 优雅退出或 deny | 1.5h | P2.2 |
| P2.7 | `TaskStop` 工具:强制 cancel Teammate,绕过 LLM 决策 | 0.5h | P2.6 |
| P2.8 | `is_async` flag:RunCtx 加 bool;Teammate / async subagent 为 true;permission Ask 在 true 时自动 deny | 1h | P1.6 |
| P2.9 | plan_approval_request / plan_approval_response:Teammate 提案 plan → Lead 批/否(纯结构化,不走 permission system) | 1h | P2.2 |
| P2.10 | 广播 `to:"*"`:fan-out 到当前 Team 所有 Teammate(不含 Lead 自己) | 0.5h | P2.2 |
| P2.11 | 端到端冒烟:真实 LLM 跑一遍 Lead+1 Teammate 任务,通过 §1.5 三条 E2E 验收 | 1.5h | 全部 |

P2 阶段总估时:15.5h;关键路径:P2.1 → P2.2 → P2.3b → P2.4 → P2.11。

---

### P2.1 — `StructuredMessage` discriminated union

**目标**:SendMessage 的 `message` 字段是一个有 5 种 variant 的 union,不是裸字符串。这是 v4 §4.2 定下的核心。

**文件**:
- 新增 `src-tauri/src/runtime/messaging/mod.rs`
- 新增 `src-tauri/src/runtime/messaging/structured.rs`
- 新增 `src-tauri/tests/structured_message_roundtrip_test.rs`

**代码草稿**:

```rust
// runtime/messaging/structured.rs
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StructuredMessage {
    Text { content: String },
    ShutdownRequest { reason: Option<String> },
    ShutdownResponse { request_id: String, approve: bool, reason: Option<String> },
    PlanApprovalRequest { request_id: String, plan: String },
    PlanApprovalResponse { request_id: String, approve: bool, feedback: Option<String> },
}

impl StructuredMessage {
    pub fn text(s: impl Into<String>) -> Self { Self::Text { content: s.into() } }

    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::Text { .. } => "text",
            Self::ShutdownRequest { .. } => "shutdown_request",
            Self::ShutdownResponse { .. } => "shutdown_response",
            Self::PlanApprovalRequest { .. } => "plan_approval_request",
            Self::PlanApprovalResponse { .. } => "plan_approval_response",
        }
    }
}
```

**Step-by-step**:

- [ ] **Step 1**:新增 messaging mod + structured.rs,粘草稿。
- [ ] **Step 2**:`runtime/mod.rs` 暴露 `pub mod messaging;`。
- [ ] **Step 3**:**改 inbox.rs**:`InboxItem::ChatMessage(StructuredMessage, MessageSource)`(P1.6 占位时是 String,这里升级)。已有 P1.6 单元测试要 fix 改 String 用法。
- [ ] **Step 4**:写 `tests/structured_message_roundtrip_test.rs`,5 case(每种 variant 一次 serde JSON roundtrip,确认 tag 字段是 `type` 且 snake_case)。
- [ ] **Step 5**:`cargo test --tests --no-fail-fast` 通过(P1.6 测试需要同步 fix)。
- [ ] **Step 6**:commit。

```bash
git add -A
git commit -m "feat(messaging): StructuredMessage discriminated union (5 variants)

Future SendMessage payloads are typed.  Variants: text,
shutdown_request, shutdown_response, plan_approval_request,
plan_approval_response.  Inbox channel upgraded from String to
StructuredMessage.

Refs: 2026-05-10 plan §4.2"
```

---

### P2.2 — `SendMessage` 工具

**目标**:Lead / Teammate 调 `SendMessage(to, message)` 把 StructuredMessage 投到对方 inbox。`to` 是 name(`team-lead` / Teammate 名 / `"*"` 广播)。

**文件**:
- 新增 `src-tauri/src/runtime/tools/builtin/send_message.rs`
- 在 `register_builtin_tools` 注册
- 新增 `src-tauri/tests/send_message_routing_test.rs`

**入参 schema**:

```rust
#[derive(Deserialize, JsonSchema)]
pub struct SendMessageArgs {
    pub to: String,                       // name 或 "*"
    pub message: StructuredMessage,       // 见 P2.1
    pub summary: Option<String>,          // 5-10 字 UI preview
}
```

**Step-by-step**:

- [ ] **Step 1**:新增文件,实现 RuntimeTool。execute 步骤:

```rust
let session = ctx.session_id().clone();
let from = ctx.agent_name().unwrap_or_else(|| ctx.agent_id().to_string());

if args.to == "*" {
    return broadcast(&session, &from, args.message, ctx).await;
}

let target_id = ctx.agent_names().resolve(&session, &args.to).await
    .ok_or_else(|| ToolError::msg(format!("no agent named `{}` in this session", args.to)))?;

let inbox = ctx.inbox_registry().get(&target_id).await
    .ok_or_else(|| ToolError::msg("target agent not subscribed to any inbox"))?;

let source = if from == "team-lead" { MessageSource::Lead } else { MessageSource::Teammate(from.clone()) };
inbox.push(InboxItem::ChatMessage(args.message.clone(), source)).await;

// 触发对方"消息入队 kick"(P2.4 路径 C)
ctx.session_runtime().kick_pending(&session, &target_id).await;

Ok(ToolOutput::json(json!({ "delivered_to": args.to, "variant": args.message.variant_name() })))
```

- [ ] **Step 2**:`InboxRegistry`(若没有)新增 — 按 `AgentId → InboxHandle` 索引;Teammate spawn 时(P1.6)在 inbox 注册;cleanup 时 deregister。这一项作为前置加在本 Task 第一步。
- [ ] **Step 2b**:**inbox 磁盘审计备份**(对齐 v4 §8.4):每次 push 进 channel 时,**同步** append 一条到 `users/{scope}/conversations/{conv}/inboxes/{name}.json`(或 `lead.json` 若 target 是 Lead)。文件结构:JSON 数组,每条是 `{from, to, message: StructuredMessage, summary?, ts}`。**主路径仍是 channel**;磁盘文件**不用于状态恢复**(crash 后丢消息可接受),只供开发期审计/排错。实现走 `runtime::messaging::inbox_audit::append(scope_paths, conv_id, target_name, entry)`,新增模块。
- [ ] **Step 3**:Tool description 严格按 §3.C / §5.9 写,中文版,1:1 对齐 claude-code-best sendMessage.ts:
  - 何时用:Teammate 完成阶段性产出告诉 Lead;Lead 派任务给 Teammate;广播紧急停止信号。
  - 不该用:报告 task 状态(用 TaskUpdate),写产物文件(直接写磁盘)。
- [ ] **Step 4**:写 `send_message_routing_test.rs`,6 case:
  - 投递给已注册 name → 对方 inbox 收到
  - 投递给未注册 name → 报错 NotFound
  - `to:"*"` → fan-out 到所有 Teammate 但不含发送者自己也不含 Lead 自己(P2.10 完成后)
  - StructuredMessage 5 variant 各投一次,对方 inbox InboxItem 收到的 variant 正确
  - 同一 Session 跨 from/to 配对,inbox 不混
  - Lead 给自己发(`to:"team-lead"` 但 from 也是 team-lead)→ 报错 SelfSend(避免死循环)
  - **磁盘审计**:投递成功后,`inboxes/{target}.json` 数组长度 +1,内容含 `from / to / message / ts`
- [ ] **Step 5**:cargo test 绿。
- [ ] **Step 6**:commit。

```bash
git add -A
git commit -m "feat(tools): SendMessage by name with StructuredMessage payload

Routes to a single peer (by name) or broadcasts via to:'*'.
Looks the name up in AgentNameRegistry, pushes a StructuredMessage
into the target's inbox, then kicks the receiver's pending-queue
hook so its idle loop wakes up promptly.

Refs: 2026-05-10 plan §3.C, §5.6"
```

---

### P2.3 — TEAMMATE_ADDENDUM 中文版 + boot prompt 注入

**目标**:Teammate 起 idle loop 时,在 EmployeeRecord 的 system prompt 之外**追加** TEAMMATE_ADDENDUM(中文版,§5.9 定稿)。Lead **不叠**对应 addendum(v4 §3.B.1 决定)。

**文件**:
- 新增 `src-tauri/src/runtime/agent/teammate_addendum.rs`(常量 + getter)
- 改 `runtime/agent/worker_runtime.rs::run_teammate_idle`(boot prompt 拼接)
- 新增 `src-tauri/tests/teammate_addendum_present_test.rs`

**草稿**(文案是核心交付物,精确照 §5.9):

```rust
// teammate_addendum.rs
pub const TEAMMATE_ADDENDUM_ZH: &str = r#"

## 你正在以 Teammate 身份运行

你不是独自工作。你属于 `{team_name}` 团队的一员,名字是 `{teammate_name}`。
当前团队还有一位 Lead(`team-lead`)与可能存在的其他 Teammate。

### 与 Lead / 其他 Teammate 通信
- 用 `SendMessage(to=..., message={"type":"text","content":"..."})` 给具体名字的成员发消息。
- 给 Lead 发 → `to: "team-lead"`。
- 广播给所有 Teammate(不含 Lead)→ `to: "*"`。
- **不要**用 SendMessage 报告 task 进度;用 `TaskUpdate(status=in_progress|completed)`。

### 任务市场
- 你可以用 `TaskList()` 查看 Team 的共享任务清单。
- 看到 owner 为空 / "*" 的任务,如果适合你的能力,用 `TaskClaim(task_id)` 认领。
- 不要重复认领别人已经 owner 的 task。

### 优雅关闭
- 你可能收到 `shutdown_request`(`{"type":"shutdown_request","reason":"..."}`)。
- 你 **必须** 用 `SendMessage(to="team-lead", message={"type":"shutdown_response","request_id":"...","approve":<bool>,"reason":"..."})` 显式回应。
- 如果工作已收尾且无未保存状态,approve=true;否则 approve=false 并简述原因(Lead 可以 retry 或 TaskStop 强制关闭)。

### 协作纪律
- **不要** 询问用户(Ask)。你是 async,任何 ask 会被自动 deny。
- 跨 turn 你的 conversation history 会被保留;但其他 Teammate 的 history 你看不见 — 该交换的信息**显式** SendMessage。
- 完成阶段性产出:`TaskUpdate(status=completed)` + `SendMessage(to=team-lead, text="..." )` 通报。
"#;

pub fn render(team_name: &str, teammate_name: &str) -> String {
    TEAMMATE_ADDENDUM_ZH
        .replace("{team_name}", team_name)
        .replace("{teammate_name}", teammate_name)
}
```

**Step-by-step**:

- [ ] **Step 1**:新建文件,粘文案 + render。
- [ ] **Step 2**:`run_teammate_idle` 在 ctx 注入 system prompt 前,把 render 结果 append 到 Employee 的 system prompt 末尾。
- [ ] **Step 3**:写 `teammate_addendum_present_test.rs`:启动 Teammate 后,检查传给 LlmGateway 的 system prompt 含 "Teammate 身份" 字样 + team_name 已替换。
- [ ] **Step 4**:cargo test 绿。
- [ ] **Step 5**:commit。

```bash
git add -A
git commit -m "feat(prompt): append TEAMMATE_ADDENDUM (zh) to Teammate boot prompt

Per v4 §3.B.1 the Lead does NOT get an addendum; collaboration
guidance for the Lead lives entirely in the tool descriptions.
Teammates get a short Chinese addendum explaining SendMessage rules,
task market behavior, shutdown handshake, and async-ask discipline.

Refs: 2026-05-10 plan §5.9"
```

---

### P2.3b — 第一 turn `team_context` attachment

**目标**:对齐 v4 §5.8(claude-code-best `attachments.ts:3776 getTeamContextAttachment`)。Teammate 在第一 turn(`hasAssistantMessage === false`)前,注入一条 `<system-reminder>` user message,告诉它:自己的名字、所属 team、team.json / tasks/ 完整路径、Lead 名字 = `team-lead`、SendMessage 用 name 寻址不要用 UUID。**只注入一次**,后续 turn 不再注入。

> 注意区分:
> - **TEAMMATE_ADDENDUM**(P2.3)= 拼到 system prompt 末尾,每个 turn 都在
> - **team_context attachment**(P2.3b)= 第一 turn 的一条 user message(包 `<system-reminder>`),只注入一次

**文件**:
- 新增 `src-tauri/src/runtime/agent/team_context.rs`(render 函数)
- 改 `src-tauri/src/runtime/agent/worker_runtime.rs::run_teammate_idle`(初始化时把 attachment 作为第一条 user message 注入 inbox 或 history)
- 新增 `src-tauri/tests/team_context_attachment_test.rs`

**`team_context` 中文模板**(对齐 §5.8,1:1 对齐 claude-code-best 结构):

```rust
pub const TEAM_CONTEXT_TEMPLATE: &str = r#"<system-reminder>
# 团队协作

你是团队 "{team_name}" 的成员。

**你的身份**:
- 名字: {agent_name}

**团队资源**:
- 团队配置: {team_json_path}
- 任务列表: {tasks_dir_path}

**团队负责人**:Lead 的名字是 "team-lead"。把进度和完成情况发给 Lead。

读取团队配置文件了解队友名单。定期检查任务列表。需要分工时创建新任务,完成后标记任务为 resolved。

**重要**:始终用名字(如 "team-lead", "researcher", "analyzer")称呼队友,绝不用 UUID。发消息时直接用名字:

```json
{
  "to": "team-lead",
  "message": "你的消息内容",
  "summary": "5-10 字预览"
}
```
</system-reminder>"#;

pub fn render(team_name: &str, agent_name: &str, team_json_path: &Path, tasks_dir: &Path) -> String {
    TEAM_CONTEXT_TEMPLATE
        .replace("{team_name}", team_name)
        .replace("{agent_name}", agent_name)
        .replace("{team_json_path}", &team_json_path.display().to_string())
        .replace("{tasks_dir_path}", &tasks_dir.display().to_string())
}
```

**Step-by-step**:

- [ ] **Step 1**:新增 `team_context.rs`,粘 template + render。
- [ ] **Step 2**:改 `run_teammate_idle`:在 P2.3 已经追加 TEAMMATE_ADDENDUM 到 system prompt 之后、push initial_prompt 之前,**先**往 inbox push 一条 `InboxItem::ChatMessage(StructuredMessage::Text { content: team_context_xml }, MessageSource::System)`。这样它会作为第一条 user message 进入 LLM history。
- [ ] **Step 3**:**只注入一次**:用一个 `first_turn_done: bool` 在 ctx 上,run_one_turn 跑完后置 true;后续若再次有"first turn"路径(不应该,但保险)直接跳过。
- [ ] **Step 4**:写 `team_context_attachment_test.rs`,4 case:
  - Teammate 第一 turn 的 LLM history 第一条 user message 含 `<system-reminder>` 且 `{team_name}` / `{agent_name}` 已替换
  - 第二 turn 的 LLM history 不再次注入 attachment
  - team.json / tasks/ 路径正确(指向 `users/{scope}/conversations/{conv}/...`)
  - 普通 AsyncOneShot subagent **不**注入 team_context(因为它没 team)
- [ ] **Step 5**:cargo test 绿。
- [ ] **Step 6**:commit。

```bash
git add -A
git commit -m "feat(teammate): inject team_context system-reminder on first turn

Aligns with claude-code-best getTeamContextAttachment.  Teammate's
first user message is a <system-reminder> describing its own name,
team name, and the absolute paths to team.json / tasks/.  Inserted
exactly once; subsequent turns get no further attachment.  AsyncOneShot
subagents do NOT receive this — they have no team.

Refs: 2026-05-10 plan §5.8"
```

---


### P2.4 — Lead idle 触发机制(双保险:A 路径 + C 路径)

**目标**:Lead 是用户/cron 派活的本次 turn 跑完后**默认结束**,但如果有 Teammate 在它跑 turn 期间投递了消息进它的 inbox,Lead 必须续 turn 处理。这就是 §5.6 的"双保险":

- **路径 A**:Lead 的 chat turn 结束前(`chat_turn_driver` 收尾)自检 `inbox.len() > 0` → 立即续 turn(不释放控制流回前端)。
- **路径 C**:Teammate SendMessage 走 P2.2 完成后,调 `kick_pending(session, lead_id)`。`async_task_store`(或新加的 `LeadPendingHook`)如果发现 Lead 当前空闲(无活跃 turn),就拉起一个新 turn;如果 Lead 当前正在跑(路径 A 会兜底),就只更新 pending mark 然后返回。

**文件**:
- 改 `src-tauri/src/runtime/chat/chat_turn_driver.rs`(turn 结束前自检)
- 改 `src-tauri/src/runtime/agent/async_task_store.rs`(扩展承载 LeadPending channel)
- 新增 `src-tauri/src/runtime/agent/lead_idle.rs`(`LeadIdleSupervisor`,封装"是否在跑 turn"状态)
- 新增 `src-tauri/tests/lead_idle_trigger_test.rs`

**Step-by-step**:

- [ ] **Step 1**:新增 `lead_idle.rs`:

```rust
pub struct LeadIdleSupervisor {
    // per (session, agent) → 状态
    state: Mutex<HashMap<(SessionId, AgentId), LeadState>>,
}
enum LeadState {
    Running,                  // 当前在跑 turn
    Idle { pending: bool },   // 空闲;pending=true 表示已经有消息排队等
}
impl LeadIdleSupervisor {
    pub async fn mark_running(&self, k: &Key);
    pub async fn mark_idle(&self, k: &Key) -> bool;   // 返回 true if had pending → 调用方需立刻续 turn
    pub async fn enqueue(&self, k: &Key) -> bool;     // 返回 true if Lead 现在 Idle 且需要 caller 唤起;false 则只置 pending
}
```

- [ ] **Step 2**:**路径 A**(chat_turn_driver):turn 主循环结束、event_bus emit AgentIdle 之前:

```rust
self.lead_idle.mark_running(&k);  // 进入循环前 mark running
// ... 跑 turn ...
let had_pending = self.lead_idle.mark_idle(&k).await;
if had_pending {
    continue; // 重入下一 turn
}
break; // 正常退出
```

- [ ] **Step 3**:**路径 C**(SendMessage 末尾):

```rust
let need_wake = ctx.lead_idle().enqueue(&key).await;
if need_wake {
    // Lead 当前 Idle,要拉起一个新 turn
    ctx.session_runtime().run_chat_turn_continuation(session_id, lead_id).await?;
}
```

- [ ] **Step 4**:`async_task_store` 不需要新建 LeadPendingHook,改成依赖 `LeadIdleSupervisor`,因为后者已经把"是否需要唤起"封装好了。`async_task_store` 现有 pending queue 留给 task-notification(P2.5)用。
- [ ] **Step 5**:写 `lead_idle_trigger_test.rs`,5 case:
  - Lead 正跑 turn,Teammate SendMessage → 不立即拉起;turn 结束自检发现 pending → 续 turn
  - Lead 空闲(idle 状态),Teammate SendMessage → 立即拉起新 turn
  - Lead 空闲,**先**置 pending **再**拉起的竞态:模拟两个 Teammate 几乎同时 SendMessage,只应拉起一次 turn(由 mark_running 的 atomic CAS 保证)
  - Lead 正在跑 turn 期间收到 10 条 SendMessage → 收尾自检后续 1 个 turn 处理全部(因为 turn 内 LLM 一次性能看见 inbox 里所有消息)
  - mark_idle 时 pending=false → 不续 turn,正常返回 AgentIdle
- [ ] **Step 6**:cargo test 绿。
- [ ] **Step 7**:commit。

```bash
git add -A
git commit -m "feat(idle): dual-path Lead idle trigger (turn-end self-check + enqueue kick)

When a Teammate's SendMessage lands in the Lead's inbox we must reliably
resume the Lead's turn loop without racing.  Path A: chat_turn_driver
re-checks the inbox before emitting AgentIdle.  Path C: SendMessage
asks the supervisor whether to wake the Lead; only one path actually
spawns the next turn thanks to an atomic Running/Idle CAS.

Refs: 2026-05-10 plan §5.6"
```

---

### P2.5 — task-notification 投递

**目标**:共享 Task 状态变化时(TaskUpdate / TaskClaim / TaskCreate / 完成),把一条 XML 包装的 user message 注入 **Lead** 的 inbox(不投 Teammate;Teammate 自己 TaskList 拉取)。XML 形如:

```xml
<task-notification id="t_xxx" actor="researcher" action="claimed">
  <subject>Survey ...</subject>
  <status>in_progress</status>
</task-notification>
```

**文件**:
- 改 `src-tauri/src/runtime/tools/builtin/task_tools.rs`(TaskUpdate/Claim/Create 路径加 emitter)
- 改 `src-tauri/src/runtime/agent/task_notification.rs`(已存在,扩展投递逻辑)
- 新增 `src-tauri/tests/task_notification_to_lead_test.rs`

**Step-by-step**:

- [ ] **Step 1**:确认 `task_notification.rs` 当前结构(已存在),扩展或新增 `emit_to_lead(session_id, payload: TaskNotificationXml)`,内部找 Team.lead.agent_id → 走 InboxRegistry → push InboxItem::ChatMessage(StructuredMessage::Text { content: xml }, MessageSource::System)。
- [ ] **Step 2**:每个 task_tools 入口(TaskCreate/TaskUpdate/TaskClaim)成功后调 emitter。注意只在该 task 属于 Team 模式时 emit(无 Team → 不投)。
- [ ] **Step 3**:emit 完后调 `LeadIdleSupervisor::enqueue` 走 P2.4 路径 C 唤起 Lead。
- [ ] **Step 4**:写 `task_notification_to_lead_test.rs`,4 case:
  - Teammate TaskClaim → Lead inbox 收到 `<task-notification action="claimed">`
  - Lead TaskCreate → 不给自己发(避免回声;同 P2.2 SelfSend 校验)
  - 非 Team 模式 TaskUpdate → 不 emit
  - 同一 turn 内多次 TaskUpdate → 多条 notification(P2 不做合并,合并留待 G3 fan-in)
- [ ] **Step 5**:cargo test 绿。
- [ ] **Step 6**:commit。

```bash
git add -A
git commit -m "feat(tasks): emit task-notification XML to Lead inbox on TaskCreate/Update/Claim

Only fires in Team mode and never echoes back to the actor.  Uses
LeadIdleSupervisor.enqueue to wake the Lead, so a Teammate claiming
a task during the Lead's turn results in the Lead processing it on
the next turn cycle.

Refs: 2026-05-10 plan §5.7"
```

---

### P2.6 — shutdown_request / shutdown_response 处理

**目标**:把 P1.6 idle loop 里的"shutdown 占位直接退出"换成走完整握手:

1. Lead `SendMessage(to=researcher, message={type:shutdown_request,reason:"任务完成"})`。
2. Teammate inbox 收到 → 走 turn → LLM 输出 `SendMessage(to=team-lead, message={type:shutdown_response, request_id, approve, reason})` + 自行 cleanup 任何未保存状态。
3. **Teammate 主动调** `TaskList` / `Done` / 自己结束后,等待 Lead 的回应。Lead 看到 shutdown_response → 决定:approve=true 就什么都不做让 Teammate 自然退出(它已经做完工作)或 TaskStop 兜底强制清理;approve=false 就重试或 TaskStop。
4. Teammate 自己**绝对不能因为发了 response 就立刻自杀** — 它要继续 idle 等 cancel(由 Lead 决定 TaskStop 还是不管让 turn 末尾资源 GC)。

**文件**:
- 改 `src-tauri/src/runtime/agent/worker_runtime.rs`(`InboxItem::Shutdown` 分支)
- 新增 `src-tauri/tests/shutdown_handshake_test.rs`

**Step-by-step**:

- [ ] **Step 1**:`run_teammate_idle` 的 shutdown 分支改成:

```rust
Some(InboxItem::ChatMessage(StructuredMessage::ShutdownRequest { reason }, _)) => {
    // 注入成一条 user message 让 LLM 决策回应
    let user_text = format!("<shutdown-request reason=\"{}\">请用 SendMessage shutdown_response 回应</shutdown-request>", reason.unwrap_or_default());
    run_one_turn(&ctx, user_text, MessageSource::Lead).await?;
    // LLM 应该在 turn 内调 SendMessage(shutdown_response);不要在这里自动退出
    // 继续 idle,Lead 看到 response 后决定下一步
}
```

- [ ] **Step 2**:写 `shutdown_handshake_test.rs`,4 case(使用 mock LLM executor 模拟回应):
  - Mock LLM 决定 approve=true → Teammate 发出 shutdown_response,Lead 收到 + Teammate 仍在 idle(直到 Lead TaskStop 或 TeamDelete)
  - Mock LLM 决定 approve=false + reason="还有未保存" → Lead 收到 response,Teammate 继续 idle
  - Lead 收到 approve=false 后调 TaskStop → Teammate 被强制退出 + cleanup 跑完
  - 同时收到两个 shutdown_request(罕见)→ 两次都正确触发 turn
- [ ] **Step 3**:cargo test 绿。
- [ ] **Step 4**:commit。

```bash
git add -A
git commit -m "feat(teammate): graceful shutdown handshake via structured messages

Teammate no longer self-terminates on shutdown_request.  Instead it
runs one turn whose user message carries the request; the LLM decides
whether to approve and replies via SendMessage(shutdown_response).
The Lead then chooses to TaskStop the teammate or retry — Teammate
stays idle until explicitly cancelled.

Refs: 2026-05-10 plan §5.3, §5.8"
```

---

### P2.7 — `TaskStop` 工具

**目标**:Lead(或任何持有权限的 caller)调 `TaskStop(agent_name)` 直接 cancel 指定 Teammate,绕过 LLM 决策。给紧急情况/卡死/shutdown_response=false 后的强制退出兜底。

> 命名:与 superpowers `TaskStop` 类似但作用域是 Teammate,而非 background task。可考虑改名 `TeammateStop` 避免混淆;**默认保持 `TaskStop` 以对齐 claude-code-best**;如果 PR review 觉得歧义大,重命名一次性改掉。

**文件**:
- 新增 `src-tauri/src/runtime/tools/builtin/task_stop.rs`(已存在但内容可能只有 placeholder;改写)
- 在 register_builtin_tools 确认注册
- 新增 `src-tauri/tests/task_stop_test.rs`

**Step-by-step**:

- [ ] **Step 1**:实现 RuntimeTool。入参 `agent_name: String`。execute:

```rust
let session = ctx.session_id().clone();
let target_id = ctx.agent_names().resolve(&session, &args.agent_name).await
    .ok_or_else(|| ToolError::msg(format!("no agent named `{}`", args.agent_name)))?;
let cancel = ctx.cancellation_registry().get(&target_id)
    .ok_or_else(|| ToolError::msg("target has no cancellation token (not a Teammate?)"))?;
cancel.cancel();
Ok(ToolOutput::json(json!({ "stopped": args.agent_name })))
```

- [ ] **Step 2**:tool description 写"仅 Lead 该用,且仅在 shutdown_response=false 之后或紧急情况"。
- [ ] **Step 3**:写 `task_stop_test.rs`,3 case:
  - 正常 TaskStop → Teammate idle loop 退出 + Team 清理 + name registry 清理
  - TaskStop 不存在的 name → 报错
  - TaskStop 已经退出的 Teammate → 静默 ok(idempotent)
- [ ] **Step 4**:cargo test 绿。
- [ ] **Step 5**:commit。

```bash
git add -A
git commit -m "feat(tools): TaskStop forcibly cancels a Teammate by name

Trips the target's CancellationToken, which causes worker_runtime's
idle loop to take the cancelled branch and run cleanup.  Idempotent
(stopping an already-stopped Teammate succeeds silently).

Refs: 2026-05-10 plan §3.C C8"
```

---

### P2.8 — `is_async` flag + permission Ask auto-deny

**目标**:Teammate(和 async subagent)的 RunCtx 上挂 `is_async: bool = true`;Lead(用户/cron 派活)永远 `false`。permission 决策点遇到 Ask 时,如果 `is_async`,直接 auto-deny;否则正常弹 ask。

**文件**:
- 改 `src-tauri/src/runtime/run_ctx.rs`(或等价 RunCtx 定义处)加字段
- 改 `src-tauri/src/runtime/permissions/decision.rs`(或同义文件)插 auto-deny
- 改 P1.6 `run_teammate_idle` 初始化时 `is_async=true`
- 新增 `src-tauri/tests/is_async_auto_deny_test.rs`

**Step-by-step**:

- [ ] **Step 1**:`RunCtx` 加 `pub is_async: bool`。默认 false;Builder 加 `with_async(bool)`。
- [ ] **Step 2**:`run_teammate_idle` 构造 ctx 时 `.with_async(true)`。
- [ ] **Step 3**:**permission Ask 入口** 加判定:

```rust
if let Decision::Ask { .. } = decision {
    if ctx.is_async {
        return Decision::Deny { reason: "async runner cannot ask user; auto-deny".into() };
    }
}
```

- [ ] **Step 4**:写 `is_async_auto_deny_test.rs`,3 case:
  - Lead Ctx (is_async=false) 遇到 Ask → 正常 emit PermissionAskRequired 事件
  - Teammate Ctx (is_async=true) 遇到 Ask → Decision::Deny,**不** emit 事件
  - async subagent (老路径,也应是 is_async=true) 遇到 Ask → 同 Teammate
- [ ] **Step 5**:cargo test 绿。
- [ ] **Step 6**:commit。

```bash
git add -A
git commit -m "feat(permissions): auto-deny user-Ask when RunCtx.is_async = true

Teammates and async subagents have no UI thread to surface a
permission prompt to.  When the decision engine would Ask, we now
deny instead and stamp the reason 'async runner cannot ask user'.
Lead runs (is_async=false) are unchanged.

Refs: 2026-05-10 plan §7.4"
```

---

### P2.9 — plan_approval_request / plan_approval_response

**目标**:Teammate 在执行高风险/不可逆动作前,可以 `SendMessage(to=team-lead, plan_approval_request{plan})` 让 Lead 看一眼。Lead 回 `plan_approval_response{approve, feedback}`。**纯结构化消息流转,不走 permission system**(permission 是 sync 的会卡 turn,这里是 async)。

**文件**:
- 不新增代码模块(P2.1 已经定义这两个 variant)
- 在 worker_runtime / chat_turn_driver 里把这两种 variant 当 ChatMessage 注入(LLM 自己理解 XML)
- 新增 `src-tauri/tests/plan_approval_roundtrip_test.rs`

**Step-by-step**:

- [ ] **Step 1**:在 worker_runtime 处理 InboxItem::ChatMessage 时,识别 PlanApprovalRequest / PlanApprovalResponse → 转 XML 注入(类似 task-notification):

```xml
<plan-approval-request id="pa_xxx" from="researcher">
  <plan>我打算 rm -rf /tmp/cache,需要确认</plan>
</plan-approval-request>
```

- [ ] **Step 2**:tool description 在 SendMessage 里更新文案,提醒 LLM 这两个 variant 的使用场景。
- [ ] **Step 3**:写 `plan_approval_roundtrip_test.rs`,2 case:
  - Teammate 发 PlanApprovalRequest → Lead inbox 收到 XML 形式 + Lead idle 被唤起
  - Lead 回 PlanApprovalResponse → Teammate inbox 收到 XML 形式 + Teammate turn 续起
- [ ] **Step 4**:cargo test 绿。
- [ ] **Step 5**:commit。

```bash
git add -A
git commit -m "feat(messaging): plan_approval handshake via StructuredMessage

The two plan_approval variants travel through SendMessage but render
as XML user messages on the receiving side, letting the LLM reason
about them naturally without coupling to the permission system.

Refs: 2026-05-10 plan §3.G, §5.7"
```

---

### P2.10 — 广播 `to:"*"`

**目标**:`SendMessage(to: "*", message)` fan-out 到当前 Team 所有 **Teammate**(不含 Lead 自己,也不含发送者自己)。MVP 朴素实现:遍历 Team.teammates,逐个 push;不做合并。

**文件**:
- 改 `runtime/tools/builtin/send_message.rs`(P2.2 已留位)
- 新增 `src-tauri/tests/broadcast_test.rs`

**Step-by-step**:

- [ ] **Step 1**:`broadcast` 函数实现:

```rust
async fn broadcast(session: &SessionId, from: &str, msg: StructuredMessage, ctx: &ToolCtx) -> ToolResult {
    let team = ctx.team_registry().get(session).await
        .ok_or_else(|| ToolError::msg("broadcast requires a Team"))?;
    let team = team.lock().await;
    let mut delivered = 0;
    for m in team.teammates.iter() {
        if m.name == from { continue; }   // 自己跳过
        let inbox = ctx.inbox_registry().get(&m.agent_id).await;
        if let Some(inbox) = inbox {
            inbox.push(InboxItem::ChatMessage(msg.clone(), MessageSource::Lead /* 或 Teammate(from) */)).await;
            delivered += 1;
        }
    }
    Ok(ToolOutput::json(json!({ "delivered_to_count": delivered, "to": "*" })))
}
```

> Lead 不收广播:这是 v4 §3.C / §5.9 的明确决定。Lead 想主动观察 Teammate 之间的通信,通过 transcript / 日志即可。

- [ ] **Step 2**:写 `broadcast_test.rs`,4 case:
  - Lead 广播 → 所有 Teammate 收到 + Lead 自己不收
  - Teammate1 广播 → Teammate2/3/4 收到 + Teammate1 自己不收 + Lead 不收
  - 无 Team → 报错
  - 0 个 Teammate → delivered_to_count=0,不报错
- [ ] **Step 3**:cargo test 绿。
- [ ] **Step 4**:commit。

```bash
git add -A
git commit -m "feat(send_message): support broadcast via to:'*'

Fans out to every Teammate in the current Team, excluding the sender
and the Lead.  Naive iteration; no coalescing yet.  Aligns with
claude-code-best swarm semantics.

Refs: 2026-05-10 plan §3.C C9, §5.9"
```

---

### P2.11 — 端到端冒烟(§1.5 验收)

**目标**:用真实 LLM(建议 lotus/Doubao 或 deepseek-v4)跑 §1.5 三条 E2E 验收:

1. **场景 A — 单 Teammate 帮 Lead 调研**:用户问"调研下 claude-code-best 的 Agent Teams 机制" → Lead TeamCreate → spawn 1 Teammate(小研)→ Teammate TaskClaim → 完成 → SendMessage 回 Lead → Lead 汇总回复用户。
2. **场景 B — 多 Teammate swarm**:用户给一个复合任务 → Lead 切 3 个子 Task → spawn 3 Teammate → Teammate 之间 SendMessage 协作 → 汇总。
3. **场景 C — shutdown 握手 + plan_approval**:Lead 让某 Teammate 做一个有 plan_approval 的操作 → 走完整请求/批准链路 → 收尾 shutdown handshake。

**文件**:
- 新增 `docs/test-intents/spec/tasks/ltr-e2e/rules.md`(写 E2E 意图断言,不写代码)
- 新增 `docs/test-intents/spec/tasks/ltr-e2e/test-progress.md`(手测记录)

**Step-by-step**:

- [ ] **Step 1**:写 rules.md(对照 §1.5,产品视角断言)。
- [ ] **Step 2**:启 `pnpm tauri:dev`,准备一个 EmployeeRecord(小研,白名单含 SendMessage/TaskList/TaskGet/TaskClaim/TaskUpdate)。
- [ ] **Step 3**:手测场景 A:
  - 看 Team.teammates 长度 == 1
  - 看 transcript JSONL(`~/.renlijia/users/{scope_key}/conversations/{conv}/teammates/agent-{agent_id}.jsonl`)产生消息
  - 看 Lead 续 turn 后产出汇总文字
- [ ] **Step 4**:手测场景 B:看 3 个 Teammate 各自 transcript + 至少 1 次 Teammate↔Teammate SendMessage 出现在 transcript。
- [ ] **Step 5**:手测场景 C:看 plan_approval XML 流转 + Lead approve=true 后 Teammate 继续 + shutdown handshake 完整。
- [ ] **Step 6**:把每个场景的 transcript 摘录到 test-progress.md,标记 PASS/FAIL。
- [ ] **Step 7**:E2E 全绿 → commit 文档。

```bash
git add -A
git commit -m "docs(test-intents): record LTR end-to-end smoke results (P2.11)

Three real-LLM scenarios from §1.5 (single Teammate research,
multi-Teammate swarm, plan_approval + shutdown handshake) all passed
manually.  Transcripts attached to test-progress.md for audit.

Refs: 2026-05-10 plan §1.5, §10"
```

---

### P2 完成检查

- [ ] `cd src-tauri && cargo test review_ --tests --no-fail-fast` 全绿
- [ ] `cd src-tauri && cargo test --tests --no-fail-fast` 整体不退化
- [ ] 新增 test 文件全 PASS:`structured_message_roundtrip_test`、`send_message_routing_test`、`teammate_addendum_present_test`、`lead_idle_trigger_test`、`task_notification_to_lead_test`、`shutdown_handshake_test`、`task_stop_test`、`is_async_auto_deny_test`、`plan_approval_roundtrip_test`、`broadcast_test`
- [ ] `pnpm test` 前端不退化
- [ ] git log 看到 P2 期 ≥ 10 个 commit
- [ ] §1.5 三条 E2E 全部手测通过(test-progress.md PASS)
- [ ] v4 §10 完成标志 9 项全部勾完

P2 通过 → LTR MVP 交付完毕,准备 PR/合并。

---

## 后续优化项(非本期)

以下条目从 v4 / claude-code-best 对照中识别,**本期不做**,留给后续迭代:

| # | 项 | 来源 | 估时 |
|---|---|---|---|
| F1 | Session GC 自动 tick(60s 检测无 owner 自动清理) | v4 §7.3 行 3 | 1h |
| F2 | 广播 fan-in coalescing(同一 turn 内多条 task-notification 合并) | v4 §3.G G3 | 1.5h |
| F3 | Teammate 跨 Team 复用(放开 MVP "限 1 个 Team" 限制) | v4 §4.4 | 2h |
| F4 | Lead transcript 也单独存 JSONL(目前只有 conversation messages) | claude-code-best 审计需求 | 1h |
| F5 | TeamDelete 之后 archive Team metadata(用于历史回溯) | v4 §6.2 暗示 | 1h |
| F6 | Subagent type 注册表 + Employee 在 UI 配 tool_whitelist 的检查(目前是后端硬校验,前端无提示) | UX 改进 | 2h |
| F7 | Teammate 资源占用监控(同时 4 个 idle loop 长跑的 memory/CPU baseline) | 风险 §9.1 | 2h |

---

## 附录 A — 测试矩阵索引

| Phase | 新增测试文件 | 验证项 |
|---|---|---|
| P0 | `review_mcp_tool_cancel_test`、`review_compact_summary_trait_isolation_test` | MCP cancel select + compact_summary 解耦 |
| P1 | `review_team_registry_session_isolation_test`、`team_json_persistence_test`、`agent_name_registry_test`、`spawn_teammate_via_employee_test`、`teammate_required_tools_test`、`task_claim_test`、`task_blocks_cycle_detection_test`、`review_task_storage_per_conv_test`、`teammate_idle_loop_skeleton_test`、`transcript_path_routing_test`、`team_tools_test`、`session_cleanup_test` | Team + team.json 持久化 + 共享 Task + Teammate idle 骨架 + transcript 路径区分 |
| P2 | `structured_message_roundtrip_test`、`send_message_routing_test`(含磁盘审计)、`teammate_addendum_present_test`、`team_context_attachment_test`、`lead_idle_trigger_test`、`task_notification_to_lead_test`、`shutdown_handshake_test`、`task_stop_test`、`is_async_auto_deny_test`、`plan_approval_roundtrip_test`、`broadcast_test` | 通信链路 + team_context attachment + inbox 磁盘审计 + idle 触发 + 结构化消息 + is_async |
| E2E | `docs/test-intents/spec/tasks/ltr-e2e/rules.md` + test-progress.md | §1.5 三条端到端 |

---

## 附录 B — 关键文件改动清单(Phase 视角)

| 文件 | P0 | P1 | P2 |
|---|---|---|---|
| `runtime/mcp/runtime_tool.rs` | ✏️ (cancel select) | | |
| `runtime/chat/chat_turn_driver.rs` | ✏️ (compact_summary 抽离) | | ✏️ (Lead idle 路径 A) |
| `runtime/chat/compact_client.rs` | ➕ 新 | | |
| `runtime/agent/team.rs` | | ✏️ 重写 + 持久化 | |
| `runtime/agent/name_registry.rs` | | ➕ 新 | |
| `runtime/agent/required_tools.rs` | | ➕ 新 | |
| `runtime/agent/team_context.rs` | | | ➕ 新(P2.3b) |
| `runtime/agent/output_writer.rs` | | ✏️ (kind 路径分流 + meta sidecar) | |
| `storage/user_scoped_paths.rs` | | ✏️ (teammates_dir / subagents_dir) | |
| `runtime/messaging/inbox_audit.rs` | | | ➕ 新(P2.2 磁盘审计) |
| `runtime/agent/worker_runtime.rs` | | ✏️ (TeammateIdle mode) | ✏️ (shutdown / addendum / team_context inject) |
| `runtime/agent/inbox.rs` | | ✏️ (InboxItem enum kind) | ✏️ (StructuredMessage 升级) |
| `runtime/agent/lead_idle.rs` | | | ➕ 新 |
| `runtime/agent/async_task_store.rs` | | | ✏️ (LeadIdle 接入) |
| `runtime/agent/task_notification.rs` | | | ✏️ (emit_to_lead) |
| `runtime/agent/teammate_addendum.rs` | | | ➕ 新 |
| `runtime/tools/builtin/spawn_subagent.rs` | | ✏️ (name/team_name/employee_id) | |
| `runtime/tools/builtin/task_tools.rs` | | ✏️ (claim + cycle) | ✏️ (notification emit) |
| `runtime/tools/builtin/team_tools.rs` | | ➕ 新 | |
| `runtime/tools/builtin/send_message.rs` | | | ➕ 新 |
| `runtime/tools/builtin/task_stop.rs` | | | ✏️ 实装 |
| `runtime/employee/store.rs` | | ✏️ (只读接口) | |
| `runtime/messaging/structured.rs` | | | ➕ 新 |
| `runtime/run_ctx.rs` (或等价) | | | ✏️ (is_async) |
| `runtime/permissions/decision.rs` (或等价) | | | ✏️ (Ask auto-deny) |
| `runtime/session_runtime.rs` | | ✏️ (cleanup) | |
| `src-tauri/src/lib.rs` | | ✏️ (app-close hook + manage 新 registry) | |

---

## 附录 C — 给执行人的提示

1. **每个 Phase 一个 PR**:P0 / P1 / P2 各开一个 PR,reviewer 负担小。
2. **每个 Task 一个 commit**(可拆,但不要合并)。commit message 都给出了模板,基本可直接抄。
3. **遇到 design 冲突先停**:如果实施过程中发现某 Task 的设计与 v4 文档冲突,**先**更新 v4 文档,**再**继续 Task。不要在实施代码里偷偷改设计。
4. **review_ 测试是架构红线**:任何 review_ 测试失败,等同于"破坏架构约束",必须修代码而非修测试。
5. **超时预警**:Phase 总估时 P0=3h / P1=11.5h / P2=14h,合计 28.5h(约 4 个工作日);若实际超出 50%,停下来检查是不是地基不对(可能要回头改 v4)。
6. **可并行**:P1.5 与 P1.1/P1.2 完全独立,可并行;P2.7 / P2.8 / P2.9 / P2.10 互相独立,可并行(都依赖 P2.2)。
7. **不要重复造轮子**:task_tools / task_notification / async_task_store / inbox / tool_whitelist / spawn_subagent / output_writer / required_tools(本期新增) 都是已有/即将有的;改它们,不要新建同名文件。


