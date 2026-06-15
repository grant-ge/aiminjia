# Agent Foreground Auto-Background Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Agent 子任务默认以前台方式启动，超过前台阻塞预算后自动提升为后台任务，并保持 TaskOutput、TaskStop、transcript 和完成通知连续可用。

**Architecture:** 不新增第二套任务系统，复用 `AsyncAgentTaskStore`、`TaskOutput`、`TaskStop` 和 `TaskNotificationQueue`。`SpawnSubagentRuntimeTool` 仍保留显式 `run_in_background=true` 的立即后台路径；普通前台路径改为 `launch_foreground_auto_background`，短任务返回同步输出，超预算任务返回 `task_id/agent_id` 并继续在同一个后台 worker 中运行。

**Tech Stack:** Rust, Tauri runtime tools, Tokio, `tokio_util::sync::CancellationToken`, `serde_json`, Vitest/AEIT intent specs.

---

## 0. 对标语义

Claude code best 的核心不是“多一个布尔开关”，而是把 Agent task 当作可迁移的一等运行对象：

- 前台阶段先阻塞等待，方便短任务直接把完整结果交给父 agent。
- 到达前台预算后，父 agent 收到后台任务 id，当前轮可以继续规划。
- 后台任务完成后，通过消息队列或通知重新进入父 agent 的后续上下文。
- transcript 从前台阶段延续到后台阶段，读取和停止都指向同一个 task id。

AIjia 的实现不能直接复制 CLI/React singleton/message queue；本仓库应按现有 Rust runtime 控制面实现：

- 任务注册：`src-tauri/src/runtime/agent/async_task_store.rs`
- 后台输出读取：`src-tauri/src/runtime/tools/builtin/task_output.rs`
- 后台停止：`src-tauri/src/runtime/tools/builtin/task_stop.rs`
- 子 Agent 启动：`src-tauri/src/runtime/tools/builtin/spawn_subagent.rs`
- 生产 launcher：`src-tauri/src/llm/tool_executor/spawn_subagent.rs`

## 1. Agentic Loop 分工

### 主线程

- 维护本计划、分支、任务分派和最终 review。
- 每个子 agent 完成后先看 diff，再跑指定验证。
- 若实现与计划冲突，主线程决定是修计划还是退回子 agent。

### CodeWorker

- 目标：实现 runtime 行为和 Rust 回归测试。
- 只能修改 Rust runtime、Rust tests、tool catalog 文案。
- 不修改 `docs/test-intents/spec/**`，不修改 RepoWiki。
- 建议模型：强推理/代码模型，例如当前会话可用的高能力 Codex 模型；如果实际工具不支持指定模型，则使用默认 worker。

### IntentWorker

- 目标：补 Agent 自动后台化意图测试，并在代码实现后做 AEIT 验收。
- 只能修改 `docs/test-intents/spec/tasks/对话/rules.md` 和必要的 intent-test 辅助说明。
- 不修改 Rust runtime。
- 建议模型：测试/验收型模型，优先使用成本较低但能读规则的 worker；最终验收结果由主线程复核。

### ReviewWorker

- 目标：交叉验证 CodeWorker 与 IntentWorker 的产物是否满足本计划。
- 只做 review 报告，不改文件。
- 建议模型：高能力审查模型。

## 2. 文件改动边界

### CodeWorker 可改文件

- Modify: `src-tauri/src/runtime/tools/builtin/spawn_subagent.rs`
  - 新增前台自动后台化 outcome 类型。
  - 扩展 `SpawnSubagentRequest` 的测试阈值字段。
  - 普通前台路径改走可提升 launcher。
  - 返回 JSON 加 `assistant_auto_backgrounded`、`task_type` 和 `auto_background_after_ms`。

- Modify: `src-tauri/src/llm/tool_executor/spawn_subagent.rs`
  - 生产实现 `launch_foreground_auto_background`。
  - 提取后台完成/失败/ panic 的公共收尾 helper，供显式 async 和自动 promotion 复用。
  - 自动 promotion 时注册 `AsyncAgentTaskStore`，后续由 TaskOutput/TaskStop 读取同一 transcript。

- Modify: `src-tauri/src/runtime/tools/catalog.rs`
  - Agent 工具说明补充：默认前台先运行，超过预算自动后台；显式 `run_in_background=true` 仍立即后台。

