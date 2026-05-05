# Mode B Subagent 实施进度（会话切换交接 v3）

> **当前状态**：2026-05-02
> **branch**: main
> **基线 commit**: `d58b3e8` (Merge branch 'pzc' into main)
> **最新 commit**: `753a4d1` (P8.1a + P8.1b)
> **commit count since baseline**: 26
>
> **大计划**: `docs/superpowers/plans/2026-04-30-lotus-subagent-mode-b-master-plan.md`
> **对标文档**: `docs/superpowers/plans/2026-04-30-subagent-benchmark-vs-claude-code-best.md` (v2)

---

## 里程碑状态

| 里程碑 | 状态 | 说明 |
|---|---|---|
| **M1 — sync subagent 全功能可用** | ✅ DONE | P0–P5 + P9.1/P9.1.5 + P10.1 全部 commit + review pass |
| **M2 — async + notification 全功能** | ⚙️ in progress (≈80%) | P6.x ✅ P7.2 ✅ P8.1a/b ✅ ；P8.1c/d/e + P10.2 待做 |
| **M3 — 测试齐全** | ⏸️ 待启动 | P10.3 + 多个 review-follow-up |

---

## 已完成 commit（26 个，按时间顺序）

| commit | task | 说明 |
|---|---|---|
| `900b26e` | P0.2 | UserScopedPaths::agents_dir() |
| `3c594e8` | P7.1 | TaskNotificationQueue + XML builder |
| `f85ea82` | P0.1 | AgentDefinition 字段扩展 |
| `d7d31b8` | P4.1 | 三层工具白名单 |
| `f821065` | P4.1-fix | mod.rs 注册 tool_whitelist |
| `42ac4c9` | P0.0 | baseline thinking/dingtalk_bridge 字段补齐 |
| `3d058cf` | P1.1 | markdown frontmatter loader |
| `5232fe3` | P1.2 | three-tier registry merge |
| `565c6ee` | P1.3 | wire AgentRegistry into Tauri startup |
| `60b5514` | P2.1 | SubAgentConfig: model_override / agent_name / disallowed_tools |
| `e403dd8` | P1.3-fix | registry_loader infallible |
| `172e245` | P2.2 | effective_settings_for_subagent 透传 model_override |
| `8793691` | P2.3 | SpawnSubagentRuntimeTool + DefaultLauncher (sync only) |
| `c2d9775` | P2.4 | spawn_subagent 加 DAILY_ALLOWED_TOOLS |
| `5dc0ae8` | P4.2 | worker_runtime 接通三层白名单 |
| `13d4e12` | P5.1 | dispatcher 并行 dispatch 验证测试 |
| `c39ea48` | P9.1 | builtin general-purpose / explore agent |
| `9ee2958` | P9.1.5 | explore.model = Inherit + plan §12 校正 |
| `8cb48de` | P10.1 | e2e spawn_subagent 通过 dispatcher (3 测试) |
| `74b75aa` | (旁支) | 8 个 LLM provider 全部加 deprecation 注释 |
| `dba20e1` | P6.1 | AsyncAgentTaskStore (6 测试) |
| `144ce40` | P6.2 | async spawn_subagent + tokio::spawn (5 测试) |
| `1e44160` | P6.2-fix | fail-closed registry + spawn panic catch + propagate err string |
| `5fc6406` | P7.2 | chat_turn_driver 注入 task notification (3 测试) |
| `52da194` | P7.2-fix | drain 改 capture-and-re-enqueue + chat.rs fail-closed 一致 |
| `753a4d1` | P8.1a + P8.1b | output_writer 模块 + task_output RuntimeTool (14 测试) |

---

## 剩余 task（7 片，每片 implementer 拿到即可执行）

> 共同前提：工作目录 `/Users/a20250311/.codex/worktrees/4dc8/lotus-app`，分支 main，禁止开 worktree。
> 每片实施模板：派 implementer → cargo build --tests → cargo test 指定项 → commit（消息见各片）→ 派 opus reviewer → 据 review 决定是否 follow-up。
> 每片末尾的「禁止」是为了防止 implementer 超纲。

### 🟢 立刻可做（无依赖）

#### 片 1 — P8.1c：task_output 工厂注册 + Resolver wiring

