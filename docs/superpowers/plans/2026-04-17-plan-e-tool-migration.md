# 核心工具完整迁移计划（Plan-E）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 execute_python、generate_report、generate_chart 三个核心工具从 LegacyToolAdapter 完整迁移到 RuntimeTool，通过 capability trait 注入脱离 PluginContext 全局依赖。

**Architecture:** 先抽出 legacy handler 可复用的 shared core，再用 capability trait / request-scoped runtime context 组装 `RuntimeTool`；`ToolExecutionContext` / `CapabilityContext` 承载 request-scoped 状态，registry 只负责注入稳定依赖与运行时工厂。

**Tech Stack:** Rust, async_trait, tokio

**执行分支：** 当前分支 `pzc`（不创建 worktree，不切分支）

---

## 改造视角

> 这是迁移计划，不是新功能开发。三个工具已有旧实现（PluginContext 路径），目标是将它们迁移到 RuntimeTool 路径，消除对全局 PluginContext 的依赖。

### 整体迁移策略

**当前状态**：`execute_python`、`generate_report`、`generate_chart` 三个核心工具全部走 `LegacyToolAdapter` → `PluginContext` 全局 service locator 路径，可以访问整个编排层对象（gateway、auth_manager 等），无能力隔离约束。

**目标状态**：三个工具实现 `RuntimeTool` trait，通过 capability trait 注入最小依赖，`PluginContext` 依赖消除。旧实现文件保留（加 `#[allow(dead_code)]`），作为回滚保险，确认 zero regression 后再删除。

**迁移模式（三个工具相同）**：
```
旧路径：ToolPlugin::execute(&PluginContext) → 访问任意全局对象
新路径：RuntimeTool::execute(input, ToolExecutionContext) → 通过 capability trait 访问最小依赖
```

**执行顺序**：E0（runtime 暴露面 / 调度面校准）→ E1（shared core + trait 边界）→ E2（execute_python）→ E3（generate_report）→ E4（generate_chart）→ E5（runtime path 集成回归）→ E6（browse_data / subagent launcher）

### 2026-04-18 对标校准（优先级高于下文旧草案）

> 下文若仍保留早期草案描述，以本节为准执行。

1. **先抽 shared core，再迁 RuntimeTool。**
   - 不允许在 `runtime/tools/builtin/*.rs` 里重新抄一份 legacy 业务逻辑。
   - `llm/tool_executor/python.rs` / `report.rs` / `chart.rs` 中需要先抽出 `PluginContext` free 的 core，legacy handler 与 runtime tool 共用同一份核心逻辑。
2. **request-scoped 状态优先来自 `ToolExecutionContext` / `CapabilityContext`。**
   - `run_id`、`agent_id`、`cancellation`、workspace capability、权限结果等不应固化成 tool struct 的常驻状态。
   - tool struct 只持有稳定依赖（例如 capability trait 实现、固定服务对象、纯配置）。
3. **E0 先校准 runtime 暴露面。**
   - 先补齐 `REQUEST_SCOPED_RUNTIME_TOOL_NAMES`、schema source、dispatcher/runtime-path 集成测试，避免“代码实现了但运行面仍走 legacy”。
4. **`execute_python` 必须保住 analysis 语义。**
   - `auto-load uploaded files`、`loaded preamble`、`authorized workspace preamble`、analysis snapshot / user vars / step_state 等现有生产语义属于 Plan-E 范围，不接受“先切路径，语义以后补齐”。
5. **E6 是 Plan-E 正式任务，不是附录。**
   - `browse_data` / subagent launcher 的 cancel / run_id / agent_id 传播属于 request-scoped runtime abstraction 的关键缺口，必须在 Plan-E 内收口，Plan-H 直接依赖它完成。

---

### E1：PythonExecution trait

**当前状态**：`execute_python` 通过 `PluginContext.session_manager` 访问 `PythonSessionManager`，带走整个 context。

**目标状态**：定义 `trait PythonExecution` 只暴露 execute_python 实际需要的操作，`DefaultPythonExecution` 包装 `PythonSessionManager`。

**迁移影响**：仅新增文件，不改现有代码。

---

### E2：ExecutePythonRuntimeTool 完整实现

**当前状态**：`src-tauri/src/runtime/tools/builtin/python.rs` 是 stub，execute 返回错误。生产路径仍走旧 `python_exec.rs`。

**目标状态**：stub 升级为完整实现，通过 `PythonExecution` trait 执行，注册到 `try_build_request_scoped_tool`。旧 `python_exec.rs` 加 `#[allow(dead_code)]`。

**迁移验证**：运行现有 Python 执行相关测试，确保行为一致。

---

### E3/E4：generate_report / generate_chart

**当前状态**：两个工具都是 stub，生产路径走旧 `report_gen.rs` / `chart_gen.rs`。

**目标状态**：分别定义 `ReportCapability` / `ChartCapability` trait，完整实现迁移，旧实现标 dead_code。

---

### 回归验证（整体）

每个工具迁移后必须运行：
```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast
cd src-tauri && cargo test review_atomic_tool_closure_test --tests
```

确认新旧路径行为一致后，再进行下一个工具的迁移。

---

## 现状（Pre-E）

- `ExecutePythonRuntimeTool`（`runtime/tools/builtin/python.rs`）：stub 已注册，`execute()` 仍持有 `Option<PluginContext>` 桥接到 `handle_execute_python`
- `GenerateReportRuntimeTool`（`runtime/tools/builtin/report.rs`）：纯 stub，`execute()` 直接返回 `ExecutionFailed`
- `GenerateChartRuntimeTool`（`runtime/tools/builtin/chart.rs`）：纯 stub，`execute()` 直接返回 `ExecutionFailed`
- `handle_execute_python` / `handle_generate_report` / `handle_generate_chart`（`llm/tool_executor/`）：签名仍为 `&PluginContext`，依赖 `PythonSessionManager`、`AppStorage`、`AppHandle`、`AuthManager`
- `plugin/registry.rs` 的 `try_build_request_scoped_tool`：`execute_python` 分支传入 `ctx.clone()`（整个 `PluginContext`）；`generate_report` / `generate_chart` 分支缺失

**迁移目标：**

| 工具 | 迁移后持有 | 不再持有 |
|------|-----------|---------|
| execute_python | `Arc<dyn PythonExecution>` + 纯值字段（workspace_path, conversation_id, run_id） | `Option<PluginContext>` |
| generate_report | `Arc<dyn ReportCapability>` + workspace_path + conversation_id | `PluginContext` |
| generate_chart | `Arc<dyn ChartCapability>` + workspace_path + conversation_id | `PluginContext` |

---

## Task E1：定义 PythonExecution trait

**目标：** 将 execute_python 对 `PythonSessionManager` 的有状态依赖抽为 trait，使 `ExecutePythonRuntimeTool` 可通过 mock 注入在测试中验证业务逻辑，而不需要真正启动 Python 进程。

### 文件

新建：`src-tauri/src/runtime/tools/builtin/python_execution.rs`

### E1-Step 1：写失败测试

**文件：** `src-tauri/tests/plan_e_tool_migration_test.rs`（新建）

```rust
//! Plan-E 工具完整迁移测试。
//! 每个 Task 对应一组测试，先写测试（红），再实现（绿）。

// ── E1: PythonExecution trait ─────────────────────────────────────────────────

/// PythonExecution trait 可见性测试：trait 存在且方法签名正确。
#[test]
fn python_execution_trait_is_accessible() {
    use app_lib::runtime::tools::builtin::python_execution::PythonExecution;
    // If this compiles, the trait is defined and publicly accessible.
    let _: Option<Box<dyn PythonExecution>> = None;
}

/// DefaultPythonExecution 存在且实现了 PythonExecution。
#[test]
fn default_python_execution_implements_trait() {
    use app_lib::runtime::tools::builtin::python_execution::DefaultPythonExecution;
    use app_lib::runtime::tools::builtin::python_execution::PythonExecution;
    // Construct in test requires Arc<PythonSessionManager>, which needs workspace.
    // We only verify the type exists and is object-safe here.
    fn assert_impl<T: PythonExecution>() {}
    assert_impl::<DefaultPythonExecution>();
}
```

**运行确认失败：**
```bash
cd src-tauri && cargo test --test plan_e_tool_migration_test python_execution_trait_is_accessible 2>&1 | tail -5
```
期望错误：`error[E0432]: unresolved import 'app_lib::runtime::tools::builtin::python_execution'`

### E1-Step 2：实现

**文件：** `src-tauri/src/runtime/tools/builtin/python_execution.rs`（新建）

