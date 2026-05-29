# Python Analysis Runtime Parity 计划（Plan-J）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 补齐 `ExecutePythonRuntimeTool` 在 analysis 模式下的 Python binary 解析缺口——当前 `app_handle: None` 导致 auto-load parse 路径退化为系统 Python（忽略 bundled runtime）。

**Architecture:**
`ExecutePythonRuntimeTool::with_runtime_deps()` 新增 `python_binary/python_home` 参数（已在 `DefaultPythonExecution` 中），将 `ExecutePythonCoreParams.app_handle` 替换为 `python_binary_path: Option<(PathBuf, Option<PathBuf>)>`，让 `handle_load_file_core` 直接接收预解析路径，消除对 `app_handle` 的传递依赖。registry 构建时在已有 `resolve_python_path(ctx.app_handle)` 调用基础上直接复用。

**Tech Stack:** Rust, async_trait, tokio

**执行分支：** `pzc`（不创建 worktree，不切分支）

---

## 背景与问题定位

### 当前调用链

```
registry.rs: try_build_request_scoped_tool("execute_python")
  → resolve_python_path(ctx.app_handle)       ← 正确解析 bundled Python
  → ExecutePythonRuntimeTool::with_runtime_deps(python, storage, file_manager, run_id, model)
                                               ← 未传 python_binary

execute() → ExecutePythonCoreParams { app_handle: None, ... }
                                               ← ❌ None 丢失了 bundled Python 路径

handle_execute_python_core (analysis mode):
  → auto-load → handle_load_file_core → PythonRunner::with_config(ctx.app_handle)
                                               ← app_handle=None 退化为系统 Python
```

### 目标调用链

```
registry.rs:
  → resolve_python_path(ctx.app_handle) → (binary, home)
  → ExecutePythonRuntimeTool::with_runtime_deps(python, storage, file_manager, run_id, model,
                                                python_binary, python_home)

execute() → ExecutePythonCoreParams {
    python_binary: Some(binary),
    python_home: Some(home),
    app_handle: None,   // 完全移除 app_handle 依赖
}

handle_execute_python_core (analysis mode auto-load):
  → LoadFileParams { python_binary, python_home, ... }
  → PythonRunner::with_config_from_path(binary, home, workspace, sandbox)
```

---

## 文件地图

| 文件 | 操作 | 职责 |
|------|------|------|
| `src-tauri/src/llm/tool_executor/python.rs` | Modify | `ExecutePythonCoreParams` 新增 `python_binary/python_home`，移除 `app_handle` |
| `src-tauri/src/llm/tool_executor/file_load.rs` | Modify | `LoadFileParams` 新增 `python_binary/python_home`，parse 路径用预解析路径构造 `PythonRunner` |
| `src-tauri/src/python/runner.rs` | Modify | 新增 `with_config_from_path(binary, home, workspace, sandbox)` 构造器 |
| `src-tauri/src/runtime/tools/builtin/python.rs` | Modify | `ExecutePythonRuntimeTool` struct 新增 `python_binary/python_home`，`with_runtime_deps` 新增参数，`execute()` 透传 |
| `src-tauri/src/plugin/registry.rs` | Modify | 构建 `ExecutePythonRuntimeTool` 时传入已解析的 `python_binary/python_home` |
| `src-tauri/src/llm/tool_executor/mod.rs` | 可能 Modify | 确认 `ExecutePythonCoreParams` re-export 不需要调整 |
| `src-tauri/tests/plan_j_python_analysis_parity_test.rs` | Create | 验证 RuntimeTool 路径在 analysis 模式下使用正确 Python binary |

---

## Task J1：PythonRunner 新增 `with_config_from_path` 构造器

**Files:**
- Modify: `src-tauri/src/python/runner.rs`

- [ ] **Step J1-1: 写失败测试**

文件：`src-tauri/tests/plan_j_python_analysis_parity_test.rs`（新建）

```rust
#[cfg(test)]
mod plan_j_python_analysis_parity_tests {
    use std::path::PathBuf;
    use lotus_app::python::runner::PythonRunner;
    use lotus_app::python::sandbox::SandboxConfig;

    #[test]
    fn runner_with_config_from_path_uses_provided_binary() {
        let workspace = std::env::temp_dir().join("test_workspace_j1");
        std::fs::create_dir_all(&workspace).ok();
        let sandbox = SandboxConfig::for_workspace(&workspace);
        let binary = PathBuf::from("/usr/bin/python3");
        let home: Option<PathBuf> = None;

        let runner = PythonRunner::with_config_from_path(binary.clone(), home, workspace, sandbox);
        assert_eq!(runner.python_binary(), &binary);
    }
}
```