**预估**：≤30 行，2 个文件。

**改动 1**：`src-tauri/src/lib.rs`，在第 561 行 `app.manage(current_user_storage.clone());` 后追加一行：
```rust
app.manage(current_user_storage.clone() as Arc<dyn storage::UserScopedPathResolver>);
```
（`schedule_runner` 在 lib.rs:576 已用同样的 cast，模式照抄；不要改其他 manage 行的顺序）

**改动 2**：`src-tauri/src/plugin/registry.rs::try_build_request_scoped_tool` 的 match 里，紧挨 `"spawn_subagent" =>` 分支后加 `"task_output" =>` 分支。完整代码：
```rust
"task_output" => {
    use tauri::Manager;
    let app = match ctx.app_handle.as_ref() {
        Some(a) => a,
        None => {
            log::error!(
                "[task_output registry] no app_handle in PluginContext — refusing to register tool"
            );
            return None;
        }
    };
    let resolver = match app
        .try_state::<Arc<dyn crate::storage::user_scoped_paths::UserScopedPathResolver>>()
    {
        Some(s) => s.inner().clone(),
        None => {
            log::error!(
                "[task_output registry] UserScopedPathResolver not in app state — refusing to register tool"
            );
            return None;
        }
    };
    Some(Arc::new(builtin::task_output::TaskOutputRuntimeTool::new(resolver))
        as Arc<dyn crate::runtime::tools::RuntimeTool>)
}
```

**验证**：
```bash
cd src-tauri && cargo build --tests 2>&1 | tail -10
cd src-tauri && cargo test --lib runtime::tools::builtin::task_output
cd src-tauri && cargo test --lib runtime::agent::output_writer
```
（不引入新测试；只确保 build clean + 已有 14 单测仍 pass）

**Commit**：`feat(tools): wire task_output factory + UserScopedPathResolver dyn-cast (P8.1c)`

**禁止**：不要动 task_output.rs/output_writer.rs/catalog.rs；不要动 lib.rs 其他 manage 行；不要写新测试（片 2 干这个）。

---

#### 片 2 — P8.1d：task_output 集成测试

**预估**：单文件新建，约 100 行。

新建 `src-tauri/tests/task_output_tool_test.rs`。

**Imports**（参考 tests/spawn_subagent_tool_basic_test.rs 顶部）：
```rust
use std::sync::Arc;
use serde_json::{json, Value};
use tempfile::TempDir;

use app_lib::runtime::agent::output_writer::{self, TranscriptLine};
use app_lib::runtime::tools::builtin::task_output::TaskOutputRuntimeTool;
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::RuntimeTool;
use app_lib::storage::user_scoped_paths::{UserScopedPathResolver, UserScopedPaths};
```

**TestResolver**（拷过来，不要 reuse src/）：
```rust
struct TestResolver { paths: UserScopedPaths }
impl UserScopedPathResolver for TestResolver {
    fn resolve_paths(&self) -> Option<UserScopedPaths> { Some(self.paths.clone()) }
}
fn build_tool(tmp: &TempDir) -> TaskOutputRuntimeTool {
    let paths = UserScopedPaths::new(tmp.path(), "t_test__u_test");
    TaskOutputRuntimeTool::new(Arc::new(TestResolver { paths }))
}
```

**4 个测试**（每个 `#[tokio::test]`，ctx = `ToolExecutionContext::for_test("c","r","tc")`）：

1. `returns_empty_for_nonexistent_task` — execute({task_id:"never_existed"}) → JSON.lines.len()=0, new_offset=0
2. `reads_three_lines_with_offset_zero` — `output_writer::append_line` 写 3 条 `assistant("msg-{i}")` 到 `paths.subagent_transcripts_dir().join("agent-x.jsonl")` → execute({task_id:"agent-x", offset:0}) → lines.len()=3, new_offset=3
3. `reads_tail_with_offset` — 同上写 3 条 → execute({offset:2}) → lines.len()=1, new_offset=3, lines[0] 含 "msg-2"
4. `incremental_after_append` — 写 3 条 → execute(offset:0) → 写第 4 条 → execute(offset:3) → lines.len()=1, new_offset=4, lines[0] 含 "msg-3"