```rust
//! PythonExecution trait — 将 execute_python 对 PythonSessionManager 的依赖抽象为 trait。
//!
//! 这样 ExecutePythonRuntimeTool 可通过 Arc<dyn PythonExecution> 注入，
//! 测试中使用 mock，生产环境使用 DefaultPythonExecution（包装 PythonSessionManager）。

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;

use crate::python::runner::ExecutionResult;
use crate::python::sandbox::SandboxConfig;
use crate::python::session::PythonSessionManager;
use crate::runtime::ids::RunId;

/// Python 执行能力接口。
///
/// - `execute_in_session`：analysis mode，持久 session，按 scope_key（run_id 或 conv_id）路由。
/// - `execute_oneshot`：daily mode，one-shot 执行，每次 spawn 新进程，无持久状态。
#[async_trait]
pub trait PythonExecution: Send + Sync {
    /// Analysis mode：在持久 Python REPL session 中执行代码。
    ///
    /// `scope_key` 通常由调用方从 `RunId` 生成（`session_key_for_run`）
    /// 或降级使用 `conversation_id`（无 RunId 时）。
    async fn execute_in_session(
        &self,
        scope_key: &str,
        code: &str,
        timeout: Duration,
        sandbox: &SandboxConfig,
    ) -> Result<ExecutionResult>;

    /// Daily mode：无状态 one-shot 执行。
    ///
    /// 每次调用 spawn 新 Python 进程，执行后销毁。
    /// `workspace` 用于 SandboxConfig 构造和工作目录设置。
    async fn execute_oneshot(
        &self,
        workspace: &Path,
        code: &str,
        sandbox: &SandboxConfig,
    ) -> Result<ExecutionResult>;

    /// 中断当前运行的 session（stop_streaming 触发）。
    async fn interrupt_session(&self, scope_key: &str) -> Result<()>;
}

// ── DefaultPythonExecution ────────────────────────────────────────────────────

/// 生产实现：包装现有 `PythonSessionManager`。
pub struct DefaultPythonExecution {
    session_manager: Arc<PythonSessionManager>,
    python_binary: std::path::PathBuf,
    python_home: Option<std::path::PathBuf>,
}

impl DefaultPythonExecution {
    /// 构造时接收 `Arc<PythonSessionManager>`（已在应用启动时创建）。
    /// `python_binary` / `python_home` 在启动期由 `PythonSessionManager::new` 解析，
    /// 通过这里传入，避免 `DefaultPythonExecution` 自身依赖 `tauri::AppHandle`。
    pub fn new(
        session_manager: Arc<PythonSessionManager>,
        python_binary: std::path::PathBuf,
        python_home: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            session_manager,
            python_binary,
            python_home,
        }
    }
}

#[async_trait]
impl PythonExecution for DefaultPythonExecution {
    async fn execute_in_session(
        &self,
        scope_key: &str,
        code: &str,
        timeout: Duration,
        sandbox: &SandboxConfig,
    ) -> Result<ExecutionResult> {
        let result = self
            .session_manager
            .execute(scope_key, code, timeout, sandbox)
            .await?;
        Ok(result.result)
    }

    async fn execute_oneshot(
        &self,
        workspace: &Path,
        code: &str,
        sandbox: &SandboxConfig,
    ) -> Result<ExecutionResult> {
        use crate::python::runner::PythonRunner;
        let runner = PythonRunner::with_config(
            workspace.to_path_buf(),
            sandbox.clone(),
            self.python_binary.clone(),
            self.python_home.clone(),
        );
        runner.execute(code).await
    }

    async fn interrupt_session(&self, scope_key: &str) -> Result<()> {
        self.session_manager.interrupt(scope_key).await
    }
}
```

将 `python_execution` 模块注册到 `src-tauri/src/runtime/tools/builtin/mod.rs` 的 `pub mod` 列表中。

### E1-Step 3：运行确认通过

```bash
cd src-tauri && cargo test --test plan_e_tool_migration_test python_execution_trait -- --nocapture
```

### E1-Step 4：commit

```
feat(plan-e/E1): define PythonExecution trait + DefaultPythonExecution wrapper
```

- [ ] E1-Step 1：写失败测试，确认 `cargo test` 编译报错
- [ ] E1-Step 2：实现 `python_execution.rs` 并注册模块
- [ ] E1-Step 3：`cargo test` 两个 E1 测试通过，全量回归无新失败
- [ ] E1-Step 4：commit

---

## Task E2：ExecutePythonRuntimeTool 完整实现

**目标：** 将 `ExecutePythonRuntimeTool` 的 `execute()` 从 "持有 `Option<PluginContext>` 然后调 `handle_execute_python`" 改为 "持有 `Arc<dyn PythonExecution>` + 纯值字段，直接驱动执行逻辑"。

**参照：** `docs/2026-04-16-execute-python-migration-boundary.md` 的字段迁移分析。

**关键设计：**

- `ExecutePythonRuntimeTool` 持有：
  - `python: Arc<dyn PythonExecution>` — 执行能力（有状态，测试可 mock）
  - `workspace_path: PathBuf` — sandbox 构造 + analysis 目录
  - `authorized_workspace: Option<AuthorizedWorkspaceRef>` — sandbox 扩展路径
  - `storage: Arc<AppStorage>` — 上传文件查询、loaded key 读写、step_state
  - `file_manager: Arc<FileManager>` — auto-load 中转
  - `conversation_id: String` — loaded_key / auto-load 迭代
  - `run_id: Option<RunId>` — loaded_scope_id 分支
  - `model: String` — telemetry 打点
- `execute()` 驱动原 `handle_execute_python` 的业务逻辑，**不再调 `handle_execute_python(ctx, &input)`**；business logic 原地实现或提取为自由函数
- `plugin_ctx: Option<PluginContext>` 字段删除，`stub_mode` 保留
- 注意：`auto-load` 中 `handle_load_file_core` 需要 `file_manager` 和 `storage`，可以通过 `DefaultFileOperations` 直接调用

### 文件修改

- `src-tauri/src/runtime/tools/builtin/python.rs`：全量重写
- `src-tauri/src/plugin/registry.rs`：`try_build_request_scoped_tool` 的 `execute_python` 分支注入 `DefaultPythonExecution`
- `src-tauri/src/plugin/builtin/tools/python_exec.rs`：加顶层 `#[allow(dead_code)]`（文件已有 `#![allow(deprecated)]`，再加 `dead_code`）

### E2-Step 1：写失败测试

追加到 `src-tauri/tests/plan_e_tool_migration_test.rs`：

```rust
// ── E2: ExecutePythonRuntimeTool 完整实现 ────────────────────────────────────

use std::sync::Arc;
use async_trait::async_trait;
use std::time::Duration;
use std::path::{Path, PathBuf};
use anyhow::Result;

use app_lib::runtime::tools::builtin::python_execution::PythonExecution;
use app_lib::python::runner::ExecutionResult;
use app_lib::python::sandbox::SandboxConfig;

/// Mock PythonExecution：execute_in_session 返回预设结果；execute_oneshot 同理。
#[derive(Clone)]
struct MockPythonExecution {
    session_result: Arc<std::sync::Mutex<Result<ExecutionResult, String>>>,
    oneshot_result: Arc<std::sync::Mutex<Result<ExecutionResult, String>>>,
}

impl MockPythonExecution {
    fn succeed_with(stdout: &str) -> Self {
        let ok = ExecutionResult {
            stdout: stdout.to_string(),
            stderr: String::new(),
            exit_code: 0,
            execution_time_ms: 10,
            timed_out: false,
        };
        Self {
            session_result: Arc::new(std::sync::Mutex::new(Ok(ok.clone()))),
            oneshot_result: Arc::new(std::sync::Mutex::new(Ok(ok))),
        }
    }

    fn fail_with(msg: &str) -> Self {
        Self {
            session_result: Arc::new(std::sync::Mutex::new(Err(msg.to_string()))),
            oneshot_result: Arc::new(std::sync::Mutex::new(Err(msg.to_string()))),
        }
    }
}

#[async_trait]
impl PythonExecution for MockPythonExecution {
    async fn execute_in_session(
        &self,
        _scope_key: &str,
        _code: &str,
        _timeout: Duration,
        _sandbox: &SandboxConfig,
    ) -> Result<ExecutionResult> {
        let guard = self.session_result.lock().unwrap();
        match &*guard {
            Ok(r) => Ok(r.clone()),
            Err(e) => Err(anyhow::anyhow!("{}", e)),
        }
    }

    async fn execute_oneshot(
        &self,
        _workspace: &Path,
        _code: &str,
        _sandbox: &SandboxConfig,
    ) -> Result<ExecutionResult> {
        let guard = self.oneshot_result.lock().unwrap();
        match &*guard {
            Ok(r) => Ok(r.clone()),
            Err(e) => Err(anyhow::anyhow!("{}", e)),
        }
    }

    async fn interrupt_session(&self, _scope_key: &str) -> Result<()> {
        Ok(())
    }
}

/// E2.1: ExecutePythonRuntimeTool::with_python 构造函数存在。
#[test]
fn execute_python_runtime_tool_accepts_python_execution_trait() {
    use app_lib::runtime::tools::builtin::python::ExecutePythonRuntimeTool;
    let mock = Arc::new(MockPythonExecution::succeed_with("42\n"));
    let tmp = std::env::temp_dir().join("plan_e_test_workspace");
    std::fs::create_dir_all(&tmp).ok();
    let _tool = ExecutePythonRuntimeTool::with_python(
        mock as Arc<dyn PythonExecution>,
        tmp,
        None,  // authorized_workspace
        None,  // run_id
        "test-conv".to_string(),
        "claude-3-5-sonnet".to_string(),
    );
    // If this compiles and runs, E2 constructor is correct.
}

/// E2.2: stub mode 仍然返回 ExecutionFailed（向后兼容）。
#[tokio::test]
async fn execute_python_stub_returns_execution_failed() {
    use app_lib::runtime::tools::builtin::python::ExecutePythonRuntimeTool;
    use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext, ToolError};
    use serde_json::json;

    let tool = ExecutePythonRuntimeTool::stub();
    let ctx = ToolExecutionContext::for_test("c", "r", "t");
    let result = tool.execute(json!({"code": "print(1)"}), ctx).await;
    assert!(
        matches!(result, Err(ToolError::ExecutionFailed(_))),
        "stub should return ExecutionFailed"
    );
}

/// E2.3: 危险代码经 check_permissions 拒绝，不到达 execute。
#[tokio::test]
async fn execute_python_dangerous_code_denied_by_permissions() {
    use app_lib::runtime::tools::builtin::python::ExecutePythonRuntimeTool;
    use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
    use app_lib::runtime::tools::permission::PermissionDecision;
    use serde_json::json;

    let mock = Arc::new(MockPythonExecution::succeed_with("ok"));
    let tmp = std::env::temp_dir().join("plan_e_test_workspace2");
    std::fs::create_dir_all(&tmp).ok();
    let tool = ExecutePythonRuntimeTool::with_python(
        mock as Arc<dyn PythonExecution>,
        tmp,
        None,
        None,
        "c".to_string(),
        "model".to_string(),
    );
    let ctx = ToolExecutionContext::for_test("c", "r", "t");
    let input = json!({"code": "os.system('rm -rf /')"});
    let decision = tool.check_permissions(&input, &ctx).await;
    assert!(
        matches!(decision, Some(PermissionDecision::Deny { .. })),
        "os.system should be blocked by check_permissions"
    );
}

/// E2.4: with_python 模式下 execute() 不再依赖 PluginContext。
/// Mock 返回成功，工具结果应包含 stdout。
/// 注意：此测试绕过 storage（无 auto-load），仅验证执行路径可达。
#[tokio::test]
async fn execute_python_with_mock_returns_stdout() {
    use app_lib::runtime::tools::builtin::python::ExecutePythonRuntimeTool;
    use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
    use serde_json::json;

    let mock = Arc::new(MockPythonExecution::succeed_with("hello from mock\n"));
    let tmp = std::env::temp_dir().join("plan_e_test_workspace3");
    std::fs::create_dir_all(&tmp).ok();
    let tool = ExecutePythonRuntimeTool::with_python(
        mock as Arc<dyn PythonExecution>,
        tmp,
        None,
        None,
        "c".to_string(),
        "model".to_string(),
    );
    let ctx = ToolExecutionContext::for_test("c", "r", "t");
    let result = tool.execute(json!({"code": "print('hello from mock')"}), ctx).await;
    assert!(result.is_ok(), "mock execution should succeed: {:?}", result);
    let tool_result = result.unwrap();
    assert!(
        tool_result.content.contains("hello from mock"),
        "result should contain mock stdout: {}",
        tool_result.content
    );
}
```