- [ ] **Step J1-2: 运行测试确认失败**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test plan_j_python_analysis_parity_test -- --nocapture 2>&1 | head -30
```

Expected: FAIL，报 `with_config_from_path` 不存在或 `python_binary()` 不存在。

- [ ] **Step J1-3: 在 PythonRunner 新增构造器和 accessor**

读取 `src-tauri/src/python/runner.rs`，找到已有的 `PythonRunner` struct 定义（有 `python_binary: PathBuf` 字段），在 `with_config` 方法后新增：

```rust
/// Creates a [`PythonRunner`] from an already-resolved Python binary path.
///
/// Used by the RuntimeTool path where `app_handle` is unavailable but the
/// binary has already been resolved during tool construction.
pub fn with_config_from_path(
    python_binary: PathBuf,
    python_home: Option<PathBuf>,
    workspace_path: PathBuf,
    sandbox: SandboxConfig,
) -> Self {
    Self {
        python_binary,
        python_home,
        workspace_path,
        sandbox,
    }
}

/// Returns the Python binary path this runner will use.
#[cfg(test)]
pub fn python_binary(&self) -> &PathBuf {
    &self.python_binary
}
```

- [ ] **Step J1-4: 运行测试确认通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test runner_with_config_from_path_uses_provided_binary -- --nocapture
```

Expected: PASS。

- [ ] **Step J1-5: Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add src-tauri/src/python/runner.rs src-tauri/tests/plan_j_python_analysis_parity_test.rs
git commit -m "feat(python): add with_config_from_path constructor for runtime tool path — J1"
```

---

## Task J2：`LoadFileParams` 接受预解析 Python binary，不依赖 `app_handle`

**Files:**
- Modify: `src-tauri/src/llm/tool_executor/file_load.rs`

- [ ] **Step J2-1: 写失败测试**

在 `src-tauri/tests/plan_j_python_analysis_parity_test.rs` 末尾追加：

```rust
    #[test]
    fn load_file_params_accepts_python_binary_without_app_handle() {
        use lotus_app::llm::tool_executor::file_load::LoadFileParams;
        use std::path::Path;
        use std::sync::Arc;

        // LoadFileParams 应该有 python_binary + python_home 字段，
        // 不再要求 app_handle 字段。
        let _params = LoadFileParams {
            storage: &Arc::new(lotus_app::storage::file_store::AppStorage::new_test()),
            file_manager: &Arc::new(lotus_app::storage::file_manager::FileManager::new_test()),
            workspace_path: Path::new("/tmp/ws"),
            conversation_id: "conv-test",
            run_id: None,
            python_binary: None,
            python_home: None,
        };
        // If this compiles, the struct has the right fields.
    }
```

- [ ] **Step J2-2: 运行测试确认失败**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test load_file_params_accepts_python_binary -- --nocapture 2>&1 | head -30
```

Expected: FAIL，编译错误——`python_binary` 字段不存在，`app_handle` 字段存在但测试不传。

- [ ] **Step J2-3: 修改 `LoadFileParams` 结构**

读取 `src-tauri/src/llm/tool_executor/file_load.rs`，找到 `LoadFileParams` struct（约第 35-42 行），将：

```rust
pub struct LoadFileParams<'a> {
    pub storage: &'a Arc<AppStorage>,
    pub file_manager: &'a Arc<FileManager>,
    pub workspace_path: &'a Path,
    pub conversation_id: &'a str,
    pub run_id: Option<&'a RunId>,
    pub app_handle: Option<&'a tauri::AppHandle>,
}
```

改为：

```rust
pub struct LoadFileParams<'a> {
    pub storage: &'a Arc<AppStorage>,
    pub file_manager: &'a Arc<FileManager>,
    pub workspace_path: &'a Path,
    pub conversation_id: &'a str,
    pub run_id: Option<&'a RunId>,
    /// Pre-resolved Python binary path. When provided, the runner uses this
    /// instead of calling `resolve_python_path(app_handle)`. This lets the
    /// RuntimeTool path pass a pre-resolved binary without carrying `AppHandle`.
    pub python_binary: Option<std::path::PathBuf>,
    pub python_home: Option<std::path::PathBuf>,
}
```