**验证**：
```bash
cd src-tauri && cargo test --test task_output_tool_test
```
预期 4/4 pass。

**Commit**：`test(tools): integration tests for task_output tool (P8.1d)`

**禁止**：不动 task_output.rs；不走 dispatcher；不 mock UserScopedPathResolver 的其他 trait 方法。

---

#### 片 3 — P8.1e：spawn_subagent::launch_async 接 output_writer

**预估**：1 主文件 + 1 wiring 文件，约 40 行净增。

**步骤 1**：`src-tauri/src/llm/tool_executor/spawn_subagent.rs` 顶部加 use：
```rust
use crate::runtime::agent::output_writer;
use crate::storage::user_scoped_paths::UserScopedPathResolver;
```

**步骤 2**：`DefaultSpawnSubagentLauncher` 加字段：
```rust
paths: Arc<dyn UserScopedPathResolver>,
```

**步骤 3**：更新 `from_runtime_deps`，在末尾加 `paths: Arc<dyn UserScopedPathResolver>` 参数，struct literal 里赋值。

**步骤 4**：更新唯一调用站点 `src-tauri/src/plugin/registry.rs` 的 `"spawn_subagent" =>` 分支（commit `1e44160` 改过的那段）— 在 `notif_queue` lookup 后再加一段 resolver lookup（与片 1 工厂分支模式相同），然后传给 `DefaultSpawnSubagentLauncher::from_runtime_deps(...)` 末尾参数。

**步骤 5**：`launch_async` 顶部（在 `let agent_id = ...` 之后）：
```rust
let transcript_path = match self.paths.require_paths() {
    Ok(p) => output_writer::transcript_path(
        &p.subagent_transcripts_dir(),
        agent_id.as_str(),
    ),
    Err(e) => {
        log::warn!("[spawn_subagent async] no user scope: {}; transcript disabled", e);
        std::path::PathBuf::new()  // 兜底：保留旧行为
    }
};
let transcript_path_for_task = transcript_path.clone();
```
（注意：launch_async 在 tokio::spawn 内，**不要** fail-closed —— 退化到无 transcript 是合理 best-effort）

**步骤 6**：`AsyncTaskHandle::output_file: PathBuf::new()` 改为 `transcript_path.clone()`。

**步骤 7**：`tokio::spawn` body 三个分支末尾（在各自的 `notif_queue.enqueue` 之前）：
```rust
// Ok 分支
let _ = output_writer::append_line(
    &transcript_path_for_task,
    &output_writer::TranscriptLine::assistant(output_ref),
);
// Err 分支
let _ = output_writer::append_line(
    &transcript_path_for_task,
    &output_writer::TranscriptLine::failed(&err_str),
);
// Panic 分支
let _ = output_writer::append_line(
    &transcript_path_for_task,
    &output_writer::TranscriptLine::failed(&panic_summary),
);
```

**步骤 8**：`build_task_notification_xml` 第 3 个参数从 `""` 改为路径字符串。注意临时变量生命周期：
```rust
let p_str = transcript_path_for_task.to_string_lossy();
let xml = build_task_notification_xml(
    id_for_task.as_str(),
    Some(&parent_tool_use_id),
    &p_str,  // ← 这里
    "completed",
    ...
);
```
三个分支都改。

**步骤 9**：`grep -rn "from_runtime_deps" src-tauri/src src-tauri/tests` 确认是否只有 plugin/registry.rs 一处构造 `DefaultSpawnSubagentLauncher::from_runtime_deps`。如果有 test 构造，更新调用站点（多传一个 Arc resolver；test 用 `TestResolver` 模式，参考片 2）。

**验证**：
```bash
cd src-tauri && cargo build --tests 2>&1 | tail -10
cd src-tauri && cargo test --test spawn_subagent_async_test --test spawn_subagent_tool_basic_test --test spawn_subagent_parallel_dispatch_test --test e2e_spawn_subagent_explore --test agent_registry_merge_test --test task_notification_injection_test --test task_output_tool_test
```
全 pass。

**Commit**：`feat(agent): wire launch_async transcript writer + populate AsyncTaskHandle.output_file (P8.1e)`