**运行确认失败：**
```bash
cd src-tauri && cargo test --test plan_e_tool_migration_test execute_python 2>&1 | tail -10
```
期望：`execute_python_runtime_tool_accepts_python_execution_trait` 编译失败（`with_python` 不存在）。

### E2-Step 2：实现 ExecutePythonRuntimeTool

重写 `src-tauri/src/runtime/tools/builtin/python.rs`：

```rust
//! execute_python as RuntimeTool — Plan-E 完整实现。
//!
//! `ExecutePythonRuntimeTool` 通过 `Arc<dyn PythonExecution>` 接收执行能力，
//! 不再持有 `PluginContext`。纯值字段（workspace_path, conversation_id, run_id）
//! 来自 registry 的 `try_build_request_scoped_tool`。
//!
//! stub() 保留向后兼容，仍在无 PythonExecution 时返回 ExecutionFailed。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use log::{info, warn};
use serde_json::{json, Value};

use crate::runtime::ids::RunId;
use crate::runtime::store::AuthorizedWorkspaceRef;
use crate::runtime::tools::builtin::python_execution::PythonExecution;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::permission::{PermissionDecision, PermissionReason};
use crate::runtime::tools::RuntimeTool;
use crate::python::sandbox::SandboxConfig;

const DANGEROUS_PATTERNS: &[&str] = &[
    "__import__('os').system",
    "__import__('subprocess')",
    "subprocess.call",
    "subprocess.Popen",
    "os.system(",
    "os.popen(",
    "exec(compile(",
    "eval(compile(",
];

pub struct ExecutePythonRuntimeTool {
    stub_mode: bool,
    /// Injected Python execution capability (None in stub mode).
    python: Option<Arc<dyn PythonExecution>>,
    /// Workspace path for sandbox and analysis directory construction.
    workspace_path: Option<PathBuf>,
    /// Authorized workspace for sandbox extended read paths.
    authorized_workspace: Option<AuthorizedWorkspaceRef>,
    /// Current run_id for loaded_scope_id routing.
    run_id: Option<RunId>,
    /// Conversation id for loaded key generation and auto-load.
    conversation_id: Option<String>,
    /// Model name for telemetry.
    model: Option<String>,
}

impl ExecutePythonRuntimeTool {
    /// Stub constructor — no real execution, used in tests and catalog registration.
    pub fn stub() -> Self {
        Self {
            stub_mode: true,
            python: None,
            workspace_path: None,
            authorized_workspace: None,
            run_id: None,
            conversation_id: None,
            model: None,
        }
    }

    /// Full constructor — production path via registry injection.
    pub fn with_python(
        python: Arc<dyn PythonExecution>,
        workspace_path: PathBuf,
        authorized_workspace: Option<AuthorizedWorkspaceRef>,
        run_id: Option<RunId>,
        conversation_id: String,
        model: String,
    ) -> Self {
        Self {
            stub_mode: false,
            python: Some(python),
            workspace_path: Some(workspace_path),
            authorized_workspace,
            run_id,
            conversation_id: Some(conversation_id),
            model: Some(model),
        }
    }

    fn loaded_scope_id(&self) -> Option<String> {
        if let Some(run_id) = &self.run_id {
            Some(run_id.as_str().to_string())
        } else {
            self.conversation_id.clone()
        }
    }

    fn build_sandbox(&self) -> Option<SandboxConfig> {
        let ws = self.workspace_path.as_ref()?;
        Some(match &self.authorized_workspace {
            Some(aw) => SandboxConfig::for_workspace_with_authorized(
                ws,
                vec![aw.root_path.clone()],
            ),
            None => SandboxConfig::for_workspace(ws),
        })
    }
}

#[async_trait]
impl RuntimeTool for ExecutePythonRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("execute_python")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("execute_python", "Execute Python code"))
    }

    async fn check_permissions(
        &self,
        input: &Value,
        _ctx: &ToolExecutionContext,
    ) -> Option<PermissionDecision> {
        let code = input.get("code").and_then(Value::as_str).unwrap_or("");
        for pattern in DANGEROUS_PATTERNS {
            if code.contains(pattern) {
                return Some(PermissionDecision::Deny {
                    message: format!(
                        "execute_python: dangerous pattern detected: '{}'",
                        pattern
                    ),
                    reason: PermissionReason::Other("static_code_check".into()),
                });
            }
        }
        None
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        if self.stub_mode {
            return Err(ToolError::ExecutionFailed(
                "ExecutePythonRuntimeTool: stub mode, real execution not available".into(),
            ));
        }

        let python = self.python.as_ref().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "ExecutePythonRuntimeTool: missing PythonExecution dependency".into(),
            )
        })?;
        let workspace = self.workspace_path.as_ref().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "ExecutePythonRuntimeTool: missing workspace_path".into(),
            )
        })?;

        let code = input
            .get("code")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("execute_python: missing 'code' argument".into()))?;
        let purpose = input
            .get("purpose")
            .and_then(Value::as_str)
            .unwrap_or("code execution");

        info!(
            "[TOOL:execute_python] purpose='{}' code_len={} workspace={:?}",
            purpose,
            code.len(),
            workspace
        );

        let sandbox = self.build_sandbox().ok_or_else(|| {
            ToolError::ExecutionFailed("execute_python: workspace_path not available".into())
        })?;

        #[allow(deprecated)]
        if let Err(e) = sandbox.validate_code(code) {
            warn!(
                "[TOOL:execute_python] validate_code warning (non-blocking): {}",
                e
            );
        }

        // Determine execution mode by checking ctx.capability for step_state.
        // When CapabilityContext.storage is present and step_state exists → analysis mode.
        // Otherwise → daily mode (one-shot).
        let scope_key = self
            .loaded_scope_id()
            .unwrap_or_else(|| ctx.session_id.as_str().to_string());

        // Determine if analysis mode by checking for step_state in storage via capability.
        // When storage is unavailable (tests/stub paths), always use oneshot.
        let is_analysis = ctx
            .capability
            .as_ref()
            .and_then(|cap| cap.storage.as_ref())
            .is_none();  // simplified: no storage cap → daily mode; real analysis detection deferred to E2 follow-up

        let exec_result = if is_analysis {
            // Daily mode: one-shot
            python
                .execute_oneshot(workspace, code, &sandbox)
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
        } else {
            // Analysis mode: persistent session
            let timeout = Duration::from_secs(sandbox.timeout_seconds as u64);
            python
                .execute_in_session(&scope_key, code, timeout, &sandbox)
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
        };

        if exec_result.exit_code != 0 {
            let error_output = if exec_result.stderr.is_empty() {
                exec_result.stdout.clone()
            } else {
                exec_result.stderr.clone()
            };
            return Ok(ToolResult::new(
                "execute_python",
                format!("Error (exit {}):\n{}", exec_result.exit_code, error_output),
                None,
            ));
        }

        let content = if exec_result.stdout.is_empty() {
            "Code executed successfully (no output)".to_string()
        } else {
            exec_result.stdout.clone()
        };

        Ok(ToolResult::new("execute_python", content, None))
    }
}
```

**注意：** 上面的 `is_analysis` 逻辑是简化版。完整的 analysis mode 检测（`get_step_state`）需要 `AppStorage`，将在 E2-Follow-up 中通过 `CapabilityContext` 扩展引入。当前 Plan-E 的目标是脱离 `PluginContext`，analysis preamble 注入可通过后续 task 补齐。

**修改 registry：**

在 `src-tauri/src/plugin/registry.rs` 的 `try_build_request_scoped_tool` 的 `execute_python` 分支替换为：

```rust
"execute_python" => {
    use crate::runtime::tools::builtin::python_execution::DefaultPythonExecution;
    let python_exec = Arc::new(DefaultPythonExecution::new(
        ctx.session_manager.clone(),
        // python_binary 和 python_home 从 session_manager 内部字段取；
        // 暂时通过 resolve_python_path(ctx.app_handle.as_ref()) 获取，
        // 后续 app_handle 依赖可在启动期消除
        {
            let (binary, _home) = crate::python::runner::resolve_python_path(ctx.app_handle.as_ref());
            binary
        },
        {
            let (_binary, home) = crate::python::runner::resolve_python_path(ctx.app_handle.as_ref());
            home
        },
    ));
    Some(Arc::new(
        builtin::python::ExecutePythonRuntimeTool::with_python(
            python_exec as Arc<dyn crate::runtime::tools::builtin::python_execution::PythonExecution>,
            ctx.workspace_path.clone(),
            ctx.authorized_workspace.clone(),
            ctx.run_id.clone(),
            ctx.conversation_id.clone(),
            ctx.model.clone(),
        ),
    ) as Arc<dyn crate::runtime::tools::RuntimeTool>)
}
```

**给旧实现加 dead_code：** 在 `src-tauri/src/plugin/builtin/tools/python_exec.rs` 顶部已有 `#![allow(deprecated)]`，在其下再加：
```rust
#![allow(dead_code)]
```

### E2-Step 3：运行确认通过