- [ ] **Step J2-4: 更新 parse 路径，优先使用预解析路径**

在 `handle_load_file_core` 内，找到构建 `PythonRunner` 的代码（约第 617-621 行）：

```rust
let runner = PythonRunner::with_config(
    workspace_pathbuf,
    parse_sandbox,
    ctx.app_handle,
);
```

改为：

```rust
let runner = if let Some(ref binary) = ctx.python_binary {
    PythonRunner::with_config_from_path(
        binary.clone(),
        ctx.python_home.clone(),
        workspace_pathbuf,
        parse_sandbox,
    )
} else {
    PythonRunner::with_config(
        workspace_pathbuf,
        parse_sandbox,
        None, // app_handle not available on runtime tool path
    )
};
```

- [ ] **Step J2-5: 修复所有 `LoadFileParams` 构造调用方**

找到所有构造 `LoadFileParams` 的位置（约有 2 处：legacy `handle_load_file` 和 python.rs 的 auto-load 代码）：

**legacy `handle_load_file`（`file_load.rs` 内的 `PluginContext` 路径）：**
```rust
let load_params = LoadFileParams {
    storage: &ctx.storage,
    file_manager: &ctx.file_manager,
    workspace_path: &ctx.workspace_path,
    conversation_id: &ctx.conversation_id,
    run_id: ctx.run_id.as_ref(),
    python_binary: None, // legacy path: runner will fall back to system python / env
    python_home: None,
};
```

注意：legacy 路径的 `PythonRunner::with_config(workspace, sandbox, None)` 内部会调用 `resolve_python_path(None)`，退化行为与之前一致。

**python.rs auto-load 路径（`handle_execute_python_core` 内）：**
```rust
let load_params = super::file_load::LoadFileParams {
    storage: params.storage,
    file_manager: params.file_manager,
    workspace_path: params.workspace_path,
    conversation_id: params.conversation_id,
    run_id: params.requested_run_id,
    python_binary: params.python_binary.clone(),
    python_home: params.python_home.clone(),
};
```

- [ ] **Step J2-6: 运行测试确认通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test load_file_params_accepts_python_binary -- --nocapture
```

Expected: PASS。

- [ ] **Step J2-7: Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add src-tauri/src/llm/tool_executor/file_load.rs
git commit -m "refactor(file-load): replace app_handle with pre-resolved python_binary in LoadFileParams — J2"
```

---

## Task J3：`ExecutePythonCoreParams` 接受预解析 Python binary，移除 `app_handle`

**Files:**
- Modify: `src-tauri/src/llm/tool_executor/python.rs`

- [ ] **Step J3-1: 写失败测试**

在 `src-tauri/tests/plan_j_python_analysis_parity_test.rs` 末尾追加：

```rust
    #[test]
    fn execute_python_core_params_has_python_binary_not_app_handle() {
        use lotus_app::llm::tool_executor::ExecutePythonCoreParams;
        use std::path::{Path, PathBuf};
        use std::sync::Arc;

        // 验证 ExecutePythonCoreParams 有 python_binary/python_home 而不是 app_handle
        let storage = Arc::new(lotus_app::storage::file_store::AppStorage::new_test());
        let file_manager = Arc::new(lotus_app::storage::file_manager::FileManager::new_test());
        let _params = ExecutePythonCoreParams {
            storage: &storage,
            file_manager: &file_manager,
            workspace_path: Path::new("/tmp"),
            authorized_workspace: None,
            conversation_id: "test",
            requested_run_id: None,
            model: "test-model",
            python_binary: Some(PathBuf::from("/usr/bin/python3")),
            python_home: None,
        };
        // If this compiles, the struct has the right fields.
    }
```