**禁止**：不改 task_output.rs/output_writer.rs/catalog.rs；不写新测试（片 4 干这个）；不动 launch_sync 路径。

---

> 三片**强制顺序 1 → 2 → 3**：片 1 完成后 cargo build 必绿；片 2 才能用 task_output；片 3 改 from_runtime_deps 签名前必须确认片 1 的 wiring 范式（resolver dyn-cast）已在主分支。

### 🟡 中等依赖

#### 片 4 — P10.2：async e2e 测试

**前置**：片 1 + 片 3 已 commit。

**预估**：单文件新建，约 200 行。

新建 `src-tauri/tests/e2e_spawn_subagent_async.rs`。

**Imports**（参考 tests/e2e_spawn_subagent_explore.rs）：
```rust
use std::sync::Arc;
use serde_json::{json, Value};
use async_trait::async_trait;

use app_lib::runtime::agent::async_task_store::{
    AsyncAgentTaskStore, AsyncTaskHandle, AsyncTaskState,
};
use app_lib::runtime::agent::registry::AgentRegistry;
use app_lib::runtime::agent::task_notification::TaskNotificationQueue;
use app_lib::runtime::ids::AgentId;
use app_lib::runtime::tools::builtin::spawn_subagent::{
    SpawnAsyncOutcome, SpawnSubagentContext, SpawnSubagentLauncher, SpawnSubagentRequest,
    SpawnSubagentRuntimeTool,
};
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::RuntimeTool;
```

**StubLauncher**（同步完成模拟，不真 tokio::spawn）：
```rust
struct StubLauncher {
    task_store: Arc<AsyncAgentTaskStore>,
    notif_queue: Arc<TaskNotificationQueue>,
}
#[async_trait]
impl SpawnSubagentLauncher for StubLauncher {
    async fn launch_sync(&self, _req: SpawnSubagentRequest, _ctx: SpawnSubagentContext)
        -> anyhow::Result<String> { unreachable!("test only exercises async") }
    async fn launch_async(&self, req: SpawnSubagentRequest, _ctx: SpawnSubagentContext)
        -> anyhow::Result<SpawnAsyncOutcome> {
        let agent_id = AgentId::new(format!("stub-{}", uuid::Uuid::new_v4()));
        if let Some(name) = &req.name {
            self.task_store.register(name, AsyncTaskHandle {
                agent_id: agent_id.clone(),
                state: AsyncTaskState::Running,
                output_file: std::path::PathBuf::new(),
                description: req.description.clone(),
            });
        }
        // 同步模拟完成（避免 tokio::spawn 时序波动）
        self.task_store.update_state(&agent_id, AsyncTaskState::Completed);
        let xml = format!("<task-notification><task-id>{}</task-id></task-notification>", agent_id.as_str());
        self.notif_queue.enqueue(agent_id.as_str(), xml);
        Ok(SpawnAsyncOutcome { agent_id, name: req.name.clone() })
    }
}
fn build_tool() -> (SpawnSubagentRuntimeTool, Arc<AsyncAgentTaskStore>, Arc<TaskNotificationQueue>) {
    let task_store = Arc::new(AsyncAgentTaskStore::new());
    let notif_queue = Arc::new(TaskNotificationQueue::new());
    let registry = Arc::new(AgentRegistry::with_builtins());
    let launcher = Arc::new(StubLauncher {
        task_store: task_store.clone(),
        notif_queue: notif_queue.clone(),
    });
    let tool = SpawnSubagentRuntimeTool::new(launcher, registry);
    (tool, task_store, notif_queue)
}
```

**5 个测试**（每个 `#[tokio::test]`）：

1. `async_path_returns_immediately_with_agent_id` — execute({subagent_type:"explore", prompt:"x", description:"x", run_in_background:true, name:"w1"}) → 解析 result.content JSON → assert status="async_launched", agent_id 非空, name="w1"
2. `async_path_registers_in_task_store` — 同上 → `task_store.find_by_name("w1").is_some()`，state == Completed
3. `async_path_enqueues_notification` — 同上 → `notif_queue.drain_all().len() == 1`，xml 含 agent_id
4. `async_path_without_name_skips_register` — execute(无 name 字段) → `task_store.list_active().is_empty()` 且 `task_store.find_by_name("anything").is_none()`，但 notif 还是发了 1 条
5. `async_path_state_is_completed_after_stub_finishes` — execute → `find_by_id(agent_id)` state == Completed