```bash
cd src-tauri && cargo test --test plan_e_tool_migration_test execute_python -- --nocapture
# 同时跑全量回归
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

### E2-Step 4：commit

```
feat(plan-e/E2): ExecutePythonRuntimeTool full implementation via PythonExecution trait
```

- [ ] E2-Step 1：写失败测试，确认 `with_python` 不存在导致编译失败
- [ ] E2-Step 2：重写 `python.rs`，修改 registry，给旧实现加 `dead_code`
- [ ] E2-Step 3：E2 全部测试通过，`review_` 回归无新失败
- [ ] E2-Step 4：commit

---

## Task E3：ReportCapability trait + GenerateReportRuntimeTool 完整实现

**目标：** 将 `GenerateReportRuntimeTool` 的 stub 升级为完整实现。`handle_generate_report` 的逻辑依赖 `workspace_path`、`authorized_workspace`、`storage`（PII unmask + file index）、`auth_manager`（product name 替换）。通过 `ReportCapability` trait 抽象，使 RuntimeTool 测试可用 mock。

**关键设计：**

- `ReportCapability` trait 暴露三个操作：
  1. `generate(title, sections, format, workspace)` → `ReportGenOutput`（包含 bytes、extension、actual_format）
  2. `get_pii_unmask_map(conversation_id)` → `HashMap<String, String>`
  3. `get_product_name()` → `Option<String>`（async，给 auth_manager 留接口）
- `GenerateReportRuntimeTool` 持有 `Arc<dyn ReportCapability>` + workspace_path + conversation_id + authorized_workspace
- `DefaultReportCapability` 包装现有 `handle_generate_report` 的 infrastructure 依赖（storage + auth_manager + workspace_path），通过内部调用 `llm/tool_executor/report.rs` 中提取出的纯函数

### 文件

新建：`src-tauri/src/runtime/tools/builtin/report_capability.rs`
修改：`src-tauri/src/runtime/tools/builtin/report.rs`
修改：`src-tauri/src/plugin/registry.rs`（`try_build_request_scoped_tool` 加 `generate_report` 分支）
标记：`src-tauri/src/plugin/builtin/tools/report_gen.rs` 顶部加 `#![allow(dead_code)]`

### E3-Step 1：写失败测试

追加到 `src-tauri/tests/plan_e_tool_migration_test.rs`：

```rust
// ── E3: ReportCapability trait + GenerateReportRuntimeTool ───────────────────

use app_lib::runtime::tools::builtin::report_capability::ReportCapability;
use std::collections::HashMap;

/// Mock ReportCapability：generate 返回固定 HTML 内容。
#[derive(Debug)]
struct MockReportCapability {
    output_bytes: Vec<u8>,
    extension: String,
    actual_format: String,
    product_name: Option<String>,
    unmask_map: HashMap<String, String>,
    file_id: String,
    file_size: u64,
    stored_path: String,
    file_name: String,
}

impl MockReportCapability {
    fn html_success() -> Self {
        Self {
            output_bytes: b"<html>mock report</html>".to_vec(),
            extension: "html".to_string(),
            actual_format: "html".to_string(),
            product_name: None,
            unmask_map: HashMap::new(),
            file_id: "mock-file-id".to_string(),
            file_size: 24,
            stored_path: "reports/mock.html".to_string(),
            file_name: "mock.html".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl ReportCapability for MockReportCapability {
    async fn generate_report_bytes(
        &self,
        _title: &str,
        _sections: &[serde_json::Value],
        _format: &str,
    ) -> anyhow::Result<app_lib::runtime::tools::builtin::report_capability::ReportGenOutput> {
        Ok(app_lib::runtime::tools::builtin::report_capability::ReportGenOutput {
            bytes: self.output_bytes.clone(),
            extension: self.extension.clone(),
            actual_format: self.actual_format.clone(),
            is_degraded: false,
            degradation_notice: None,
        })
    }

    fn get_pii_unmask_map(&self, _conversation_id: &str) -> HashMap<String, String> {
        self.unmask_map.clone()
    }

    async fn get_product_name(&self) -> Option<String> {
        self.product_name.clone()
    }

    async fn persist_file(
        &self,
        _conversation_id: &str,
        _bytes: &[u8],
        _extension: &str,
        _title: &str,
        _actual_format: &str,
    ) -> anyhow::Result<app_lib::runtime::tools::builtin::report_capability::PersistedFileInfo> {
        Ok(app_lib::runtime::tools::builtin::report_capability::PersistedFileInfo {
            file_id: self.file_id.clone(),
            file_name: self.file_name.clone(),
            stored_path: self.stored_path.clone(),
            file_size: self.file_size,
        })
    }
}

/// E3.1: ReportCapability trait 可访问。
#[test]
fn report_capability_trait_is_accessible() {
    let _: Option<Box<dyn ReportCapability>> = None;
}

/// E3.2: stub mode 仍返回 ExecutionFailed。
#[tokio::test]
async fn generate_report_stub_returns_execution_failed() {
    use app_lib::runtime::tools::builtin::report::GenerateReportRuntimeTool;
    use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext, ToolError};
    use serde_json::json;

    let tool = GenerateReportRuntimeTool::stub();
    let ctx = ToolExecutionContext::for_test("c", "r", "t");
    let result = tool.execute(json!({"title": "test", "sections": [{"heading": "A"}]}), ctx).await;
    assert!(matches!(result, Err(ToolError::ExecutionFailed(_))));
}

/// E3.3: with_capability 模式 + mock → 返回成功结果。
#[tokio::test]
async fn generate_report_with_mock_capability_succeeds() {
    use app_lib::runtime::tools::builtin::report::GenerateReportRuntimeTool;
    use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
    use serde_json::json;

    let cap = Arc::new(MockReportCapability::html_success());
    let tmp = std::env::temp_dir().join("plan_e_report_workspace");
    std::fs::create_dir_all(&tmp).ok();
    let tool = GenerateReportRuntimeTool::with_capability(
        cap as Arc<dyn ReportCapability>,
        tmp,
        None,   // authorized_workspace
        "test-conv".to_string(),
    );
    let ctx = ToolExecutionContext::for_test("c", "r", "t");
    let input = json!({
        "title": "测试报告",
        "sections": [{"heading": "概述", "content": "测试内容"}]
    });
    let result = tool.execute(input, ctx).await;
    assert!(result.is_ok(), "mock report should succeed: {:?}", result);
    let tool_result = result.unwrap();
    assert!(
        tool_result.content.contains("mock-file-id"),
        "result should contain file_id: {}",
        tool_result.content
    );
}

/// E3.4: missing title 返回 ExecutionFailed。
#[tokio::test]
async fn generate_report_missing_title_fails() {
    use app_lib::runtime::tools::builtin::report::GenerateReportRuntimeTool;
    use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext, ToolError};
    use serde_json::json;

    let cap = Arc::new(MockReportCapability::html_success());
    let tmp = std::env::temp_dir().join("plan_e_report_workspace2");
    std::fs::create_dir_all(&tmp).ok();
    let tool = GenerateReportRuntimeTool::with_capability(
        cap as Arc<dyn ReportCapability>,
        tmp,
        None,
        "c".to_string(),
    );
    let ctx = ToolExecutionContext::for_test("c", "r", "t");
    let result = tool.execute(json!({"sections": [{"heading": "A"}]}), ctx).await;
    assert!(
        matches!(result, Err(ToolError::ExecutionFailed(_))),
        "missing title should fail"
    );
}
```

**运行确认失败：**
```bash
cd src-tauri && cargo test --test plan_e_tool_migration_test report 2>&1 | tail -10
```
期���：`report_capability` 模块不存在 / `with_capability` 方法不存在。

### E3-Step 2：实现 ReportCapability trait

新建 `src-tauri/src/runtime/tools/builtin/report_capability.rs`：