- [ ] **Step J3-2: 运行测试确认失败**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test execute_python_core_params_has_python_binary -- --nocapture 2>&1 | head -30
```

Expected: FAIL，编译错误——`python_binary` 不存在，`app_handle` 字段存在。

- [ ] **Step J3-3: 修改 `ExecutePythonCoreParams`**

读取 `src-tauri/src/llm/tool_executor/python.rs`，找到 `ExecutePythonCoreParams` struct（约第 24-33 行），将：

```rust
pub(crate) struct ExecutePythonCoreParams<'a> {
    pub storage: &'a Arc<AppStorage>,
    pub file_manager: &'a Arc<FileManager>,
    pub workspace_path: &'a Path,
    pub authorized_workspace: Option<&'a AuthorizedWorkspaceRef>,
    pub conversation_id: &'a str,
    pub requested_run_id: Option<&'a RunId>,
    pub model: &'a str,
    pub app_handle: Option<&'a tauri::AppHandle>,
}
```

改为：

```rust
pub(crate) struct ExecutePythonCoreParams<'a> {
    pub storage: &'a Arc<AppStorage>,
    pub file_manager: &'a Arc<FileManager>,
    pub workspace_path: &'a Path,
    pub authorized_workspace: Option<&'a AuthorizedWorkspaceRef>,
    pub conversation_id: &'a str,
    pub requested_run_id: Option<&'a RunId>,
    pub model: &'a str,
    /// Pre-resolved Python binary for auto-load / parse operations.
    /// None on legacy paths — falls back to system Python resolution.
    pub python_binary: Option<std::path::PathBuf>,
    pub python_home: Option<std::path::PathBuf>,
}
```

同时删除 `impl<'a> ExecutePythonCoreParams<'a>` 中不再需要的 `app_handle` 依赖（`loaded_scope_id`/`loaded_key`/`load_failed_key` 不用动）。

- [ ] **Step J3-4: 更新 `handle_execute_python_core` 内 auto-load 调用**

在 `handle_execute_python_core` 内找到 `LoadFileParams` 构造（约第 130-138 行），将 `app_handle: params.app_handle` 改为：

```rust
let load_params = super::file_load::LoadFileParams {
    storage: params.storage,
    file_manager: params.file_manager,
    workspace_path: params.workspace_path,
    conversation_id: params.conversation_id,
    run_id: params.requested_run_id,
    python_binary: params.python_binary.clone(),
    python_home: params.python_home.clone(),
};
```

- [ ] **Step J3-5: 更新 legacy `handle_execute_python` 调用**

找到 `handle_execute_python(ctx: &PluginContext, ...)` 函数（约第 52-66 行），将 `ExecutePythonCoreParams` 构造中的：

```rust
app_handle: ctx.app_handle.as_ref(),
```

改为：

```rust
python_binary: {
    let (binary, _) = crate::python::runner::resolve_python_path(ctx.app_handle.as_ref());
    Some(binary)
},
python_home: {
    let (_, home) = crate::python::runner::resolve_python_path(ctx.app_handle.as_ref());
    home
},
```

注意：`resolve_python_path` 被调用两次会有性能损耗，改为先解构：

```rust
let (python_binary, python_home) =
    crate::python::runner::resolve_python_path(ctx.app_handle.as_ref());
let params = ExecutePythonCoreParams {
    storage: &ctx.storage,
    file_manager: &ctx.file_manager,
    workspace_path: &ctx.workspace_path,
    authorized_workspace: ctx.authorized_workspace.as_ref(),
    conversation_id: &ctx.conversation_id,
    requested_run_id: ctx.run_id.as_ref(),
    model: &ctx.model,
    python_binary: Some(python_binary),
    python_home,
};
```

- [ ] **Step J3-6: 运行测试确认通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test execute_python_core_params_has_python_binary -- --nocapture
```

Expected: PASS。

- [ ] **Step J3-7: Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add src-tauri/src/llm/tool_executor/python.rs
git commit -m "refactor(python): replace app_handle with pre-resolved binary in ExecutePythonCoreParams — J3"
```

---

## Task J4：`ExecutePythonRuntimeTool` 传入预解析 Python binary

**Files:**
- Modify: `src-tauri/src/runtime/tools/builtin/python.rs`
- Modify: `src-tauri/src/plugin/registry.rs`

- [ ] **Step J4-1: 写失败测试**

在 `src-tauri/tests/plan_j_python_analysis_parity_test.rs` 末尾追加：

```rust
    #[test]
    fn execute_python_runtime_tool_exposes_python_binary() {
        use std::path::PathBuf;
        use lotus_app::runtime::tools::builtin::python::ExecutePythonRuntimeTool;
        use lotus_app::runtime::tools::builtin::python_execution::DefaultPythonExecution;
        use lotus_app::python::session::PythonSessionManager;
        use std::sync::Arc;

        let session_manager = Arc::new(PythonSessionManager::new());
        let binary = PathBuf::from("/usr/bin/python3");
        let home: Option<PathBuf> = None;
        let python = Arc::new(DefaultPythonExecution::new(
            session_manager,
            binary.clone(),
            home.clone(),
        ));
        let storage = Arc::new(lotus_app::storage::file_store::AppStorage::new_test());
        let file_manager = Arc::new(lotus_app::storage::file_manager::FileManager::new_test());

        let tool = ExecutePythonRuntimeTool::with_runtime_deps(
            python, storage, file_manager, None, "test-model".to_string(),
            binary.clone(), None,
        );

        // The tool should carry the binary path so execute() can pass it through.
        assert_eq!(tool.python_binary_path(), Some(&binary));
    }