**验证**：
```bash
cd src-tauri && cargo test --test e2e_spawn_subagent_async
```
5/5 pass。

**Commit**：`test(agent): e2e async spawn_subagent + task_store + notif_queue (P10.2)`

**禁止**：不真跑 LLM；不依赖 tokio::spawn 实际跑（stub 同步完成）；不读 transcript 文件（片 3 的责任）。

---

### 🔴 收尾

#### 片 5 — P10.1 follow-up：Fixed-model + 负向测试

**预估**：改 `src-tauri/tests/spawn_subagent_tool_basic_test.rs` 既存文件，加 2 个测试 + 1 helper，约 60 行。

**辅助函数**（加在 module 顶部，已有 imports 之后）：
```rust
fn registry_with_fixed_model_agent() -> Arc<AgentRegistry> {
    use app_lib::runtime::agent::definition::{
        AgentDefinition, AgentModel, AgentPermissionMode, AgentPrompt, AgentSource,
    };
    let mut reg = AgentRegistry::with_builtins();
    reg.register(AgentDefinition {
        name: "fixed-model-test-agent".into(),
        description: "test".into(),
        allowed_tools: vec![],
        disallowed_tools: vec![],
        max_iterations: 5,
        model: AgentModel::Fixed("test-fixed-model-id".into()),
        system_prompt: AgentPrompt::Inline("test".into()),
        source: AgentSource::Builtin,
        permission_mode: AgentPermissionMode::Bubble,
        background_default: false,
    });
    Arc::new(reg)
}
```
（先 grep 确认 `AgentDefinition` 字段以现行 commit 为准；如有差异按现状改）

**测试 1**：`fixed_model_definition_resolves_when_caller_omits_model`
- 用 `registry_with_fixed_model_agent()` + 已有 RecordingLauncher pattern 构造 tool
- execute(`{subagent_type:"fixed-model-test-agent", prompt:"x", description:"x"}`)（**不**传 model）
- assert recorded_request.effective_model == Some("test-fixed-model-id".to_string())

**测试 2**：`caller_model_overrides_fixed_definition`
- 同上 registry
- execute(`{subagent_type:"fixed-model-test-agent", prompt:"x", description:"x", model:"caller-id"}`)
- assert recorded_request.effective_model == Some("caller-id".to_string())

**测试 3 不必新增** — 现有 `unknown_subagent_type_returns_helpful_error` 已覆盖负向；只需跑 `cargo test --test spawn_subagent_tool_basic_test` 确认所有原有 11 测试 + 新增 2 测试都过。

**验证**：
```bash
cd src-tauri && cargo test --test spawn_subagent_tool_basic_test
```
13 测试 pass。

**Commit**：`test(tools): cover AgentModel::Fixed branch in spawn_subagent (P10.1 follow-up)`

**禁止**：不改 spawn_subagent.rs 生产代码；不改 RecordingLauncher。

---

#### 片 6 — P10.3：review_ 架构约束回归

**预估**：单文件新建，约 80 行。

新建 `src-tauri/tests/review_agent_b_constraints.rs`。

**测试 1**：`agent_modules_do_not_use_tauri_directly`
```rust
#[test]
fn agent_modules_do_not_use_tauri_directly() {
    for path in &[
        "src/runtime/agent/markdown_loader.rs",
        "src/runtime/agent/registry.rs",
        "src/runtime/agent/registry_loader.rs",
        "src/runtime/agent/async_task_store.rs",
        "src/runtime/agent/task_notification.rs",
        "src/runtime/agent/output_writer.rs",
        "src/runtime/agent/tool_whitelist.rs",
    ] {
        let src = std::fs::read_to_string(path)
            .unwrap_or_else(|_| panic!("missing {}", path));
        assert!(
            !src.contains("use tauri::") && !src.contains("tauri::Manager"),
            "{} must not import tauri::* (runtime layer purity)",
            path
        );
    }
}
```