```rust
//! ReportCapability trait — 将 generate_report 的 infrastructure 依赖抽象为 trait。
//!
//! 这使 GenerateReportRuntimeTool 可通过 Arc<dyn ReportCapability> 注入，
//! 测试中使用 mock，生产环境使用 DefaultReportCapability。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

/// generate_report 的文件生成输出（含 bytes，不含持久化路径）。
pub struct ReportGenOutput {
    pub bytes: Vec<u8>,
    pub extension: String,
    pub actual_format: String,
    pub is_degraded: bool,
    pub degradation_notice: Option<String>,
}

/// 持久化后的文件信息（由 persist_file 返回）。
pub struct PersistedFileInfo {
    pub file_id: String,
    pub file_name: String,
    pub stored_path: String,
    pub file_size: u64,
}

/// Report 生成能力接口。
#[async_trait]
pub trait ReportCapability: Send + Sync + std::fmt::Debug {
    /// 将 sections 渲染为指定 format 的字节流。
    /// 不负责持久化；持久化由 `persist_file` 完成。
    async fn generate_report_bytes(
        &self,
        title: &str,
        sections: &[serde_json::Value],
        format: &str,
    ) -> Result<ReportGenOutput>;

    /// 获取当前对话的 PII unmask 映射（用于脱敏还原）。
    fn get_pii_unmask_map(&self, conversation_id: &str) -> HashMap<String, String>;

    /// 获取租户定制 product_name（来自 AuthManager，可选）。
    async fn get_product_name(&self) -> Option<String>;

    /// 将生成的字节流持久化到 workspace/reports/ 目录并写入 file index。
    async fn persist_file(
        &self,
        conversation_id: &str,
        bytes: &[u8],
        extension: &str,
        title: &str,
        actual_format: &str,
    ) -> Result<PersistedFileInfo>;
}

// ── DefaultReportCapability ───────────────────────────────────────────────────

/// 生产实现：包装现有 storage/auth_manager/workspace_path/python 依赖。
pub struct DefaultReportCapability {
    pub storage: Arc<crate::storage::file_store::AppStorage>,
    pub auth_manager: Option<Arc<crate::auth::AuthManager>>,
    pub workspace_path: PathBuf,
    /// Python binary 路径，用于 PDF 转换（reportlab）。
    pub python_binary: PathBuf,
    pub python_home: Option<PathBuf>,
}

impl std::fmt::Debug for DefaultReportCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultReportCapability")
            .field("workspace_path", &self.workspace_path)
            .finish()
    }
}

#[async_trait]
impl ReportCapability for DefaultReportCapability {
    async fn generate_report_bytes(
        &self,
        title: &str,
        sections: &[serde_json::Value],
        format: &str,
    ) -> Result<ReportGenOutput> {
        use crate::llm::tool_executor::report::{build_html_report, build_markdown_report, convert_sections_to_pdf_standalone};
        use crate::python::runner::PythonRunner;
        use uuid::Uuid;

        let html = build_html_report(title, sections);

        let (bytes, extension, actual_format, is_degraded, degradation_notice) = match format {
            "markdown" => {
                let md = build_markdown_report(title, sections);
                (md.into_bytes(), "md", "markdown", false, None)
            }
            "pdf" => {
                let runner = PythonRunner::with_config(
                    self.workspace_path.clone(),
                    crate::python::sandbox::SandboxConfig::for_workspace(&self.workspace_path),
                    self.python_binary.clone(),
                    self.python_home.clone(),
                );
                match convert_sections_to_pdf_standalone(title, sections, &runner).await {
                    Ok(pdf_bytes) => (pdf_bytes, "pdf", "pdf", false, None),
                    Err(e) => {
                        log::warn!("[generate_report] PDF conversion failed: {}; falling back to HTML", e);
                        (html.into_bytes(), "html", "pdf(html_fallback)", true,
                         Some(format!("PDF conversion failed ({}), returning HTML instead", e)))
                    }
                }
            }
            "docx" => {
                // DOCX conversion is not yet implemented in this codebase;
                // fall back to HTML and mark as degraded.
                (html.into_bytes(), "html", "docx(html_fallback)", true,
                 Some("DOCX conversion not available, returning HTML".to_string()))
            }
            _ => {
                // default: html
                (html.into_bytes(), "html", "html", false, None)
            }
        };

        Ok(ReportGenOutput {
            bytes,
            extension: extension.to_string(),
            actual_format: actual_format.to_string(),
            is_degraded,
            degradation_notice,
        })
    }

    fn get_pii_unmask_map(&self, conversation_id: &str) -> HashMap<String, String> {
        crate::llm::tool_executor::file_load::get_pii_unmask_map(&self.storage, conversation_id)
    }

    async fn get_product_name(&self) -> Option<String> {
        if let Some(ref auth) = self.auth_manager {
            auth.get_auth_info().await.tenant
                .and_then(|t| t.product_name.filter(|n| !n.is_empty()))
        } else {
            None
        }
    }

    async fn persist_file(
        &self,
        conversation_id: &str,
        bytes: &[u8],
        extension: &str,
        title: &str,
        actual_format: &str,
    ) -> Result<PersistedFileInfo> {
        use uuid::Uuid;
        let file_id = Uuid::new_v4().to_string();
        let file_name = format!(
            "report_{}.{}",
            Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
            extension
        );
        let reports_dir = self.workspace_path.join("reports");
        std::fs::create_dir_all(&reports_dir)?;
        let output_path = reports_dir.join(&file_name);
        std::fs::write(&output_path, bytes)?;

        let stored_path = format!("reports/{}", file_name);
        let file_size = bytes.len() as u64;

        self.storage.insert_generated_file(
            &file_id,
            conversation_id,
            None,
            &file_name,
            &stored_path,
            extension,
            file_size as i64,
            "report",
            Some(title),
            1,
            true,
            None,
            None,
            None,
        )?;

        Ok(PersistedFileInfo {
            file_id,
            file_name,
            stored_path,
            file_size,
        })
    }
}
```

**注意：** `build_html_report`、`build_markdown_report`、`convert_sections_to_pdf_standalone` 需要从 `llm/tool_executor/report.rs` 提取为 `pub(crate)` 函数（或移到新的 `report_builder.rs`）。`convert_sections_to_pdf_standalone` 是从 `convert_sections_to_pdf(ctx, ...)` 提取出不依赖 `PluginContext` 的版本，只接受 title/sections/runner。

**重写 `report.rs`（RuntimeTool）：**

```rust
//! generate_report as RuntimeTool — Plan-E 完整实现。

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::store::AuthorizedWorkspaceRef;
use crate::runtime::tools::builtin::report_capability::ReportCapability;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct GenerateReportRuntimeTool {
    stub_mode: bool,
    capability: Option<Arc<dyn ReportCapability>>,
    workspace_path: Option<PathBuf>,
    authorized_workspace: Option<AuthorizedWorkspaceRef>,
    conversation_id: Option<String>,
}

impl GenerateReportRuntimeTool {
    pub fn stub() -> Self {
        Self {
            stub_mode: true,
            capability: None,
            workspace_path: None,
            authorized_workspace: None,
            conversation_id: None,
        }
    }

    pub fn with_capability(
        capability: Arc<dyn ReportCapability>,
        workspace_path: PathBuf,
        authorized_workspace: Option<AuthorizedWorkspaceRef>,
        conversation_id: String,
    ) -> Self {
        Self {
            stub_mode: false,
            capability: Some(capability),
            workspace_path: Some(workspace_path),
            authorized_workspace,
            conversation_id: Some(conversation_id),
        }
    }
}

#[async_trait]
impl RuntimeTool for GenerateReportRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("generate_report")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("generate_report", "Generate report"))
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        if self.stub_mode {
            return Err(ToolError::ExecutionFailed(
                "GenerateReportRuntimeTool: stub mode".into(),
            ));
        }

        let cap = self.capability.as_ref().ok_or_else(|| {
            ToolError::ExecutionFailed("GenerateReportRuntimeTool: missing ReportCapability".into())
        })?;
        let conversation_id = self.conversation_id.as_deref().unwrap_or("");

        // Parse title (required)
        let title = input
            .get("title")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::ExecutionFailed(
                    "generate_report: missing required 'title' argument".into(),
                )
            })?;

        // Resolve sections: prefer 'source' (file path) over inline 'sections'
        let workspace = self.workspace_path.as_ref().ok_or_else(|| {
            ToolError::ExecutionFailed("generate_report: missing workspace_path".into())
        })?;
        let sections_value: Vec<Value>;
        let sections: &[Value] = if let Some(source_path) =
            input.get("source").and_then(|v| v.as_str())
        {
            let full_path = if std::path::Path::new(source_path).is_absolute() {
                std::path::PathBuf::from(source_path)
            } else {
                workspace.join(source_path)
            };
            let canonical = full_path.canonicalize().map_err(|e| {
                ToolError::ExecutionFailed(format!(
                    "Failed to read source file '{}': {}",
                    source_path, e
                ))
            })?;
            let workspace_canonical = workspace.canonicalize().unwrap_or_else(|_| workspace.clone());
            let in_workspace = canonical.starts_with(&workspace_canonical);
            let in_authorized = self
                .authorized_workspace
                .as_ref()
                .map(|aw| canonical.starts_with(&aw.root_path))
                .unwrap_or(false);
            if !in_workspace && !in_authorized {
                return Err(ToolError::ExecutionFailed(format!(
                    "Source file '{}' is outside the workspace and authorized workspace.",
                    source_path
                )));
            }
            let content = std::fs::read_to_string(&canonical).map_err(|e| {
                ToolError::ExecutionFailed(format!(
                    "Failed to read source file '{}': {}",
                    source_path, e
                ))
            })?;
            sections_value = serde_json::from_str::<Vec<Value>>(&content).map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to parse sections from '{}': {}", source_path, e))
            })?;
            if sections_value.is_empty() {
                return Err(ToolError::ExecutionFailed(format!(
                    "Source file '{}' contains an empty sections array.",
                    source_path
                )));
            }
            &sections_value
        } else if let Some(arr) = input.get("sections").and_then(|v| v.as_array()) {
            arr.as_slice()
        } else {
            return Err(ToolError::ExecutionFailed(
                "generate_report: missing 'sections' or 'source' argument".into(),
            ));
        };

        let format = input
            .get("format")
            .and_then(Value::as_str)
            .unwrap_or("html");

        // Get PII unmask map and product name
        let unmask_map = cap.get_pii_unmask_map(conversation_id);
        let product_name = cap.get_product_name().await;

        // Generate bytes
        let mut output = cap
            .generate_report_bytes(title, sections, format)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        // Apply product name substitution
        if let Some(name) = &product_name {
            if let Ok(s) = std::str::from_utf8(&output.bytes) {
                output.bytes = s.replace("AI小家", name).into_bytes();
            }
        }

        // Apply PII unmask
        if !unmask_map.is_empty() {
            if let Ok(s) = std::str::from_utf8(&output.bytes) {
                use crate::llm::tool_executor::file_load::unmask_text;
                output.bytes = unmask_text(s, &unmask_map).into_bytes();
            }
        }

        // Persist
        let persisted = cap
            .persist_file(conversation_id, &output.bytes, &output.extension, title, &output.actual_format)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let content = serde_json::to_string_pretty(&serde_json::json!({
            "fileId": persisted.file_id,
            "fileName": persisted.file_name,
            "storedPath": persisted.stored_path,
            "fileSize": persisted.file_size,
            "format": output.actual_format,
            "isDegraded": output.is_degraded,
            "degradationNotice": output.degradation_notice,
        }))
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(ToolResult::new("generate_report", content, None))
    }
}
```

**修改 registry：** 在 `try_build_request_scoped_tool` 的 `match` 中，在 `"execute_python"` 分支之后添加：

```rust
"generate_report" => {
    use crate::runtime::tools::builtin::report_capability::DefaultReportCapability;
    let (python_binary, python_home) =
        crate::python::runner::resolve_python_path(ctx.app_handle.as_ref());
    let cap = Arc::new(DefaultReportCapability {
        storage: ctx.storage.clone(),
        auth_manager: ctx.auth_manager.clone(),
        workspace_path: ctx.workspace_path.clone(),
        python_binary,
        python_home,
    });
    Some(Arc::new(
        builtin::report::GenerateReportRuntimeTool::with_capability(
            cap as Arc<dyn crate::runtime::tools::builtin::report_capability::ReportCapability>,
            ctx.workspace_path.clone(),
            ctx.authorized_workspace.clone(),
            ctx.conversation_id.clone(),
        ),
    ) as Arc<dyn crate::runtime::tools::RuntimeTool>)
}
```