- Modify: `src-tauri/tests/spawn_subagent_async_test.rs`
  - 更新 dispatcher contract 测试。

- Modify or Create: `src-tauri/tests/spawn_subagent_auto_background_test.rs`
  - 增加短任务前台返回、超预算自动后台、父取消、TaskOutput/TaskStop 入口兼容测试。

- Modify: `src-tauri/tests/e2e_spawn_subagent_async.rs`
  - 仅在需要共享 stub helper 时修改，保持显式 async 语义不回退。

### IntentWorker 可改文件

- Modify: `docs/test-intents/spec/tasks/对话/rules.md`
  - 新增 `意图-对话-029: 前台长子任务，自动转后台`。

## 3. 公开契约

### Agent 工具输入

保留既有输入：

```json
{
  "subagent_type": "explore",
  "prompt": "string",
  "description": "string",
  "run_in_background": false,
  "name": "optional name"
}
```

新增仅测试使用的隐藏字段：

```json
{
  "_auto_background_after_ms": 25
}
```

字段规则：

- 缺省和 `run_in_background=false`：前台先运行，超过预算自动后台。
- `run_in_background=true`：沿用现有立即后台路径，不经过前台等待预算。
- `_auto_background_after_ms`：仅用于 Rust tests 和 intent debug，生产 prompt/catalog 不主动暴露给用户。
- 生产默认预算：`15_000ms`。
- 测试最小预算：允许 `1ms`，小于 `1ms` 按 `1ms` 处理。

### Agent 工具输出

短任务前台完成：

```text
<sub agent final output>
```

自动后台化：

```json
{
  "status": "async_launched",
  "agent_id": "uuid",
  "task_id": "uuid",
  "task_type": "local_agent",
  "name": "optional name or null",
  "assistant_auto_backgrounded": true,
  "auto_background_after_ms": 15000
}
```

显式 `run_in_background=true` 继续返回兼容 JSON，但补充 `task_type`：

```json
{
  "status": "async_launched",
  "agent_id": "uuid",
  "task_id": "uuid",
  "task_type": "local_agent",
  "name": "optional name or null"
}
```

## 4. Task 1: Dispatcher Contract

**Files:**
- Modify: `src-tauri/src/runtime/tools/builtin/spawn_subagent.rs`
- Modify: `src-tauri/tests/spawn_subagent_async_test.rs`

- [ ] **Step 1: 写失败测试**

在 `src-tauri/tests/spawn_subagent_async_test.rs` 的 `RecordingLauncher` 增加 `foreground_auto_calls` 计数，并新增两个测试：

```rust
#[tokio::test]
async fn run_in_background_false_calls_foreground_auto_background() {
    let (tool, launcher) = build_tool();

    let ctx = ToolExecutionContext::for_test("sess", "run", "tc-foreground-false");
    let result = tool
        .execute(
            json!({
                "subagent_type": "explore",
                "prompt": "x",
                "description": "x",
                "run_in_background": false,
            }),
            ctx,
        )
        .await
        .expect("foreground auto path should succeed");

    assert_eq!(result.content, "sync-output");
    assert_eq!(launcher.foreground_auto_calls.load(Ordering::SeqCst), 1);
    assert_eq!(launcher.sync_calls.load(Ordering::SeqCst), 0);
    assert_eq!(launcher.async_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn foreground_auto_backgrounded_returns_task_json() {
    let (tool, launcher) = build_tool();
    launcher.set_foreground_auto_backgrounded("agent-auto-1");

    let ctx = ToolExecutionContext::for_test("sess", "run", "tc-foreground-auto");
    let result = tool
        .execute(
            json!({
                "subagent_type": "explore",
                "prompt": "x",
                "description": "x",
                "_auto_background_after_ms": 5,
            }),
            ctx,
        )
        .await
        .expect("foreground auto path should succeed");
    let body: Value = serde_json::from_str(&result.content).expect("json body");

    assert_eq!(body["status"], "async_launched");
    assert_eq!(body["agent_id"], "agent-auto-1");
    assert_eq!(body["task_id"], "agent-auto-1");
    assert_eq!(body["task_type"], "local_agent");
    assert_eq!(body["assistant_auto_backgrounded"], true);
    assert_eq!(body["auto_background_after_ms"], 5);
}
```