**测试 2**：`spawn_subagent_tool_is_concurrency_safe`
```rust
use async_trait::async_trait;
use std::sync::Arc;
use app_lib::runtime::agent::registry::AgentRegistry;
use app_lib::runtime::tools::builtin::spawn_subagent::{
    SpawnAsyncOutcome, SpawnSubagentContext, SpawnSubagentLauncher, SpawnSubagentRequest,
    SpawnSubagentRuntimeTool,
};
use app_lib::runtime::tools::RuntimeTool;

struct NoopLauncher;
#[async_trait]
impl SpawnSubagentLauncher for NoopLauncher {
    async fn launch_sync(&self, _: SpawnSubagentRequest, _: SpawnSubagentContext) -> anyhow::Result<String> { unreachable!() }
    async fn launch_async(&self, _: SpawnSubagentRequest, _: SpawnSubagentContext) -> anyhow::Result<SpawnAsyncOutcome> { unreachable!() }
}

#[test]
fn spawn_subagent_tool_is_concurrency_safe() {
    let tool = SpawnSubagentRuntimeTool::new(
        Arc::new(NoopLauncher),
        Arc::new(AgentRegistry::with_builtins()),
    );
    assert!(tool.is_concurrency_safe(&serde_json::Value::Null));
}
```

**测试 3**：`async_agent_default_disallows_ask_user_question`
- 先 `grep -n "fn resolve_agent_tools" src-tauri/src/runtime/agent/tool_whitelist.rs` 确认 6 参数签名顺序（与 P4.1 commit `d7d31b8` 一致）
```rust
#[test]
fn async_agent_default_disallows_ask_user_question() {
    use app_lib::runtime::agent::tool_whitelist::resolve_agent_tools;
    let allowed = resolve_agent_tools(
        &[],  // def_allowed = empty (= all)
        &[],  // def_disallowed
        &["ask_user_question".to_string(), "read_file".to_string()],  // available
        true,  // is_async
        false, // allow_recursive_spawn
    );
    assert!(
        !allowed.contains(&"ask_user_question".to_string()),
        "async agents must never get ask_user_question"
    );
}
```

**验证**：
```bash
cd src-tauri && cargo test --test review_agent_b_constraints
cd src-tauri && cargo test review_ --tests --no-fail-fast
```
3 个新测试 pass，所有既有 review_ 测试不破坏。

**Commit**：`test(agent): review_ regression for Mode B architectural constraints (P10.3)`

**禁止**：不改生产代码；不动其他 review_ 测试。

---

#### 片 7 — 收尾 follow-up #43：browse_data SECURITY 注释

**预估**：1 文件 1 处注释，约 10 行。

`src-tauri/src/llm/tool_executor/internal_system.rs:368` 附近。先：
```bash
grep -n "allowed_tools\s*[:=]\s*vec!\[" src-tauri/src/llm/tool_executor/internal_system.rs
```
找到 browse_data 子 agent 的硬编码 allowed_tools 定义点（commit `5dc0ae8` 删 run_sub_agent 守卫后由这个列表承担递归保护）。

在该 `let allowed_tools: Vec<String> = vec![ ... ];` 上方加注释：
```rust
// SECURITY: This list MUST NOT contain "browse_data". browse_data delegates
// to a sub-agent which would otherwise recursively spawn another browse_data
// sub-agent on every nested data extraction request — infinite recursion +
// LLM cost explosion. The historic guard in run_sub_agent that rejected
// browse_data in allowed_tools was deleted in commit 5dc0ae8 (P4.2) when
// worker_runtime took over whitelist enforcement. The recursive protection
// now relies entirely on this hardcoded list. If this list is ever changed
// to be dynamic, browse_data MUST be added to ALL_AGENT_DISALLOWED in
// runtime/agent/tool_whitelist.rs to restore the guard.
```

**验证**：
```bash
cd src-tauri && cargo build --tests 2>&1 | tail -3
```
build clean（仅注释）。

**Commit**：`docs(llm): SECURITY note on browse_data sub-agent allowed_tools (#43 follow-up)`

**禁止**：不改任何代码；只加注释。

---

## 已跳过/决议