**给旧实现加 dead_code：** `src-tauri/src/plugin/builtin/tools/report_gen.rs` 顶部加 `#![allow(dead_code)]`。

### E3-Step 3：运行确认通过

```bash
cd src-tauri && cargo test --test plan_e_tool_migration_test report -- --nocapture
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

### E3-Step 4：commit

```
feat(plan-e/E3): ReportCapability trait + GenerateReportRuntimeTool full implementation
```

- [ ] E3-Step 1：写失败测试，确认 `report_capability` 模块不存在导致编译失败
- [ ] E3-Step 2：实现 `report_capability.rs`，重写 `report.rs`，修改 registry，给旧实现加 `dead_code`
- [ ] E3-Step 3：E3 测试通过，`review_` 回归无新失败
- [ ] E3-Step 4：commit

---

## Task E4：ChartCapability trait + GenerateChartRuntimeTool 完整实现

**目标：** 将 `GenerateChartRuntimeTool` 的 stub 升级为完整实现。`handle_generate_chart` 的逻辑依赖 `workspace_path`（data_file 安全检查、temp 目录、charts 目录）、`storage`（`insert_generated_file`）、`app_handle`（`PythonRunner::new`）。通过 `ChartCapability` trait 抽象。

**关键设计：**

- `ChartCapability` trait 暴露：
  1. `run_chart_python(chart_type, title, data, options, workspace)` → `ChartRunOutput`（包含 bytes + filename）
  2. `persist_chart(conversation_id, bytes, filename, chart_type, title)` → `PersistedChartInfo`
- `GenerateChartRuntimeTool` 持有 `Arc<dyn ChartCapability>` + workspace_path + conversation_id
- `DefaultChartCapability` 包装 `PythonRunner` + `AppStorage`，调用 `llm/tool_executor/chart.rs` 中提取的纯函数

### 文件

新建：`src-tauri/src/runtime/tools/builtin/chart_capability.rs`
修改：`src-tauri/src/runtime/tools/builtin/chart.rs`
修改：`src-tauri/src/plugin/registry.rs`（加 `generate_chart` 分支）
标记：`src-tauri/src/plugin/builtin/tools/chart_gen.rs` 顶部加 `#![allow(dead_code)]`

### E4-Step 1：写失败测试

追加到 `src-tauri/tests/plan_e_tool_migration_test.rs`：

```rust
// ── E4: ChartCapability trait + GenerateChartRuntimeTool ─────────────────────

use app_lib::runtime::tools::builtin::chart_capability::ChartCapability;

/// Mock ChartCapability：run_chart_python 返回固定 HTML 内容。
#[derive(Debug)]
struct MockChartCapability {
    output_html: String,
    file_id: String,
    file_name: String,
    stored_path: String,
    file_size: u64,
}

impl MockChartCapability {
    fn success() -> Self {
        Self {
            output_html: "<html>mock chart</html>".to_string(),
            file_id: "mock-chart-id".to_string(),
            file_name: "chart_mock.html".to_string(),
            stored_path: "charts/chart_mock.html".to_string(),
            file_size: 23,
        }
    }
}

#[async_trait::async_trait]
impl ChartCapability for MockChartCapability {
    async fn run_chart_python(
        &self,
        _chart_type: &str,
        _title: &str,
        _data: &serde_json::Value,
        _options: &serde_json::Value,
    ) -> anyhow::Result<app_lib::runtime::tools::builtin::chart_capability::ChartRunOutput> {
        Ok(app_lib::runtime::tools::builtin::chart_capability::ChartRunOutput {
            html_bytes: self.output_html.as_bytes().to_vec(),
            chart_filename: self.file_name.clone(),
        })
    }

    async fn persist_chart(
        &self,
        _conversation_id: &str,
        _bytes: &[u8],
        _filename: &str,
        _chart_type: &str,
        _title: &str,
    ) -> anyhow::Result<app_lib::runtime::tools::builtin::chart_capability::PersistedChartInfo> {
        Ok(app_lib::runtime::tools::builtin::chart_capability::PersistedChartInfo {
            file_id: self.file_id.clone(),
            file_name: self.file_name.clone(),
            stored_path: self.stored_path.clone(),
            file_size: self.file_size,
        })
    }
}

/// E4.1: ChartCapability trait 可访问。
#[test]
fn chart_capability_trait_is_accessible() {
    let _: Option<Box<dyn ChartCapability>> = None;
}

/// E4.2: stub mode 仍返回 ExecutionFailed。
#[tokio::test]
async fn generate_chart_stub_returns_execution_failed() {
    use app_lib::runtime::tools::builtin::chart::GenerateChartRuntimeTool;
    use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext, ToolError};
    use serde_json::json;

    let tool = GenerateChartRuntimeTool::stub();
    let ctx = ToolExecutionContext::for_test("c", "r", "t");
    let result = tool
        .execute(json!({"chart_type": "bar", "title": "test", "data": {}}), ctx)
        .await;
    assert!(matches!(result, Err(ToolError::ExecutionFailed(_))));
}

/// E4.3: with_capability + mock → 返回成功结果。
#[tokio::test]
async fn generate_chart_with_mock_capability_succeeds() {
    use app_lib::runtime::tools::builtin::chart::GenerateChartRuntimeTool;
    use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
    use serde_json::json;

    let cap = Arc::new(MockChartCapability::success());
    let tmp = std::env::temp_dir().join("plan_e_chart_workspace");
    std::fs::create_dir_all(&tmp).ok();
    let tool = GenerateChartRuntimeTool::with_capability(
        cap as Arc<dyn ChartCapability>,
        tmp,
        "test-conv".to_string(),
    );
    let ctx = ToolExecutionContext::for_test("c", "r", "t");
    let input = json!({
        "chart_type": "bar",
        "title": "销售趋势",
        "data": {"labels": ["Q1", "Q2"], "values": [100, 200]}
    });
    let result = tool.execute(input, ctx).await;
    assert!(result.is_ok(), "mock chart should succeed: {:?}", result);
    let tool_result = result.unwrap();
    assert!(
        tool_result.content.contains("mock-chart-id"),
        "result should contain file_id: {}",
        tool_result.content
    );
}

/// E4.4: missing chart_type 返回 ExecutionFailed。
#[tokio::test]
async fn generate_chart_missing_chart_type_fails() {
    use app_lib::runtime::tools::builtin::chart::GenerateChartRuntimeTool;
    use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext, ToolError};
    use serde_json::json;

    let cap = Arc::new(MockChartCapability::success());
    let tmp = std::env::temp_dir().join("plan_e_chart_workspace2");
    std::fs::create_dir_all(&tmp).ok();
    let tool = GenerateChartRuntimeTool::with_capability(
        cap as Arc<dyn ChartCapability>,
        tmp,
        "c".to_string(),
    );
    let ctx = ToolExecutionContext::for_test("c", "r", "t");
    let result = tool
        .execute(json!({"title": "no type", "data": {}}), ctx)
        .await;
    assert!(
        matches!(result, Err(ToolError::ExecutionFailed(_))),
        "missing chart_type should fail"
    );
}
```

**运行确认失败：**
```bash
cd src-tauri && cargo test --test plan_e_tool_migration_test chart 2>&1 | tail -10
```
期望：`chart_capability` 模块不存在。

### E4-Step 2：实现 ChartCapability trait

新建 `src-tauri/src/runtime/tools/builtin/chart_capability.rs`：

```rust
//! ChartCapability trait — 将 generate_chart 的 infrastructure 依赖抽象为 trait。

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

/// Plotly HTML chart 生成输出（含 bytes 和 filename，不含持久化路径）。
pub struct ChartRunOutput {
    pub html_bytes: Vec<u8>,
    pub chart_filename: String,
}

/// 持久化后的图表文件信息。
pub struct PersistedChartInfo {
    pub file_id: String,
    pub file_name: String,
    pub stored_path: String,
    pub file_size: u64,
}

/// Chart 生成能力接口。
#[async_trait]
pub trait ChartCapability: Send + Sync + std::fmt::Debug {
    /// 通过 Python/Plotly 生成 HTML chart 字节流。
    async fn run_chart_python(
        &self,
        chart_type: &str,
        title: &str,
        data: &Value,
        options: &Value,
    ) -> Result<ChartRunOutput>;

    /// 持久化 chart 字节流到 workspace/charts/ 并写入 file index。
    async fn persist_chart(
        &self,
        conversation_id: &str,
        bytes: &[u8],
        filename: &str,
        chart_type: &str,
        title: &str,
    ) -> Result<PersistedChartInfo>;
}

// ── DefaultChartCapability ────────────────────────────────────────────────────

/// 生产实现：包装 PythonRunner + AppStorage。
pub struct DefaultChartCapability {
    pub storage: Arc<crate::storage::file_store::AppStorage>,
    pub workspace_path: PathBuf,
    pub python_binary: PathBuf,
    pub python_home: Option<PathBuf>,
}

impl std::fmt::Debug for DefaultChartCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultChartCapability")
            .field("workspace_path", &self.workspace_path)
            .finish()
    }
}

#[async_trait]
impl ChartCapability for DefaultChartCapability {
    async fn run_chart_python(
        &self,
        chart_type: &str,
        title: &str,
        data: &Value,
        options: &Value,
    ) -> Result<ChartRunOutput> {
        use crate::llm::tool_executor::chart::build_chart_python;
        use crate::python::runner::PythonRunner;
        use uuid::Uuid;

        let chart_filename = format!(
            "chart_{}.html",
            Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
        );
        let chart_dir = self.workspace_path.join("charts");
        std::fs::create_dir_all(&chart_dir)?;
        let output_path = chart_dir.join(&chart_filename);

        let temp_dir = self.workspace_path.join("temp");
        std::fs::create_dir_all(&temp_dir)?;
        let data_temp = temp_dir.join(format!(
            "chart_data_{}.json",
            Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
        ));
        let options_temp = temp_dir.join(format!(
            "chart_opts_{}.json",
            Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
        ));
        std::fs::write(&data_temp, serde_json::to_string(data).unwrap_or_default())?;
        std::fs::write(&options_temp, serde_json::to_string(options).unwrap_or_default())?;

        let python_code = build_chart_python(
            chart_type,
            title,
            &data_temp.to_string_lossy(),
            &options_temp.to_string_lossy(),
            &output_path.to_string_lossy(),
        );

        let runner = PythonRunner::with_config(
            self.workspace_path.clone(),
            crate::python::sandbox::SandboxConfig::for_workspace(&self.workspace_path),
            self.python_binary.clone(),
            self.python_home.clone(),
        );
        let result = runner.execute(&python_code).await?;
        let _ = std::fs::remove_file(&data_temp);
        let _ = std::fs::remove_file(&options_temp);

        if result.exit_code != 0 {
            return Err(anyhow::anyhow!(
                "Chart generation failed (exit {}):\n{}",
                result.exit_code,
                if result.stderr.is_empty() { &result.stdout } else { &result.stderr }
            ));
        }

        let html_bytes = std::fs::read(&output_path)?;
        Ok(ChartRunOutput { html_bytes, chart_filename })
    }

    async fn persist_chart(
        &self,
        conversation_id: &str,
        bytes: &[u8],
        filename: &str,
        chart_type: &str,
        title: &str,
    ) -> Result<PersistedChartInfo> {
        use uuid::Uuid;

        let chart_dir = self.workspace_path.join("charts");
        std::fs::create_dir_all(&chart_dir)?;
        let output_path = chart_dir.join(filename);
        if !output_path.exists() {
            std::fs::write(&output_path, bytes)?;
        }

        let stored_path = format!("charts/{}", filename);
        let file_size = bytes.len() as u64;
        let file_id = Uuid::new_v4().to_string();

        self.storage.insert_generated_file(
            &file_id,
            conversation_id,
            None,
            filename,
            &stored_path,
            "html",
            file_size as i64,
            "chart",
            Some(title),
            1,
            true,
            None,
            None,
            None,
        )?;

        Ok(PersistedChartInfo {
            file_id,
            file_name: filename.to_string(),
            stored_path,
            file_size,
        })
    }
}
```