- [ ] **Step 2: 运行测试确认失败**

Run:

```powershell
cd src-tauri
cargo test --test spawn_subagent_async_test run_in_background_false_calls_foreground_auto_background -- --nocapture
```

Expected: FAIL，原因是 `SpawnSubagentLauncher` 没有 `launch_foreground_auto_background` 或前台路径仍调用 `launch_sync`。

- [ ] **Step 3: 新增 outcome 和 launcher 方法**

在 `src-tauri/src/runtime/tools/builtin/spawn_subagent.rs` 中加入：

```rust
#[derive(Debug, Clone)]
pub enum SpawnForegroundAutoOutcome {
    Completed(String),
    Backgrounded {
        agent_id: AgentId,
        name: Option<String>,
        auto_background_after_ms: u64,
    },
}
```

扩展 trait，保留默认实现以降低现有测试 stub 的改动面：

```rust
async fn launch_foreground_auto_background(
    &self,
    request: SpawnSubagentRequest,
    context: SpawnSubagentContext,
) -> Result<SpawnForegroundAutoOutcome> {
    self.launch_sync(request, context)
        .await
        .map(SpawnForegroundAutoOutcome::Completed)
}
```

- [ ] **Step 4: 给 request 加预算字段**

在 `SpawnSubagentRequest` 加字段：

```rust
pub auto_background_after_ms: Option<u64>,
```

解析输入：

```rust
let auto_background_after_ms = input
    .get("_auto_background_after_ms")
    .and_then(Value::as_u64)
    .map(|v| v.max(1));
```

构造 request 时传入该字段。

- [ ] **Step 5: 改普通前台路径**

把 `request.run_in_background == false` 的尾部逻辑改为：

```rust
match self
    .launcher
    .launch_foreground_auto_background(request, launch_ctx)
    .await?
{
    SpawnForegroundAutoOutcome::Completed(output) => Ok(ToolResult::new("Agent", output, None)),
    SpawnForegroundAutoOutcome::Backgrounded {
        agent_id,
        name,
        auto_background_after_ms,
    } => {
        let agent_id_str = agent_id.as_str().to_string();
        let json = serde_json::json!({
            "status": "async_launched",
            "agent_id": agent_id_str,
            "task_id": agent_id_str,
            "task_type": "local_agent",
            "name": name,
            "assistant_auto_backgrounded": true,
            "auto_background_after_ms": auto_background_after_ms,
        });
        Ok(ToolResult::new("Agent", json.to_string(), None))
    }
}
```

显式 async JSON 同步补 `task_type: "local_agent"`，不要加 `assistant_auto_backgrounded`。

- [ ] **Step 6: 跑 dispatcher 测试**

Run:

```powershell
cd src-tauri
cargo test --test spawn_subagent_async_test -- --nocapture
```

Expected: PASS。

## 5. Task 2: Production Promotion State Machine

**Files:**
- Modify: `src-tauri/src/llm/tool_executor/spawn_subagent.rs`
- Test: `src-tauri/tests/spawn_subagent_auto_background_test.rs`

- [ ] **Step 1: 写 production 行为测试**

创建 `src-tauri/tests/spawn_subagent_auto_background_test.rs`，用 fake launcher 或新 helper 覆盖三件事：

