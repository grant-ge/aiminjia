# Mode B P1 修复计划（codex review 后）

**日期**：2026-05-02
**前置 review**：codex 对 Mode B 大计划 d58b3e8..78235d7 做完整 review，给出 14 条 finding（5 P1 + 9 P2），结论 REQUEST_CHANGES。
**Master plan**：`docs/superpowers/plans/2026-04-30-lotus-subagent-mode-b-master-plan.md`
**前序 handoff**：`docs/superpowers/plans/2026-04-30-mode-b-progress-handoff.md`
**verifier**：本计划提示词作者已逐项 grep + Read 验证 codex finding 属实（详见 §0）。

## 0. Finding 验证结论（已属实）

| # | Finding | 验证方式 | 状态 |
|---|---|---|---|
| F1 | P4 白名单执行期失效 | `worker_runtime.rs:107` 用原始 `config.allowed_tools`；`final_allowed`（line 122-128）只用于 line 132 schema filter | 属实 |
| F2 | TaskNotificationQueue 全局 drain_all 跨会话泄露 | `task_notification.rs:81-84` `QueuedNotification { agent_id, xml }` 无 SessionId/RunId | 属实 |
| F3 | task_output 路径穿越 | `task_output.rs:49-66` 直接 `output_writer::transcript_path(&dir, task_id)`，未校验 `..` `/` `\` | 属实 |
| F4 | 初始 notification 注入顺序错位 | `chat_turn_driver.rs:1177-1180` 注入位置在 `extend(history)` 之后、`push(user_message)` 之前 | 属实 |
| F5 | tool round 后两条取消路径吞 notification | `chat_turn_driver.rs:1566-1569`（CP-2）无 re_enqueue；line 1593 附近 staged 路径同问题 | 属实 |
| F6 | explore 工具名与 catalog 不匹配 | explore.rs 用 `read_file/grep/glob`；catalog 实际是 `read_workspace_file/grep_content/search_files` | 属实 |
| F7 | ToolRoundDriver 不查 is_concurrency_safe | `tool_round_driver.rs:135` 仅按 `permitted.len() <= 1` 分串/并 | 属实，比 codex 描述更严重 |
| F8 | spawn_subagent catalog 仍说 async 未实现 | catalog.rs:573, 599 文案 "暂未实现/not_implemented_yet 占位符" | 属实 |
| F9 | Async terminal state 早于 transcript ready | spawn_subagent.rs Ok/Err/Panic 三分支均 update_state → append_line → enqueue | 属实 |
| F10 | transcript 写失败被 silent 吞 | `let _ = output_writer::append_line(...)` 三处 | 属实 |
| F11 | review_agent_b_constraints 假安全感（worker_runtime 仍用 tauri） | worker_runtime.rs 65/715/722/736/743 `tauri::AppHandle/Emitter` | 属实 |
| F12 | agents_dir 启动未创建 | `aijia_home.rs::ensure_user_dirs` 不含 agents/ | 属实（待 Read 二次确认） |
| F13 | Markdown loader 未拒绝空字段 | markdown_loader.rs:66-90 仅 serde 字段必需，无 trim/非空校验 | 属实（待 Read 二次确认） |
| F14 | P10 review_ 测试覆盖空洞 | review_agent_b_constraints.rs:10-28 硬编码 7 路径 | 属实 |

> F12/F13 在写片提示词时由 implementer 二次 Read 确认；其余 12 项已直读。

## 1. Phase 总览

修复分为 3 个 milestone（M1/M2/M3），M1 解决高危安全/正确性问题，M2 解决一致性/合同断点，M3 收尾对齐。每个 milestone 内的片可独立 commit。

| Milestone | 主题 | 必要前置 | 完成后软件状态 |
|---|---|---|---|
| **M1** | 安全与执行期正确性（F1, F3, F2, F5, F4） | — | 子 agent 工具白名单在执行期生效；task_output 不可路径穿越；通知不会跨会话泄露/丢失 |
| **M2** | 合同对齐（F7, F6, F8, F9, F10） | M1 | 并发安全合同贯穿生产路径；explore 工具名正确；catalog 文案准确；async lifecycle 顺序正确；transcript 写失败可观察 |
| **M3** | 架构约束 + 数据校验（F11, F12, F13, F14） | — | review_ 真扫整个 runtime/；agents_dir 启动创建；malformed .md fail-closed |

## 2. 执行参数

| 维度 | 设置 |
|---|---|
| 工作目录 | `/Users/a20250311/.codex/worktrees/4dc8/lotus-app` |
| 分支 | main，禁止开 worktree |
| Implementer 模型 | 按复杂度选 haiku/sonnet（机械活 haiku，多文件 TDD/重构 sonnet） |
| Reviewer 模型 | sonnet（除架构问题用 opus） |
| 并行规则 | 串行（避免 git reset 互相污染） |
| 顺序约束 | M1 内：F1 → F3 → F2 → F5 → F4（F2 改动 QueuedNotification 结构会影响 F5 的 re_enqueue，所以先 F2）。修正：**F2 先于 F5**。最终：**F1 → F3 → F2 → F5 → F4**。M2 内：**F7 → F6 → F8 → F9 → F10**（F7 改 ToolRoundDriver 后 F6 才能补"真实 final tool_defs"测试）。M3 任意顺序。 |
| 每片节奏 | 派 implementer → cargo build & test → commit → 派 reviewer → 据 review 决定是否 follow-up |

---

## 3. Milestone M1 — 安全与执行期正确性

### M1.1 — F1 修复：worker_runtime 执行期使用 final_allowed

**问题**：`build_turn_request` 算出 `final_allowed`（已应用 ALL_AGENT_DISALLOWED + ASYNC_AGENT_ALLOWED + 默认禁递归 spawn）后只用于过滤 schema；`build_run_config` 仍用原始 `config.allowed_tools` 塞进 `WorkerRunConfig.allowed_tools`，下游 `ToolRoundDriver::with_allowed_tools` 拿到的是未过滤值。

**预估**：2 文件，~30 行净增（含一个 worker_runtime 单测）。

#### 步骤

**步骤 1**：`src-tauri/src/runtime/agent/worker_runtime.rs` 改造 `run_inner`（line 95-103）：
```rust
async fn run_inner(&self, config: SubAgentConfig)
    -> std::result::Result<SubAgentResult, LegacyToolError>
{
    let all_schemas = self.tool_registry.get_all_schemas().await;
    let available_names: Vec<String> = all_schemas.iter().map(|s| s.name.clone()).collect();

    let final_allowed = crate::runtime::agent::tool_whitelist::resolve_agent_tools(
        &config.allowed_tools,
        &config.disallowed_tools,
        &available_names,
        config.background,
        false,
    );

    let turn_request = self.build_turn_request_with_allowed(&config, all_schemas, &final_allowed);
    let run_config = Self::build_run_config_with_allowed(&config, final_allowed);
    self.run_worker_turn(turn_request, run_config).await
}
```

**步骤 2**：抽出 `build_turn_request_with_allowed(config, all_schemas, final_allowed)` 和 `build_run_config_with_allowed(config, final_allowed)` —— 后者把 `WorkerRunConfig.allowed_tools` 设为 `final_allowed.clone()`。删掉旧版 `build_turn_request` / `build_run_config`（无其他调用方）。

**步骤 3**：在 `worker_runtime.rs` 测试模块中新增单测：
```rust
#[tokio::test]
async fn final_whitelist_is_used_in_run_config_and_turn_request() {
    // build runtime with a fake registry exposing read_workspace_file + spawn_subagent
    // call run_inner with config.allowed_tools = vec![] (= 全集) + background=false + allow_recursive_spawn=false
    // assert: WorkerRunConfig.allowed_tools 不包含 "spawn_subagent"
    //         WorkerTurnRequest.tool_defs 也不包含
}
```
（如果 worker_runtime 单测需要太多 mock，改成放在 `tests/worker_runtime_whitelist_test.rs` 集成测试）

#### 验证
```bash
cd src-tauri && cargo build --tests 2>&1 | tail -5
cd src-tauri && cargo test --lib runtime::agent::worker_runtime 2>&1 | tail -10
cd src-tauri && cargo test --test spawn_subagent_async_test --test spawn_subagent_tool_basic_test --test e2e_spawn_subagent_explore --test e2e_spawn_subagent_async --no-fail-fast 2>&1 | tail -15
```

#### Commit
```
fix(agent): apply final_allowed whitelist to WorkerRunConfig (P4 execution-time enforcement)
```

#### 禁止
- 不动 tool_whitelist.rs 算法本身。
- 不改 SubAgentConfig 字段。
- 不改 ToolRoundDriver（M2.1 干）。

---

### M1.2 — F3 修复：task_output 校验 task_id

**问题**：`task_output` 直接拿模型传入的 `task_id` 拼路径，可读 scope 内任何 jsonl（如 `../conversations/xxx/messages.0.jsonl`）。

**预估**：1 文件 + 1 测试文件追加，~25 行。

#### 步骤

**步骤 1**：`src-tauri/src/runtime/tools/builtin/task_output.rs` `execute` 方法在 line 54 后插入校验：
```rust
fn validate_task_id(s: &str) -> Result<&str, ToolError> {
    if s.is_empty()
        || s.contains('/')
        || s.contains('\\')
        || s.contains('\0')
        || s == "."
        || s == ".."
        || s.split(|c: char| c == '_' || c == '-' || c == '.')
            .all(|seg| seg.is_empty())
    {
        return Err(ToolError::ExecutionFailed(format!(
            "invalid task_id: {s:?} (must not contain path separators or be empty)"
        )));
    }
    Ok(s)
}
let task_id = validate_task_id(task_id)?;
```

**注意**：保留原本 `agent-{uuid}` / `stub-{uuid}` 形态可通过校验（含 `-` 和 `.` 但不含 `/` `\`）。

**步骤 2**：在 `tests/task_output_tool_test.rs` 追加 3 个负向测试：
- `rejects_path_traversal_dotdot` — task_id="../foo" → ExecutionFailed
- `rejects_absolute_path_separator` — task_id="/etc/passwd" → ExecutionFailed
- `rejects_backslash_separator` — task_id="..\\foo" → ExecutionFailed

#### 验证
```bash
cd src-tauri && cargo test --test task_output_tool_test 2>&1 | tail -10
cd src-tauri && cargo test --lib runtime::tools::builtin::task_output 2>&1 | tail -10
```
预期：原 4 + 新 3 = 7 集成测试 pass；7 单测仍 pass。

#### Commit
```
fix(tools): reject path traversal in task_output.task_id (security)
```

#### 禁止
- 不改 output_writer.rs（防御应在 tool 边界）。
- 不引入新依赖（如 dunce::canonicalize）—— 仅字符串校验即可。

---

### M1.3 — F2 修复：TaskNotificationQueue 按 SessionId/RunId 路由

**问题**：QueuedNotification 无 session/run，跨会话 drain_all 会窜话。

**预估**：3-4 文件，~80 行。**这是结构性改动，sonnet implementer。**

#### 步骤

**步骤 1**：`src-tauri/src/runtime/agent/task_notification.rs` 改造：
```rust
use crate::runtime::ids::{SessionId, RunId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedNotification {
    pub agent_id: String,
    pub xml: String,
    pub parent_session_id: SessionId,
    pub parent_run_id: Option<RunId>,
}

impl TaskNotificationQueue {
    pub fn enqueue(
        &self,
        agent_id: impl Into<String>,
        xml: impl Into<String>,
        parent_session_id: SessionId,
        parent_run_id: Option<RunId>,
    ) { ... }

    /// Drain only notifications belonging to the given session.
    /// Other sessions' notifications are preserved in the queue.
    pub fn drain_for_session(&self, session_id: &SessionId) -> Vec<QueuedNotification> {
        let mut guard = self.inner.lock().expect("notification queue poisoned");
        let mut keep = Vec::with_capacity(guard.len());
        let mut taken = Vec::new();
        for n in std::mem::take(&mut *guard) {
            if &n.parent_session_id == session_id { taken.push(n); }
            else { keep.push(n); }
        }
        *guard = keep;
        taken
    }

    /// Re-enqueue (used by chat_turn_driver when a turn cancels mid-flight).
    pub fn re_enqueue(&self, items: Vec<QueuedNotification>) { ... }

    pub fn pending_count(&self) -> usize { ... }
}
```
保留 `drain_all` 仅 `#[cfg(test)]` 用，或重命名为 `drain_all_for_test`。

**步骤 2**：`spawn_subagent.rs::launch_async` 在三个分支调 enqueue 时携带 parent session/run：
- 调用方需要传入 `SpawnSubagentContext`，确认 context 已有 session_id / run_id（如无，从 `SubAgentConfig.parent_run_id` 推导，session_id 通过 conversation_id → SessionId 转换）。**先 grep 验证 `SpawnSubagentContext` 字段**。

**步骤 3**：`chat_turn_driver.rs::drain_and_inject_task_notifications` 改签名加 session_id 参数：
```rust
fn drain_and_inject_task_notifications(
    queue: &TaskNotificationQueue,
    session_id: &SessionId,
    msgs: &mut Vec<ChatMessage>,
) -> Vec<QueuedNotification>
```
所有调用站点（line 1179, 1304）传入 `turn.session_id()`。`re_enqueue_task_notifications` 也加 session_id 参数（用 `re_enqueue` 不重新过滤，按原序入回）。

**步骤 4**：更新 `task_notification.rs` 单测和 `tests/task_notification_injection_test.rs`：
- 新增 `drain_for_session_only_returns_matching_session` —— enqueue 来自 session A 和 B 各 1 条，drain_for_session(&A) 应返回 1 条 A 的，B 的仍在队列。
- 修正既有测试以传入 session_id。

#### 验证
```bash
cd src-tauri && cargo build --tests 2>&1 | tail -5
cd src-tauri && cargo test --lib runtime::agent::task_notification 2>&1 | tail -10
cd src-tauri && cargo test --test task_notification_injection_test --test e2e_spawn_subagent_async --test spawn_subagent_async_test --no-fail-fast 2>&1 | tail -15
```

#### Commit
```
fix(agent): route TaskNotificationQueue by SessionId to prevent cross-session leakage (P7)
```

#### 禁止
- 不改 XML 结构（兼容 agent_id/output_file 等字段）。
- 不动 `output_writer.rs`。

---

### M1.4 — F5 修复：tool round 后两条取消路径补 re_enqueue

**问题**：chat_turn_driver.rs:1566-1569（CP-2 取消）和 line 1590 附近（staged tool result 取消）直接 break，跳过 `re_enqueue_task_notifications`，已 drain 的通知 silent 吞。

**前置**：M1.3 已完成（re_enqueue 签名变更）。

**预估**：1 文件，~10 行净增 + 1 测试。

#### 步骤

**步骤 1**：`chat_turn_driver.rs:1566-1570` CP-2 分支改为：
```rust
if cancel.is_cancelled() {
    state.append_messages_batch(vec![assistant_history_message.clone()]);
    re_enqueue_task_notifications(
        &self.task_notification_queue,
        std::mem::take(&mut pending_task_notifications),
    );
    mark_turn_cancelled_with_synthetic_results(&mut state, cancel.reason());
    break 'turn;
}
```

**步骤 2**：staged 路径（line ~1590-1596）同样在 break 前 re_enqueue。

**步骤 3**：新建 `tests/task_notification_cancel_paths_test.rs`，2 个测试：
- `cancel_after_drain_before_round_re_enqueues` — 模拟 drain 后立即 cancel，断言队列恢复 1 条。
- `cancel_during_staged_tool_result_re_enqueues` — 模拟 staged 路径 cancel，断言队列恢复。

  实现细节：用 stub round_driver 立即返回 + cancel.cancel_with_reason() 触发取消。如果 turn driver 单测不易构造，至少补一个集成测试覆盖 CP-2。**先评估可行性**，若构造成本高（>100 行），降级为单条 turn driver 集成测试。

#### 验证
```bash
cd src-tauri && cargo test --test task_notification_cancel_paths_test 2>&1 | tail -10
cd src-tauri && cargo test --test task_notification_injection_test 2>&1 | tail -10
```

#### Commit
```
fix(chat): re-enqueue task notifications on tool round cancellation paths (P7)
```

#### 禁止
- 不动 enqueue/drain 算法。
- 不改其他 cancel 路径（line 1410/1414/1432/1525 已经正确 re_enqueue）。

---

### M1.5 — F4 修复：初始 notification 注入位置改到 user_message 之后

**问题**：`chat_turn_driver.rs:1177-1180` 当前在 `extend(history)` 之后、`push(user_message)` 之前 drain 注入，使 `<task-notification>` 成为独立 user message，夹在 history 和当前 user message 之间。如果当前 user message 改变上下文，模型会优先响应当前 input 而忽略 async 完成。

**预估**：1 文件，~5 行净改 + 1 测试。

#### 步骤

**步骤 1**：把 line 1180 的 `initial_messages.push(user_message);` 移到 line 1178 之前：
```rust
initial_messages.extend(history);
initial_messages.push(user_message);
let mut pending_task_notifications =
    drain_and_inject_task_notifications(
        &self.task_notification_queue,
        turn.session_id(),
        &mut initial_messages,
    );
```
**注意**：`drain_and_inject_task_notifications` 应保证注入在末尾（即 user_message 之后）。如其当前实现是 `msgs.push(...)`，则只需移动 user_message push 顺序；如其内部 splice 到中间，需要调整其逻辑为追加。**先 Read 该函数实现确认。**

**步骤 2**：在 `tests/task_notification_injection_test.rs` 追加 1 测试：
- `initial_injection_order_after_user_message` — 构造 history + 1 pending notif + new user message，运行 turn 至 `initial_messages` 构造完，断言顺序：[system, ..., history..., user_message, task_notification_message]。

#### 验证
```bash
cd src-tauri && cargo test --test task_notification_injection_test 2>&1 | tail -10
```

#### Commit
```
fix(chat): inject task notifications after current user message (P7 ordering)
```

#### 禁止
- 不动其他注入点（mid-turn drain at line 1304 不受影响）。

---

## 4. Milestone M2 — 合同对齐

### M2.1 — F7 修复：ToolRoundDriver 查 is_concurrency_safe

**问题**：`tool_round_driver.rs:135` 仅按 `len <= 1` 决定串/并；P5 的 concurrency_safe 合同完全失效。所有非 concurrency-safe 工具（如绝大多数 builtin tool）会被并发执行。

**预估**：1 主文件 + 1 测试文件，~50 行。**sonnet implementer。**

#### 步骤

**步骤 1**：`tool_round_driver.rs:135-220` 重构 dispatch：
- 把 `permitted` 按 `RuntimeTool::is_concurrency_safe(&input)` 分两组：safe / unsafe。
- safe 组用 `join_all` 并发；unsafe 组逐条串行。
- 保持 `results.sort_by_key(|(idx, _)| *idx)` 顺序。
- `is_concurrency_safe` 查询通过 `query_engine.tool_concurrency_safe(&call.tool_name, &call.input)` 暴露（如该方法不存在则添加，转发到 dispatcher / registry）。

**步骤 2**：在 `tests/tool_round_driver_concurrency_test.rs`（新建）加 2 测试：
- `concurrency_safe_tools_dispatch_in_parallel` — 注册两个 safe tool，stub 各 sleep(50ms)，total < 80ms（并发）
- `non_safe_tools_dispatch_serially` — 注册两个 unsafe tool，stub 各 sleep(50ms)，total >= 100ms（串行）
- `mixed_safe_unsafe_preserves_order` — safe + unsafe 混合，索引顺序仍然正确

#### 验证
```bash
cd src-tauri && cargo test --test tool_round_driver_concurrency_test 2>&1 | tail -10
cd src-tauri && cargo test --test spawn_subagent_parallel_dispatch_test 2>&1 | tail -10
```

#### Commit
```
fix(chat): ToolRoundDriver respects RuntimeTool::is_concurrency_safe (P5 enforcement)
```

#### 禁止
- 不改 `dispatcher.rs::dispatch_batch`（已正确）。
- 不改 RuntimeTool trait 签名。

---

### M2.2 — F6 修复：explore 工具名对齐 catalog

**问题**：explore.rs:9-15 用 `read_file/grep/glob`，catalog 实际是 `read_workspace_file/grep_content/search_files`。resolve_agent_tools 按 available_names 过滤后 explore 只剩 `list_directory + web_search`。

**前置**：M2.1 完成（is_concurrency_safe 已生效，避免 explore 真正跑起来时遇到 race）。

**预估**：1 文件 + 1 测试，~15 行。**haiku implementer。**

#### 步骤

**步骤 1**：`src-tauri/src/runtime/agent/builtin/explore.rs` 把 allowed_tools 改为：
```rust
allowed_tools: vec![
    "read_workspace_file".into(),
    "grep_content".into(),
    "search_files".into(),
    "list_directory".into(),
    "web_search".into(),
],
```

**步骤 2**：在 `tests/e2e_spawn_subagent_explore.rs`（或新文件 `tests/explore_agent_tools_test.rs`）加 1 测试：
- `explore_final_tool_defs_include_read_and_grep` — 构造 worker_runtime，对 explore 跑 build_turn_request_with_allowed，断言 tool_defs 包含 `read_workspace_file` 和 `grep_content`。

  如 worker_runtime 内部不易直接调用，改写为：直接调 `resolve_agent_tools(&explore.allowed_tools, &[], &available_names_from_catalog, false, false)` 断言结果。

#### 验证
```bash
cd src-tauri && cargo test --test e2e_spawn_subagent_explore --test explore_agent_tools_test --no-fail-fast 2>&1 | tail -10
```

#### Commit
```
fix(agent): align explore.allowed_tools with current catalog tool names (P9)
```

#### 禁止
- 不改 catalog 工具名（catalog 名是事实标准）。
- 不改 general-purpose agent。

---

### M2.3 — F8 修复：catalog spawn_subagent async 文案

**问题**：catalog.rs:573, 599 文案 "暂未实现/not_implemented_yet 占位符" 是 stale；async 已通过 P6.2 实现。

**预估**：1 文件，~8 行改写 + 测试名更新。**haiku。**

#### 步骤

**步骤 1**：catalog.rs:570-573 改为：
```rust
"【Composite 工具】启动一个子 Agent 执行聚焦任务。\
\n\n适用场景：任务需要干净上下文、专属 Agent 类型（如 'explore'、'general-purpose'）或不同模型。\
\n\n同步路径（run_in_background=false 或省略）：等待子 Agent 完成并返回输出。\
\n\n异步路径（run_in_background=true）：立即返回 agent_id；子 Agent 后台运行；完成后通过 task_output(task_id=agent_id) 增量读 transcript，并在父下一轮收到 <task-notification>。"
```

**步骤 2**：catalog.rs:597-600 改 run_in_background 描述：
```rust
"description": "若为 true，异步运行并立即返回 agent_id；后续用 task_output 增量读结果。"
```

**步骤 3**：把测试 `run_in_background_true_returns_not_implemented_placeholder`（如还存在）改名 `run_in_background_true_returns_async_launched`，并把断言对齐当前实际返回值（status="async_launched"）。**先 grep 测试名是否仍存在；若已是 async_launched 名只改文案。**

#### 验证
```bash
cd src-tauri && cargo test --test spawn_subagent_tool_basic_test 2>&1 | tail -10
```

#### Commit
```
docs(catalog): update spawn_subagent description to reflect async launched (P2.3 stale doc)
```

#### 禁止
- 不改 schema（required/properties）。
- 不改 RuntimeTool 实现。

---

### M2.4 — F9 修复：async terminal state 顺序

**问题**：`spawn_subagent.rs::launch_async` 三个分支均 update_state(terminal) → append_line → enqueue。父收到 Completed 后立即 task_output 可能读到空文件。

**预估**：1 文件，~10 行调序 + 1 测试。**sonnet。**

#### 步骤

**步骤 1**：`src-tauri/src/llm/tool_executor/spawn_subagent.rs`，三个分支顺序改为：
```
1. let _ = output_writer::append_line(...);    // transcript first
2. notif_queue.enqueue(...);                   // then notification
3. self.task_store.update_state(&agent_id, terminal_state);  // last
```
理由：父侧观察 store state 用作就绪信号（虽然我们也修了 notif 路由，但仍要确保两条独立信号都成立）。

**步骤 2**：在 `tests/spawn_subagent_async_test.rs` 加 1 测试：
- `terminal_state_set_after_transcript_written` — 用 stub task_store 装一个 callback 在 update_state 时回查 transcript file 大小，断言 update_state 时文件已含至少 1 行。

  如 update_state callback 不易插桩，降级为：launch_async 完成后断言 store state 转换前已 append。**先 grep AsyncAgentTaskStore::update_state 是否可加 hook。**

#### 验证
```bash
cd src-tauri && cargo test --test spawn_subagent_async_test --no-fail-fast 2>&1 | tail -10
```

#### Commit
```
fix(agent): write transcript and enqueue notification before terminal state in launch_async (P6 ordering)
```

#### 禁止
- 不改 update_state 语义（仍然不删 entry）。
- 不动 notif 入队顺序（M1.3 已确定 enqueue 携带 session_id）。

---

### M2.5 — F10 修复：transcript 写失败可观察

**问题**：spawn_subagent.rs Ok/Err/Panic 三处 `let _ = append_line(...)` silent 吞错。

**前置**：M2.4 完成（顺序已是 append → enqueue → update_state）。

**预估**：1 文件，~10 行 + 1 测试。**haiku。**

#### 步骤

**步骤 1**：把三处 `let _ = ...` 改为：
```rust
if let Err(e) = output_writer::append_line(&transcript_path_for_task, &line) {
    log::warn!(
        "[spawn_subagent async {agent_id}] transcript append failed: {e}; downstream task_output may be empty"
    );
}
```

**步骤 2**：考虑是否在 `<task-notification>` XML 中加 `<transcript-status>unavailable</transcript-status>` —— 评估：如果 path 是 PathBuf::new()（degraded fallback），就在 enqueue 前 set status="unavailable"。这要求 build_task_notification_xml 加可选字段，**改动稍大；**如时间紧，第一版只加 log，XML 字段留 follow-up。

**步骤 3**：单测：在 task_notification 单测加 1 测试断言 build_task_notification_xml 接受新可选字段（如选择上面的方案 2）。

**实施时建议**：先做最小集 —— 仅 log::warn，不改 XML（降低耦合）。XML 改为 follow-up ticket。

#### 验证
```bash
cd src-tauri && cargo build --tests 2>&1 | tail -5
cd src-tauri && cargo test --test spawn_subagent_async_test --no-fail-fast 2>&1 | tail -10
```

#### Commit
```
fix(agent): log transcript append failures in async subagent (observability)
```

#### 禁止
- 不动 append_line 自身（仍 best-effort）。
- 不改 build_task_notification_xml 签名（除非选方案 2，那需另起小计划）。

---

## 5. Milestone M3 — 架构约束 + 数据校验

### M3.1 — F11 修复：review_agent_b_constraints 真扫整个 runtime/

**预估**：1 测试文件 + worker_runtime 标记，~30 行。**haiku。**

#### 步骤

**步骤 1**：`tests/review_agent_b_constraints.rs` 改 test 1：用 `walkdir` 递归扫 `src-tauri/src/runtime/`（**先确认 walkdir 在 dev-deps 已可用**；如无，改用 `std::fs::read_dir` 递归）。
```rust
#[test]
fn runtime_modules_do_not_use_tauri_directly() {
    // Allowlist for legacy modules pending refactor
    const LEGACY_ALLOWED: &[&str] = &[
        "src/runtime/agent/worker_runtime.rs", // P-runtime-host-trait pending
    ];
    let root = std::path::Path::new("src/runtime");
    for entry in walk_rs_files(root) {
        let rel = entry.strip_prefix(".").unwrap_or(&entry);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if LEGACY_ALLOWED.iter().any(|a| rel_str.ends_with(a)) { continue; }
        let src = std::fs::read_to_string(&entry).unwrap();
        assert!(
            !src.contains("use tauri::") && !src.contains("tauri::Manager") && !src.contains("tauri::Emitter") && !src.contains("tauri::AppHandle"),
            "{rel_str} must not use tauri::* directly (runtime layer purity); add to LEGACY_ALLOWED with TODO if intentional"
        );
    }
}

fn walk_rs_files(root: &Path) -> Vec<PathBuf> { /* recursive .rs scan */ }
```

**步骤 2**：在 `worker_runtime.rs` 顶部加 TODO 注释：
```rust
// TODO(P-runtime-host-trait): worker_runtime 仍直接持有 tauri::AppHandle 并调用 tauri::Emitter
// 用于 stream delta 转发；待将事件出口替换为 RuntimeHost trait / event sink 后即可移除。
// 这是 review_agent_b_constraints.rs::LEGACY_ALLOWED 的唯一例外。
```

#### 验证
```bash
cd src-tauri && cargo test --test review_agent_b_constraints 2>&1 | tail -10
```

#### Commit
```
test(agent): expand review_agent_b_constraints to scan all of runtime/ (with legacy allowlist)
```

#### 禁止
- 不真去重构 worker_runtime（这是独立专项）。
- 不改 transport/ 层 tauri 用法（不在范围）。

---

### M3.2 — F12 修复：ensure_user_dirs 创建 agents/

**预估**：1 文件 + 1 测试，~5 行。**haiku。**

#### 步骤

**步骤 1**：`src-tauri/src/storage/aijia_home.rs::ensure_user_dirs` 加：
```rust
std::fs::create_dir_all(paths.user_agents_dir())?;
```
（**先 grep 确认 user_agents_dir() 名字**）

**步骤 2**：相关单测断言激活 scope 后 agents/ 存在。

#### Commit
```
fix(storage): create agents_dir during ensure_user_dirs (P0.2)
```

---

### M3.3 — F13 修复：markdown_loader trim+非空校验

**预估**：1 文件 + 单测，~25 行。**haiku → sonnet 边界（看 deserialize 复杂度）。**

#### 步骤

**步骤 1**：`markdown_loader.rs` 在 deserialize 后做后置校验：
- name.trim() 不空
- description.trim() 不空
- system_prompt body 不空（trim 后）
- allowed_tools / disallowed_tools 各项 trim 不空
- model 字段如是 Fixed，其内 string trim 不空
- 任一项不通过 → 返回 Err 而非 Ok（调用方按现有 fail-closed 路径丢弃该 .md）

**步骤 2**：在 `tests/agent_registry_merge_test.rs` 或 `tests/markdown_loader_validation_test.rs` 加 4-5 个测试：
- `rejects_empty_name`
- `rejects_whitespace_only_description`
- `rejects_empty_system_prompt_body`
- `accepts_valid_definition`（确保正例不破）

#### Commit
```
fix(agent): reject malformed markdown agent definitions (P1.1 fail-closed)
```

---

### M3.4 — F14 follow-up（实际由 M3.1 覆盖；本片可省略）

M3.1 已直接增强 review_agent_b_constraints 覆盖面；F14 关掉。如果想补"async_agent_default_disallows_ask_user_question 加 def_allowed 显式包含 ask_user_question case"，可作为单独 ~5 行测试追加，**作为 M3.3 的 follow-up，不必单独 milestone。**

---

## 6. 自检清单（M1+M2 完成后必须通过）

```bash
cd src-tauri
cargo build --tests 2>&1 | tail -5

cargo test --lib runtime::agent::worker_runtime
cargo test --lib runtime::agent::task_notification
cargo test --lib runtime::tools::builtin::task_output

cargo test --test task_output_tool_test
cargo test --test e2e_spawn_subagent_async
cargo test --test e2e_spawn_subagent_explore
cargo test --test spawn_subagent_async_test
cargo test --test spawn_subagent_tool_basic_test
cargo test --test spawn_subagent_parallel_dispatch_test
cargo test --test task_notification_injection_test
cargo test --test task_notification_cancel_paths_test
cargo test --test tool_round_driver_concurrency_test
cargo test --test review_agent_b_constraints

# 回归（不应破坏）
cargo test --test agent_registry_merge_test
cargo test --test worker_runtime_test  # 如存在
cargo test review_ --tests --no-fail-fast  # 已知 2 pre-existing 失败，不计入
```

## 7. 重要架构事实（修复后的不变量）

1. **白名单执行期：`final_allowed` 来自 `resolve_agent_tools` 一次计算，被 `WorkerRunConfig.allowed_tools` 和 `WorkerTurnRequest.tool_defs` 共同消费**（M1.1 后）
2. **ToolNotificationQueue 按 SessionId 分桶**：`drain_for_session(&session_id)` 只取本 session 的，其他 session 的留队（M1.3 后）
3. **task_output.task_id 严格 AgentId 形态**：`/`, `\`, `..`, `.`, 空 都被拒绝（M1.2 后）
4. **launch_async 顺序**：append_line → enqueue → update_state（M2.4 后）
5. **ToolRoundDriver 真查 is_concurrency_safe**：safe 并发 / unsafe 串行（M2.1 后）
6. **review_agent_b_constraints 全 runtime/ 扫描**：worker_runtime 是 LEGACY_ALLOWED 唯一例外，待 P-runtime-host-trait 专项（M3.1 后）

## 8. 已知不在本计划范围

- **P-router-model-passthrough**：8 个 LLM provider 收敛 → 独立长期专项
- **P-runtime-host-trait**：worker_runtime 的 tauri 解耦 → 独立专项（M3.1 仅加 TODO 标记和 LEGACY_ALLOWED 例外）
- **F10 XML transcript-status 字段**：作为 follow-up
- **真 tokio::spawn lifecycle e2e 测试**（codex 阶段 2 提到的覆盖空洞）：作为 follow-up

## 9. 进度跟踪

| Milestone | 片 | Finding | Commit | Reviewer 判定 |
|---|---|---|---|---|
| M1 | M1.1 | F1 | _pending_ | _pending_ |
| M1 | M1.2 | F3 | _pending_ | _pending_ |
| M1 | M1.3 | F2 | _pending_ | _pending_ |
| M1 | M1.4 | F5 | _pending_ | _pending_ |
| M1 | M1.5 | F4 | _pending_ | _pending_ |
| M2 | M2.1 | F7 | _pending_ | _pending_ |
| M2 | M2.2 | F6 | _pending_ | _pending_ |
| M2 | M2.3 | F8 | _pending_ | _pending_ |
| M2 | M2.4 | F9 | _pending_ | _pending_ |
| M2 | M2.5 | F10 | _pending_ | _pending_ |
| M3 | M3.1 | F11 | _pending_ | _pending_ |
| M3 | M3.2 | F12 | _pending_ | _pending_ |
| M3 | M3.3 | F13 | _pending_ | _pending_ |

执行人在每片完成后填表。