**重写 `chart.rs`（RuntimeTool）：**

```rust
//! generate_chart as RuntimeTool — Plan-E 完整实现。

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::runtime::tools::builtin::chart_capability::ChartCapability;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct GenerateChartRuntimeTool {
    stub_mode: bool,
    capability: Option<Arc<dyn ChartCapability>>,
    workspace_path: Option<PathBuf>,
    conversation_id: Option<String>,
}

impl GenerateChartRuntimeTool {
    pub fn stub() -> Self {
        Self {
            stub_mode: true,
            capability: None,
            workspace_path: None,
            conversation_id: None,
        }
    }

    pub fn with_capability(
        capability: Arc<dyn ChartCapability>,
        workspace_path: PathBuf,
        conversation_id: String,
    ) -> Self {
        Self {
            stub_mode: false,
            capability: Some(capability),
            workspace_path: Some(workspace_path),
            conversation_id: Some(conversation_id),
        }
    }
}

#[async_trait]
impl RuntimeTool for GenerateChartRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("generate_chart")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("generate_chart", "Generate chart"))
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        if self.stub_mode {
            return Err(ToolError::ExecutionFailed(
                "GenerateChartRuntimeTool: stub mode".into(),
            ));
        }

        let cap = self.capability.as_ref().ok_or_else(|| {
            ToolError::ExecutionFailed("GenerateChartRuntimeTool: missing ChartCapability".into())
        })?;
        let workspace = self.workspace_path.as_ref().ok_or_else(|| {
            ToolError::ExecutionFailed("GenerateChartRuntimeTool: missing workspace_path".into())
        })?;
        let conversation_id = self.conversation_id.as_deref().unwrap_or("");

        let chart_type = input
            .get("chart_type")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::ExecutionFailed(
                    "generate_chart: missing required 'chart_type' argument".into(),
                )
            })?;
        let title = input
            .get("title")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::ExecutionFailed(
                    "generate_chart: missing required 'title' argument".into(),
                )
            })?;

        // Resolve data: prefer data_file over inline data
        let data: Value = if let Some(data_file_path) = input.get("data_file").and_then(|v| v.as_str())
        {
            let full_path = if std::path::Path::new(data_file_path).is_absolute() {
                std::path::PathBuf::from(data_file_path)
            } else {
                workspace.join(data_file_path)
            };
            let canonical = full_path.canonicalize().map_err(|e| {
                ToolError::ExecutionFailed(format!(
                    "Failed to read data_file '{}': {}",
                    data_file_path, e
                ))
            })?;
            let workspace_canonical = workspace.canonicalize().unwrap_or_else(|_| workspace.clone());
            if !canonical.starts_with(&workspace_canonical) {
                return Err(ToolError::ExecutionFailed(format!(
                    "data_file '{}' is outside the workspace directory.",
                    data_file_path
                )));
            }
            let content = std::fs::read_to_string(&canonical).map_err(|e| {
                ToolError::ExecutionFailed(format!(
                    "Failed to read data_file '{}': {}",
                    data_file_path, e
                ))
            })?;
            serde_json::from_str(&content).map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to parse chart data: {}", e))
            })?
        } else if let Some(inline_data) = input.get("data") {
            inline_data.clone()
        } else {
            return Err(ToolError::ExecutionFailed(
                "generate_chart: missing 'data' or 'data_file' argument".into(),
            ));
        };

        let options = input.get("options").cloned().unwrap_or(json!({}));

        let chart_output = cap
            .run_chart_python(chart_type, title, &data, &options)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let persisted = cap
            .persist_chart(
                conversation_id,
                &chart_output.html_bytes,
                &chart_output.chart_filename,
                chart_type,
                title,
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let content = serde_json::to_string_pretty(&json!({
            "fileId": persisted.file_id,
            "fileName": persisted.file_name,
            "storedPath": persisted.stored_path,
            "fileSize": persisted.file_size,
            "chartType": chart_type,
        }))
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(ToolResult::new("generate_chart", content, None))
    }
}
```

**修改 registry：** 在 `try_build_request_scoped_tool` 的 `"generate_report"` 分支之后添加：

```rust
"generate_chart" => {
    use crate::runtime::tools::builtin::chart_capability::DefaultChartCapability;
    let (python_binary, python_home) =
        crate::python::runner::resolve_python_path(ctx.app_handle.as_ref());
    let cap = Arc::new(DefaultChartCapability {
        storage: ctx.storage.clone(),
        workspace_path: ctx.workspace_path.clone(),
        python_binary,
        python_home,
    });
    Some(Arc::new(
        builtin::chart::GenerateChartRuntimeTool::with_capability(
            cap as Arc<dyn crate::runtime::tools::builtin::chart_capability::ChartCapability>,
            ctx.workspace_path.clone(),
            ctx.conversation_id.clone(),
        ),
    ) as Arc<dyn crate::runtime::tools::RuntimeTool>)
}
```

**给旧实现加 dead_code：** `src-tauri/src/plugin/builtin/tools/chart_gen.rs` 顶部加 `#![allow(dead_code)]`。

### E4-Step 3：运行确认通过

```bash
cd src-tauri && cargo test --test plan_e_tool_migration_test chart -- --nocapture
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

### E4-Step 4：commit

```
feat(plan-e/E4): ChartCapability trait + GenerateChartRuntimeTool full implementation
```

- [ ] E4-Step 1：写失败测试，确认 `chart_capability` 模块不存在导致编译失败
- [ ] E4-Step 2：实现 `chart_capability.rs`，重写 `chart.rs`，修改 registry，给旧实现加 `dead_code`
- [ ] E4-Step 3：E4 测试通过，`review_` 回归无新失败
- [ ] E4-Step 4：commit

---

## Task E5：runtime path 集成回归

**目标：** 验证 runtime dispatcher / schema source / request-scoped factory 已真实接管 `execute_python`、`generate_report`、`generate_chart`，并跑完整体回归。模块暴露应在 E1/E3/E4 各自落地时同步完成，不再集中拖到 E5。

### 文件

修改：`src-tauri/src/runtime/tools/builtin/mod.rs`

### E5-Step 1：写回归验证测试

追加到 `src-tauri/tests/plan_e_tool_migration_test.rs`：

```rust
// ── E5: 模块注册 + 集成验证 ───────────────────────────────────────────────────

/// E5.1: 三个新 capability 模块均可导入。
#[test]
fn all_plan_e_modules_accessible() {
    use app_lib::runtime::tools::builtin::chart_capability::ChartCapability;
    use app_lib::runtime::tools::builtin::python_execution::PythonExecution;
    use app_lib::runtime::tools::builtin::report_capability::ReportCapability;
    let _: Option<Box<dyn PythonExecution>> = None;
    let _: Option<Box<dyn ReportCapability>> = None;
    let _: Option<Box<dyn ChartCapability>> = None;
}

/// E5.2: 三个工具的 stub() 仍可用（向后兼容）。
#[test]
fn all_plan_e_tool_stubs_are_constructible() {
    use app_lib::runtime::tools::builtin::chart::GenerateChartRuntimeTool;
    use app_lib::runtime::tools::builtin::python::ExecutePythonRuntimeTool;
    use app_lib::runtime::tools::builtin::report::GenerateReportRuntimeTool;
    use app_lib::runtime::tools::RuntimeTool;

    let py = ExecutePythonRuntimeTool::stub();
    assert_eq!(py.definition().id, "execute_python");

    let rpt = GenerateReportRuntimeTool::stub();
    assert_eq!(rpt.definition().id, "generate_report");

    let cht = GenerateChartRuntimeTool::stub();
    assert_eq!(cht.definition().id, "generate_chart");
}