```rust
#[tokio::test]
async fn foreground_auto_completed_returns_sync_output() {
    let launcher = AutoBackgroundHarness::new()
        .with_subagent_delay_ms(1)
        .with_subagent_output("short-result");

    let result = launcher
        .launch_with_input(json!({
            "subagent_type": "explore",
            "prompt": "short",
            "description": "short",
            "_auto_background_after_ms": 50
        }))
        .await;

    assert_eq!(result.content, "short-result");
    assert!(launcher.task_store().list_active().is_empty());
}

#[tokio::test]
async fn foreground_auto_promotes_to_local_agent_task() {
    let launcher = AutoBackgroundHarness::new()
        .with_subagent_delay_ms(80)
        .with_subagent_output("long-result");

    let result = launcher
        .launch_with_input(json!({
            "subagent_type": "explore",
            "prompt": "long",
            "description": "long",
            "_auto_background_after_ms": 5,
            "name": "auto-agent-1"
        }))
        .await;
    let body: Value = serde_json::from_str(&result.content).expect("json body");
    let task_id = body["task_id"].as_str().expect("task_id");

    assert_eq!(body["assistant_auto_backgrounded"], true);
    assert_eq!(body["task_type"], "local_agent");
    assert!(launcher.task_store().find_by_name("auto-agent-1").is_some());

    launcher.wait_until_completed(task_id).await;
    let task_output = launcher.read_task_output(task_id, 0).await;
    assert!(task_output.to_string().contains("long-result"));
}

#[tokio::test]
async fn parent_cancel_before_promotion_cancels_child_without_registering_task() {
    let launcher = AutoBackgroundHarness::new()
        .with_subagent_delay_ms(80)
        .with_subagent_output("never-returned");

    let cancel = CancellationToken::new();
    let fut = launcher.launch_with_cancel_and_input(
        cancel.clone(),
        json!({
            "subagent_type": "explore",
            "prompt": "cancel",
            "description": "cancel",
            "_auto_background_after_ms": 200
        }),
    );
    cancel.cancel();

    let err = fut.await.expect_err("parent cancellation should fail foreground wait");
    assert!(err.to_string().contains("cancel"));
    assert!(launcher.task_store().list_active().is_empty());
}
```

若 fake harness 不能直接包住 production launcher，则先用 trait-level fake 测 dispatcher，再在 production launcher 中拆出 `run_foreground_auto_background_with_future` 私有 helper 并对 helper 做单测。

- [ ] **Step 2: 运行新测试确认失败**

Run:

```powershell
cd src-tauri
cargo test --test spawn_subagent_auto_background_test -- --nocapture
```

Expected: FAIL，原因是 production launcher 尚未实现 promotion。

- [ ] **Step 3: 提取后台收尾 helper**

在 `src-tauri/src/llm/tool_executor/spawn_subagent.rs` 中提取一个私有 async helper，显式 async 和自动 promotion 完成后都调用它：

```rust
struct SpawnBackgroundTaskCtx {
    task_store: Arc<AsyncAgentTaskStore>,
    notif_queue: Arc<TaskNotificationQueue>,
    agent_id: AgentId,
    transcript_path: std::path::PathBuf,
    parent_tool_use_id: String,
    parent_session_id: SessionId,
    parent_run_id: Option<RunId>,
    subagent_type: String,
}

async fn finish_background_subagent(
    ctx: SpawnBackgroundTaskCtx,
    outcome: std::thread::Result<anyhow::Result<crate::llm::sub_agent::SubAgentRunResult>>,
) {
    // 复用现有 launch_async match 三分支：
    // Ok(Ok(sub_result)) -> append assistant line, enqueue completed XML, update Completed
    // Ok(Err(e)) -> append failed line, enqueue failed XML, update Failed
    // Err(panic_payload) -> append failed line, enqueue failed XML, update Failed
}
```

这里的任务状态更新必须保持“写 transcript 和 enqueue notification 在前，`update_state` 在最后”的顺序。

- [ ] **Step 4: 实现 `launch_foreground_auto_background`**

生产 launcher 的状态机结构：

```rust
const DEFAULT_AGENT_AUTO_BACKGROUND_AFTER_MS: u64 = 15_000;

enum ForegroundPromotionState {
    Waiting(tokio::sync::oneshot::Sender<anyhow::Result<crate::llm::sub_agent::SubAgentRunResult>>),
    Promoted,
    Finished,
}
```

执行流程：

```rust
let auto_background_after_ms = request
    .auto_background_after_ms
    .unwrap_or(DEFAULT_AGENT_AUTO_BACKGROUND_AFTER_MS);
let (gateway, tool_registry, app_settings) = self.build_run_components()?;
let (mut config, runtime_deps) = self.build_sub_agent_args(&request, &context, true).await?;
let agent_id = AgentId::new(uuid::Uuid::new_v4().to_string());
let cancel_token = CancellationToken::new();
config.cancel_token = Some(cancel_token.clone());
let transcript_path = /* same as launch_async */;
let (done_tx, done_rx) = tokio::sync::oneshot::channel();
let state = Arc::new(tokio::sync::Mutex::new(ForegroundPromotionState::Waiting(done_tx)));
```

Spawn worker：