```

- [ ] **Step J4-2: 运行测试确认失败**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test execute_python_runtime_tool_exposes_python_binary -- --nocapture 2>&1 | head -30
```

Expected: FAIL。

- [ ] **Step J4-3: 修改 `ExecutePythonRuntimeTool`**

读取 `src-tauri/src/runtime/tools/builtin/python.rs`，修改 struct 和 constructor：

```rust
pub struct ExecutePythonRuntimeTool {
    stub_mode: bool,
    python: Option<Arc<dyn PythonExecution>>,
    storage: Option<Arc<AppStorage>>,
    file_manager: Option<Arc<FileManager>>,
    requested_run_id: Option<RunId>,
    model: Option<String>,
    /// Pre-resolved Python binary for auto-load / parse operations.
    python_binary: Option<std::path::PathBuf>,
    python_home: Option<std::path::PathBuf>,
}

impl ExecutePythonRuntimeTool {
    pub fn stub() -> Self {
        Self {
            stub_mode: true,
            python: None,
            storage: None,
            file_manager: None,
            requested_run_id: None,
            model: None,
            python_binary: None,
            python_home: None,
        }
    }

    pub fn with_runtime_deps(
        python: Arc<dyn PythonExecution>,
        storage: Arc<AppStorage>,
        file_manager: Arc<FileManager>,
        requested_run_id: Option<RunId>,
        model: String,
        python_binary: std::path::PathBuf,
        python_home: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            stub_mode: false,
            python: Some(python),
            storage: Some(storage),
            file_manager: Some(file_manager),
            requested_run_id,
            model: Some(model),
            python_binary: Some(python_binary),
            python_home,
        }
    }

    #[cfg(test)]
    pub fn python_binary_path(&self) -> Option<&std::path::PathBuf> {
        self.python_binary.as_ref()
    }
}
```

- [ ] **Step J4-4: 在 `execute()` 方法中传入 python_binary**

在 `ExecutePythonRuntimeTool::execute()` 内，修改 `ExecutePythonCoreParams` 构造：

```rust
let params = crate::llm::tool_executor::ExecutePythonCoreParams {
    storage,
    file_manager,
    workspace_path: &storage_cap.workspace_path,
    authorized_workspace: storage_cap.authorized_workspace.as_ref(),
    conversation_id: ctx.session_id.as_str(),
    requested_run_id: self.requested_run_id.as_ref(),
    model: self.model.as_deref().unwrap_or("unknown"),
    python_binary: self.python_binary.clone(),
    python_home: self.python_home.clone(),
};
```

- [ ] **Step J4-5: 修复 registry 构建调用，传入 python_binary**

读取 `src-tauri/src/plugin/registry.rs`，找到 `"execute_python"` 分支（约第 672-691 行），将：

```rust
Some(Arc::new(
    builtin::python::ExecutePythonRuntimeTool::with_runtime_deps(
        python,
        ctx.storage.clone(),
        ctx.file_manager.clone(),
        ctx.run_id.clone(),
        ctx.model.clone(),
    ),
) as Arc<dyn crate::runtime::tools::RuntimeTool>)
```

改为：

```rust
Some(Arc::new(
    builtin::python::ExecutePythonRuntimeTool::with_runtime_deps(
        python,
        ctx.storage.clone(),
        ctx.file_manager.clone(),
        ctx.run_id.clone(),
        ctx.model.clone(),
        python_binary,  // already resolved above via resolve_python_path
        python_home,
    ),
) as Arc<dyn crate::runtime::tools::RuntimeTool>)
```