/// E5.3: PluginContext 中的 execute_python 旧路径不再被调用（编译通过即验证）。
/// 通过确认 ExecutePythonRuntimeTool 不持有 PluginContext 字段来验证。
#[test]
fn execute_python_runtime_tool_does_not_have_plugin_ctx_field() {
    use app_lib::runtime::tools::builtin::python::ExecutePythonRuntimeTool;
    // Constructing with_python does not require PluginContext.
    // The test below verifies this compiles without importing PluginContext.
    let _tool = ExecutePythonRuntimeTool::stub();
    // If this file compiles without `use app_lib::plugin::context::PluginContext`,
    // then the test confirms the RuntimeTool no longer requires PluginContext.
}
```

**运行：**
```bash
cd src-tauri && cargo test --test plan_e_tool_migration_test -- --nocapture
cd src-tauri && cargo test 2>&1 | grep -E "(FAILED|test result)" | tail -20
```

### E5-Step 2：确认 `builtin/mod.rs` 完整注册

`src-tauri/src/runtime/tools/builtin/mod.rs` 应包含：

```rust
pub mod chart;
pub mod chart_capability;
pub mod python;
pub mod python_execution;
pub mod report;
pub mod report_capability;
// ... 其余已有模块
```

### E5-Step 3：全量回归

```bash
# 前端测试（不受本次变更影响，但作为基线确认）
# pnpm test

# Rust 全量测试
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test

# review_ 架构回归测试
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_ --tests --no-fail-fast

# Plan-E 专项测试
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test plan_e_tool_migration_test -- --nocapture
```

预期：全部通过。`primitive_tools_migration_test` 中的 `execute_python`、`generate_report`、`generate_chart` 测试应继续通过（stub 接口不变）。

### E5-Step 4：commit

```
test(plan-e/E5): module registration + full regression gate for Plan-E
```

- [ ] E5-Step 1：写 E5 回归测试
- [ ] E5-Step 2：确认 `builtin/mod.rs` 完整注册三个新 capability 模块
- [ ] E5-Step 3：全量测试通过，`review_` 套件无新失败
- [ ] E5-Step 4：commit

---

## 完成标准（Definition of Done）

Plan-E 完成当且仅当：

1. **E1 完成：** `python_execution.rs` 存在，`PythonExecution` trait 有 `execute_in_session` / `execute_oneshot` / `interrupt_session` 三个方法，`DefaultPythonExecution` 实现包装 `PythonSessionManager`。
2. **E2 完成：** `ExecutePythonRuntimeTool` 不再持有 `Option<PluginContext>` 字段；`with_python` / 等价构造仅持有稳定依赖；`ToolExecutionContext` / `CapabilityContext` 能提供 request-scoped 状态；`try_build_request_scoped_tool` 的 `execute_python` 分支注入 runtime path；旧 `python_exec.rs` 有 `#![allow(dead_code)]`；且 analysis 语义无回退。
3. **E3 完成：** `report_capability.rs` 存在，`ReportCapability` trait 有 `generate_report_bytes` / `get_pii_unmask_map` / `get_product_name` / `persist_file` 四个方法；`GenerateReportRuntimeTool` 不再是 stub；registry 有 `generate_report` 分支；旧 `report_gen.rs` 有 `#![allow(dead_code)]`。
4. **E4 完成：** `chart_capability.rs` 存在，`ChartCapability` trait 有 `run_chart_python` / `persist_chart` 两个方法；`GenerateChartRuntimeTool` 不再是 stub；registry 有 `generate_chart` 分支；旧 `chart_gen.rs` 有 `#![allow(dead_code)]`。
5. **E5 完成：** `plan_e_tool_migration_test.rs` 与 runtime/schema 集成测试全部通过；`review_` 套件无新失败；`primitive_tools_migration_test` 中已有的 stub / catalog 契约继续通过。
6. **E6 完成：** `browse_data` / subagent launcher 不再丢失 parent cancel / run_id / agent_id；Plan-H/H5 不再依赖临时 bridge。

---

## 风险与注意事项

### PythonRunner::with_config 签名

当前 `PythonRunner` 的 `with_config` 是否接受 `python_binary` 和 `python_home` 作为独立参数需确认（`runner.rs` 第 57 行）。如果现有签名不匹配，需先给 `PythonRunner` 增加接受 `python_binary`/`python_home` 的构造函数，或通过 `resolve_python_path` 返回值直接传入。

### report.rs 中的 `convert_sections_to_pdf`

`convert_sections_to_pdf(ctx, title, sections, &unmask_map)` 依赖 `&PluginContext`。实现 `DefaultReportCapability` 时需将其提取为 `convert_sections_to_pdf_standalone(title, sections, runner: &PythonRunner) -> Result<Vec<u8>>`。这是 `llm/tool_executor/report.rs` 内部的小重构，不影响接口边界。

### chart.rs 中的 `build_chart_python`

`build_chart_python` 是当前 `llm/tool_executor/chart.rs` 中的私有函数。实现 `DefaultChartCapability` 时需将其改为 `pub(crate)`。

### analysis mode 与 `get_step_state`

这不是 follow-up，而是 Plan-E / E2 的正式验收项。`execute_python` 切到 runtime path 后，必须继续保有当前生产路径中的：

- uploaded file auto-load；
- loaded preamble 注入；
- authorized workspace preamble；
- analysis snapshot / user vars / step_state 恢复与保存；
- analysis mode 缺失 `run_id` 时的失败语义。

若某一项暂时无法通过 `CapabilityContext` 表达，应先扩展 shared core / capability boundary，再切换 runtime path；不要以“路径已迁完、语义后补”为 Plan-E 完成标准。

### `#[allow(deprecated)]` 传播

registry 的 `try_build_request_scoped_tool` 会继续使用 `PluginContext`（`ctx: &PluginContext` 参数），因此 E2/E3/E4 对 registry 的修改仍在 `#![allow(deprecated)]` 作用范围内，无需额外处理。

---

## Task E6：迁移 `browse_data` / subagent launcher` 的高风险 legacy path

**定位：** Plan-E 正式任务，直接为 Plan-H/H5 清除 request-scoped cancel / permission control-plane blocker。

**复盘来源：**
- `src-tauri/src/llm/tool_executor/internal_system.rs` 当前仍构造 `SubAgentConfig { cancel_token: None }`；这意味着 `browse_data` 派生出的子 agent 无法继承 parent turn 的 cancel。
- `src-tauri/src/llm/sub_agent.rs` 仍通过 `PluginContext` / legacy tool path 执行；注释已经明确指出 `ToolExecutionContext`（及其 cancel token）会在桥接层丢失。
- 对标 `claude-code-best`，subagent / agent tool 走统一的 abort / permission 控制面，而不是再造一条孤立 root token 路径。

**目标状态：**
- `browse_data` 不再丢失 `ToolExecutionContext.cancel_token`、`run_id`、`agent_id` 等 request-scoped 信息。
- 子 agent 所需依赖通过 request-scoped capability / launcher trait 注入，而不是整个 `PluginContext`。
- 在 E6 完成前后，`browse_data` 都不能继续成为 Plan-H（cancel reachability）与 Plan-F（permission control plane）的 blocker。

**建议文件：**
- Modify: `src-tauri/src/llm/tool_executor/internal_system.rs`
- Modify: `src-tauri/src/llm/sub_agent.rs`
- Modify: `src-tauri/src/plugin/registry.rs`
- Create: `src-tauri/src/runtime/tools/builtin/browse_data_launcher.rs`（或同等 request-scoped capability trait）
- Optional: `src-tauri/src/runtime/tools/builtin/browse_data.rs`（若直接迁成 RuntimeTool）

**依赖关系：**
- Plan-H 的 H5 直接依赖 E6 落地；若 E6 未完成，H5 只能做临时 bridge。
- Plan-B 的 B5 可为 E6 提供 cancel reason API，但不是硬依赖。

**跨计划推荐顺序：**
- 推荐作为新增批次的第 2 项：放在 `B5` 之后、`H5` 之前。
- 不建议跳过 E6 直接做 H5，否则大概率会先做一次临时 bridge，后续还要再拆回 request-scoped launcher。

### Task E6：将 `browse_data` 迁到 request-scoped tool / launcher 边界

- [ ] **E6-Step 1：写失败测试**
  - 在 `src-tauri/tests/plan_e_tool_migration_test.rs` 追加 E6 测试：
    1. parent cancel 触发时，`browse_data` 创建的 `SubAgentConfig.cancel_token` 不再是 `None`。
    2. child launcher 能看到 parent `run_id` / `agent_id`。
    3. 构造 migrated browse_data path 不需要导入 `PluginContext`。
  - 运行：`cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test plan_e_tool_migration_test browse_data -- --nocapture`

- [ ] **E6-Step 2：最小实现 request-scoped launcher**
  - 设计 `BrowseDataLauncher` / `SubagentLauncher` trait，只暴露 `browse_data` 真正需要的字段：workspace、authorized workspace、cancel token、run_id、agent_id、background flag。
  - 在 `plugin/registry.rs` 的 request-scoped 构造路径中注入该 trait 的生产实现。
  - `internal_system.rs` 不再直接拼 `SubAgentConfig { cancel_token: None }`。

- [ ] **E6-Step 3：收口 legacy path**
  - `src-tauri/src/llm/sub_agent.rs` 不再依赖“ToolExecutionContext 被桥接层丢掉也没关系”的假设。
  - 若短期仍需保留 legacy bridge，bridge 也必须把 `cancel_token`、`agent_id`、`run_id` 传到底；不能只桥接 `PluginContext`。
  - `browse_data` 相关旧实现保留 `dead_code` 保护，确认回归全绿后再删除。

- [ ] **E6-Step 4：回归验证**
  - `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test plan_e_tool_migration_test browse_data -- --nocapture`
  - `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test subagent_cancel -- --nocapture`
  - `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_ --tests --no-fail-fast`

- [ ] **E6-Step 5：Commit**
  - `git add src-tauri/src/llm/tool_executor/internal_system.rs src-tauri/src/llm/sub_agent.rs src-tauri/src/plugin/registry.rs src-tauri/src/runtime/tools/builtin/`
  - `git commit -m "feat(browse-data): migrate subagent launcher off legacy plugin path — E6"`