```rust
let worker_state = state.clone();
let worker_finish_ctx = finish_ctx.clone();
tokio::spawn(async move {
    use futures::FutureExt;
    let outcome = std::panic::AssertUnwindSafe(crate::log_context::scoped(log_ctx, async {
        crate::llm::sub_agent::run_sub_agent(
            &gateway,
            &tool_registry,
            &runtime_deps,
            config,
            &app_settings,
        )
        .await
    }))
    .catch_unwind()
    .await;

    let mut guard = worker_state.lock().await;
    match std::mem::replace(&mut *guard, ForegroundPromotionState::Finished) {
        ForegroundPromotionState::Waiting(tx) => {
            if let Ok(Ok(sub_result)) = &outcome {
                let _ = tx.send(Ok(sub_result.clone()));
            } else {
                let _ = tx.send(outcome_to_anyhow_result(outcome));
            }
        }
        ForegroundPromotionState::Promoted => {
            drop(guard);
            finish_background_subagent(worker_finish_ctx, outcome).await;
        }
        ForegroundPromotionState::Finished => {}
    }
});
```

如果 `SubAgentRunResult` 不能 clone，则把 `ForegroundPromotionState::Waiting` 的 sender 类型设为完整 outcome，并把 outcome 原样 send 给前台等待者。

Foreground select：

```rust
tokio::select! {
    result = done_rx => {
        match result {
            Ok(Ok(sub_result)) => Ok(SpawnForegroundAutoOutcome::Completed(sub_result.envelope.output)),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(anyhow::anyhow!("sub-agent worker finished without result")),
        }
    }
    _ = tokio::time::sleep(std::time::Duration::from_millis(auto_background_after_ms)) => {
        let handle = AsyncTaskHandle {
            agent_id: agent_id.clone(),
            state: AsyncTaskState::Running,
            output_file: transcript_path.clone(),
            description: request.description.clone(),
            cancel_token: cancel_token.clone(),
        };
        if let Some(ref name) = request.name {
            self.task_store.register(name, handle);
        } else {
            self.task_store.register_anonymous(handle);
        }
        let mut guard = state.lock().await;
        *guard = ForegroundPromotionState::Promoted;
        Ok(SpawnForegroundAutoOutcome::Backgrounded {
            agent_id,
            name: request.name.clone(),
            auto_background_after_ms,
        })
    }
    _ = context.cancellation.cancelled() => {
        cancel_token.cancel();
        Err(anyhow::anyhow!("sub-agent cancelled before auto-background promotion"))
    }
}
```

Race rule：如果 worker 在 timer 分支 register 之前完成，前台应返回 completed；如果 timer 先拿到状态锁，则任务必须注册，worker 完成后走后台收尾。

- [ ] **Step 5: 跑 production 行为测试**

Run:

```powershell
cd src-tauri
cargo test --test spawn_subagent_auto_background_test -- --nocapture
```

Expected: PASS。

## 6. Task 3: TaskOutput, TaskStop, Notification 回归

**Files:**
- Modify: `src-tauri/tests/e2e_spawn_subagent_async.rs`
- Modify or Create: `src-tauri/tests/spawn_subagent_auto_background_test.rs`

- [ ] **Step 1: 补 TaskOutput 断言**

测试自动 promotion 后完成的任务：

```rust
let task_output = TaskOutputRuntimeTool::new(Arc::new(TestResolver {
    paths: UserScopedPaths::new(tmp.path(), "t_test__u_test"),
}));
let result = task_output
    .execute(json!({"task_id": task_id, "offset": 0, "task_type": "local_agent"}), ctx)
    .await
    .expect("TaskOutput must read promoted local_agent transcript");
let body: Value = serde_json::from_str(&result.content).expect("json body");
assert!(body["lines"].to_string().contains("long-result"));
assert_eq!(body["task_type"], "local_agent");
```

- [ ] **Step 2: 补 TaskStop 断言**

测试 promotion 后但未完成时，`TaskStop(task_id)` 能 cancel 独立 child token：

```rust
let stop_result = task_stop
    .execute(json!({"task_id": task_id, "task_type": "local_agent"}), ctx)
    .await
    .expect("TaskStop must stop promoted local_agent task");
assert!(stop_result.content.contains("stopped") || stop_result.content.contains("cancel"));
```

- [ ] **Step 3: 补 notification 顺序断言**

等待后台完成后：