注意：当前 registry 代码在 `execute_python` 分支中已经调用了 `let (python_binary, python_home) = crate::python::runner::resolve_python_path(ctx.app_handle.as_ref());`，只需追加两个参数。

- [ ] **Step J4-6: 运行测试确认通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test execute_python_runtime_tool_exposes_python_binary -- --nocapture
```

Expected: PASS。

- [ ] **Step J4-7: 运行全量回归**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_ --tests --no-fail-fast
```

Expected: 所有 `review_` 测试通过。

- [ ] **Step J4-8: Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add src-tauri/src/runtime/tools/builtin/python.rs src-tauri/src/plugin/registry.rs
git commit -m "feat(python): thread pre-resolved python binary through RuntimeTool execute path — J4"
```

---

## Task J5：端到端验证 + `review_` 约束固化

**Files:**
- Modify: `src-tauri/tests/plan_j_python_analysis_parity_test.rs`

- [ ] **Step J5-1: 写 review 约束测试**

在 `src-tauri/tests/plan_j_python_analysis_parity_test.rs` 末尾追加：

```rust
    /// J5: 架构约束 — ExecutePythonRuntimeTool::execute() 不再使用 app_handle=None 占位符
    #[test]
    fn review_execute_python_runtime_tool_no_app_handle_passthrough() {
        let source = std::fs::read_to_string(
            "src/runtime/tools/builtin/python.rs"
        ).expect("file must exist");

        // RuntimeTool 路径不应再出现 app_handle: None 硬编码占位
        assert!(
            !source.contains("app_handle: None"),
            "ExecutePythonRuntimeTool must not pass app_handle: None to core params"
        );
    }

    /// J5: 架构约束 — LoadFileParams 有 python_binary 字段而不是 app_handle
    #[test]
    fn review_load_file_params_has_python_binary_not_app_handle() {
        let source = std::fs::read_to_string(
            "src/llm/tool_executor/file_load.rs"
        ).expect("file must exist");

        assert!(
            source.contains("pub python_binary: Option"),
            "LoadFileParams must have python_binary field"
        );
        assert!(
            !source.contains("pub app_handle"),
            "LoadFileParams must not have app_handle field (replaced by python_binary)"
        );
    }

    /// J5: 架构约束 — ExecutePythonCoreParams 有 python_binary 字段而不是 app_handle
    #[test]
    fn review_execute_python_core_params_has_python_binary_not_app_handle() {
        let source = std::fs::read_to_string(
            "src/llm/tool_executor/python.rs"
        ).expect("file must exist");

        assert!(
            source.contains("pub python_binary: Option"),
            "ExecutePythonCoreParams must have python_binary field"
        );
        assert!(
            !source.contains("pub app_handle"),
            "ExecutePythonCoreParams must not have app_handle field"
        );
    }
```

- [ ] **Step J5-2: 运行约束测试**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_execute_python_runtime_tool_no_app_handle -- --nocapture && cargo test review_load_file_params_has_python_binary -- --nocapture && cargo test review_execute_python_core_params_has_python_binary -- --nocapture
```

Expected: 三个 `review_` 测试全部 PASS。

- [ ] **Step J5-3: 全量回归**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_ --tests --no-fail-fast && cargo test --tests --no-fail-fast 2>&1 | grep -E "^FAILED|^error\[" | head -20
```

Expected: `review_` 全绿，grep 无输出。

- [ ] **Step J5-4: Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add src-tauri/tests/plan_j_python_analysis_parity_test.rs
git commit -m "test(python): add architecture constraints for python binary runtime parity — J5"
```

---

## 完成标准（Definition of Done）

- `ExecutePythonRuntimeTool::execute()` 在 analysis 模式下不再传 `app_handle: None`；auto-load parse 使用与 legacy 路径相同的 bundled Python binary
- `LoadFileParams` 和 `ExecutePythonCoreParams` 无 `app_handle` 字段
- 3 个 `review_` 架构约束测试通过
- `cargo test review_ --tests --no-fail-fast` 全绿
- `cargo test --tests --no-fail-fast` 无新增 FAILED

---

## 推荐执行顺序

J1 → J2 → J3 → J4 → J5

原因：J1 先在 runner 补构造器；J2 改 LoadFileParams（下游）；J3 改 ExecutePythonCoreParams（中游）；J4 改 RuntimeTool + registry（上游注入）；J5 固化 review 约束。逆向改会导致编译错误累积。