- **P9.2 browse_data 兼容包装**：用户决议跳过，browse_data 维持 legacy ToolPlugin 路径
- **#34 P2.2-fix** model_override 在 cloud/custom 路由模式下失效：被 P9.1.5 + memory note + P-router-model-passthrough 取代
- **#39 P2.3-fix**：(1) silent fallback ✅ 已 fail-closed (P6.2-fix) (2) async placeholder echo parent_tool_use_id ✅ 已通过 P6.2 SpawnAsyncOutcome 解决

---

## 长期专项（不在 Mode B 范围）

- **P-router-model-passthrough**：8 个 LLM provider 收敛为单一 OpenAI-兼容实现 + endpoint/认证。8 个文件已加 deprecation 注释（commit `74b75aa`）。详见 memory `project_lotus_llm_routing.md`。

---

## 执行参数（已约定）

| 维度 | 设置 |
|---|---|
| 工作目录 | `/Users/a20250311/.codex/worktrees/4dc8/lotus-app` |
| 分支 | main，禁止开 worktree |
| Implementer 模型 | 按复杂度选 haiku/sonnet（机械活 haiku，多文件 TDD sonnet） |
| Reviewer 模型 | **opus**（用户明确要求） |
| 并行规则 | 串行（避免 git reset 互相污染） |
| Review 7 维度 | 每次 review 强制：直接 / 横向 / 纵向 / 时间 / 失败 / 安全 / 计划对齐 |

---

## 重要架构事实（已验证，记账）

1. **LlmGateway 是 stateless w.r.t. model**：model 走 `AppSettings.primary_model`（实际是 endpoint key）和 `cloud_model`/`custom_model_name`（实际 model id）
2. **AgentRegistry 注入位置**：`lib.rs::app.manage(Arc<AgentRegistry>)` setup 阶段一次注册，不按用户切换刷新
3. **runtime/ 层纯度**：`runtime/tools/builtin/spawn_subagent.rs` 不导 LlmGateway/SubAgentConfig/tauri::*；DefaultSpawnSubagentLauncher 在 `llm/tool_executor/spawn_subagent.rs`
4. **三层工具白名单**（resolve_agent_tools）：def_allowed → ALL_AGENT_DISALLOWED 过滤 → ASYNC_AGENT_ALLOWED 子集（async only）→ 删 spawn_subagent（除非 allow_recursive_spawn=true）
5. **browse_data 递归保护**：旧 run_sub_agent 守卫已删；新保护 = `internal_system.rs:368` 硬编码 allowed_tools 列表（不含 browse_data）。**这是隐式安全边界** — 待片 7 加 SECURITY 注释
6. **lotus LLM 路由**：所有 provider 走 OpenAI 协议；只有 lotus / custom 透传 model id；其余 6 个写死 DEFAULT_MODEL（claude.rs 是 Anthropic Messages API 异类）。详见 memory `project_lotus_llm_routing.md`
7. **AsyncAgentTaskStore lifecycle**：`update_state(Completed/Failed/Killed)` **不删除 entry**，parent 完成后还能 task_output 查询（P6.1 设计 + P8.1 依赖）
8. **TaskNotificationQueue drain 语义**：drain 是 capture-and-re-enqueue（P7.2-fix），失败/cancel/Err 路径会把 drained 列表重新入队避免永久丢失（10 个失败分支已 trace）
9. **fail-closed 一致**：plugin/registry.rs 和 transport/tauri_commands/chat.rs 缺 app state 时都 `log::error!` + 不注册/不接 queue（不 panic，不静默 fresh-instance）

---

## 测试覆盖（截至 commit `753a4d1`）

| 模块 | 测试数 |
|---|---|
| `runtime::agent` lib 单测 | 28 (含 output_writer 7) |
| `runtime::tools::builtin::task_output` 单测 | 7 |
| `tests/agent_*` 集成测试 | 12 |
| `tests/spawn_subagent_*` | 24 |
| `tests/task_notification_injection_test` | 4 |
| `tests/e2e_spawn_subagent_explore` | 3 |
| `tests/worker_runtime_*` | 4 |
| `tests/review_*`（部分） | 已修过 |
| **合计** | **80+ 单测/集成测试，全部 PASS** |

预先存在的 `storage::file_store::messages` 5 个失败与 Mode B 无关，独立 ticket 跟踪。