```rust
let notifications = queue.drain_for_session(&SessionId::new(TEST_SESSION_ID));
assert_eq!(notifications.len(), 1);
assert!(notifications[0].xml.contains(task_id));
assert!(notifications[0].xml.contains("<status>completed</status>"));
```

再从 store 读状态：

```rust
let handle = store.find_by_id(&AgentId::new(task_id)).expect("handle");
assert_eq!(handle.state, AsyncTaskState::Completed);
```

- [ ] **Step 4: 跑相关 tests**

Run:

```powershell
cd src-tauri
cargo test --test spawn_subagent_auto_background_test -- --nocapture
cargo test --test e2e_spawn_subagent_async -- --nocapture
cargo test --test task_output_tool_test -- --nocapture
```

Expected: PASS。

## 7. Task 4: Tool Catalog and Rust Full Check

**Files:**
- Modify: `src-tauri/src/runtime/tools/catalog.rs`

- [ ] **Step 1: 更新 Agent 工具说明**

在 Agent 描述中加入这段语义：

```text
默认路径：未设置 run_in_background 或设置为 false 时，子 Agent 先以前台方式运行；若超过前台阻塞预算，系统会自动返回 task_id（task_type=local_agent）并让同一个子 Agent 继续后台执行。短任务仍直接返回最终输出。

异步路径：设置 run_in_background=true 时立即返回 agent_id/task_id；子 Agent 从一开始就在后台运行。
```

不要把 `_auto_background_after_ms` 写进用户可见 description。

- [ ] **Step 2: 聚焦测试**

Run:

```powershell
cd src-tauri
cargo test --test spawn_subagent_async_test -- --nocapture
cargo test --test spawn_subagent_auto_background_test -- --nocapture
cargo test --test e2e_spawn_subagent_async -- --nocapture
```

Expected: PASS。

- [ ] **Step 3: 编译检查**

Run:

```powershell
cd src-tauri
cargo check
```

Expected: PASS。

## 8. Task 5: Intent Test Spec

**Files:**
- Modify: `docs/test-intents/spec/tasks/对话/rules.md`

- [ ] **Step 1: 追加新意图**

在文件末尾追加：

```markdown
## 意图-对话-029: 前台长子任务，自动转后台

### 场景

用户要求 agent 委派一个长耗时子任务，但没有要求一开始后台运行。系统应先以前台方式启动子任务；超过前台阻塞预算后，自动把同一个子任务提升为后台任务，让当前轮继续返回 task_id。子任务完成后，TaskOutput 能按这个 task_id 读取输出。

### 操作步骤

1. 应用探活：`tauri-pilot aijia health-check`
2. 开启一条新对话：`tauri-pilot aijia new-task`。
3. 通过 `tauri-pilot aijia where --json` 记录 `{scope}` 和 `{conversationId}`。
4. 发送消息：请委派一个子 Agent 完成一个会超过前台等待预算的子任务，不要主动把 `run_in_background` 设为 true。子 Agent 的任务是先等待超过 20 秒，再返回字符串 `aijia-agent-auto-bg-029`。当系统把子任务自动转后台后，请不要等待子任务自然结束，立刻告诉我 task_id。
5. 等 agent 在 45 秒内回复，并从对话消息文件 `~/.renlijia/users/{scope}/conversations/{conversationId}/messages.jsonl` 记录自动后台任务的 `{taskId}`。
6. 等待 25 秒。
7. 发送消息：请使用 TaskOutput 从 offset 0 读取刚才 `{taskId}` 的输出，并告诉我是否看到了 `aijia-agent-auto-bg-029`。
8. 等 agent 回复。

### 验收标准

应该看到：

- Agent 工具第一次调用参数中没有 `run_in_background == true`
- Agent 工具结果 JSON 中 `assistant_auto_backgrounded == true`
- Agent 工具结果 JSON 中 `status == "async_launched"`
- Agent 工具结果 JSON 中 `task_type == "local_agent"`
- Agent 工具结果 JSON 中 `task_id == "{taskId}"`
- assistant 第一次回复中包含 `{taskId}`
- TaskOutput 工具调用参数中 `task_id == "{taskId}"`
- TaskOutput 工具调用参数中 `offset == 0`
- TaskOutput 工具结果中包含 `aijia-agent-auto-bg-029`
- assistant 第二次回复中包含 `aijia-agent-auto-bg-029`

不应该看到：

- Agent 工具第一次调用参数中不应包含 `run_in_background == true`
- 第一次 assistant 回复不应等到 `aijia-agent-auto-bg-029` 出现后才返回
- TaskOutput 工具结果中不应包含 `No task found`
- `{taskId}` 对应任务不应被记录为 `local_bash`
```

- [ ] **Step 2: 规则校验**

Run:

```powershell
node scripts/run-userwiki-qa-smoke.mjs --validate-only
```

Expected: PASS。该命令不执行意图测试，只确保仓库规则类 fixture 没被破坏。

- [ ] **Step 3: AEIT 目标验收**

如果本地 AIjia dev app 和 tauri-pilot 可用，运行单条意图：

```powershell
pnpm tauri-pilot aijia health-check
pnpm tauri-pilot aijia new-task
pnpm tauri-pilot aijia send "请委派一个子 Agent 完成一个会超过前台等待预算的子任务，不要主动把 run_in_background 设为 true。子 Agent 的任务是先等待超过 20 秒，再返回字符串 aijia-agent-auto-bg-029。当系统把子任务自动转后台后，请不要等待子任务自然结束，立刻告诉我 task_id。"
```

Expected: 45 秒内出现 `assistant_auto_backgrounded == true` 的 Agent tool result 和 `task_type == "local_agent"`。

## 9. Task 6: Review and Cross-Validation

**Files:**
- No direct modifications.

- [ ] **Step 1: 主线程 diff review**

Run:

```powershell
git diff --stat
git diff -- src-tauri/src/runtime/tools/builtin/spawn_subagent.rs src-tauri/src/llm/tool_executor/spawn_subagent.rs
```

Review points:

- 显式 `run_in_background=true` 仍直接走 `launch_async`。
- 普通前台 path 不再无限等待长子 Agent。
- 自动 promotion 后注册 `AsyncTaskType::LocalAgent`。
- transcript 写入、notification enqueue、state terminal update 顺序没有反转。
- 父 run cancel 在 promotion 前会取消 child，不留下孤儿 task。
- promotion 后 TaskStop cancel 的 token 与 store handle 是同一个 token。

- [ ] **Step 2: 子 agent 交叉 review**

派 ReviewWorker 读取本计划和 diff，输出只包含：

```text
STATUS: PASS | FAIL
SPEC GAPS:
- ...
QUALITY RISKS:
- ...
VERIFICATION SEEN:
- ...
```

- [ ] **Step 3: 最终验证命令**

Run:

```powershell
cd src-tauri
cargo test --test spawn_subagent_async_test -- --nocapture
cargo test --test spawn_subagent_auto_background_test -- --nocapture
cargo test --test e2e_spawn_subagent_async -- --nocapture
cargo check
cd ..
node scripts/run-userwiki-qa-smoke.mjs --validate-only
```

Expected: all PASS。

- [ ] **Step 4: 提交**

CodeWorker 和 IntentWorker 的最终产物合并后由主线程提交：

```powershell
git add src-tauri/src/runtime/tools/builtin/spawn_subagent.rs src-tauri/src/llm/tool_executor/spawn_subagent.rs src-tauri/src/runtime/tools/catalog.rs src-tauri/tests/spawn_subagent_async_test.rs src-tauri/tests/spawn_subagent_auto_background_test.rs src-tauri/tests/e2e_spawn_subagent_async.rs docs/test-intents/spec/tasks/对话/rules.md docs/superpowers/plans/2026-06-15-agent-foreground-auto-background.md
git commit -m "feat: auto-background foreground agent tasks"
```

## 10. 完成标准

- Wiki 补充已单独提交，不和 runtime 实现混在同一 commit。
- `Agent` 默认前台调用超过预算后自动返回后台 task id。
- 短 Agent 任务仍以前台同步输出返回。
- 显式 `run_in_background=true` 行为兼容。
- `TaskOutput(task_type=local_agent)` 能读取自动 promotion 后的输出。
- `TaskStop(task_type=local_agent)` 能停止自动 promotion 后的任务。
- 后台完成通知包含同一个 task id，并能进入父对话后续上下文。
- 新增 `意图-对话-029` 覆盖用户真实路径。
- Rust 聚焦测试和 `cargo check` 通过。
