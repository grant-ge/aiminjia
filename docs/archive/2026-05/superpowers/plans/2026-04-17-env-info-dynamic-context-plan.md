# Env Info Dynamic Context Injection — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 LLM 在每轮对话中都能看到当前工作目录、授权目录、git 状态、平台信息，对齐 claude-code-best 的 `computeSimpleEnvInfo` 设计。

**Architecture:** 新增 `build_env_info()` 函数写入 `context_builder.rs`，通过 `RuntimeLlmExecutor` trait 新增的 `get_env_info()` 方法，在 `run_chat_turn_s4` 的迭代循环中拼入 `dynamic_context`。同时修复 legacy 路径的 `build_workspace_context` 以支持无授权目录时 fallback 到普通 workspace 路径。

**Tech Stack:** Rust, Tauri v2, std::process::Command (git), sys-info / std::env

---

## File Structure

| 文件 | 改什么 |
|---|---|
| `src-tauri/src/runtime/chat/context_builder.rs` | 新增 `build_env_info(workspace_path, authorized_workspace)` 函数 |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | `RuntimeLlmExecutor` trait 新增 `get_env_info()` 默认方法；`run_chat_turn_s4` 调用它并传入 `build_iteration_context` |
| `src-tauri/src/transport/tauri_commands/chat.rs` | `TauriLegacyTurnExecutor` 实现 `get_env_info()` |
| `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` | `build_workspace_context` 新增 fallback：无 authorized_workspace 时用 workspace_path |

---

## Task 1: context_builder.rs — 新增 `build_env_info()` 函数

**Files:**
- Modify: `src-tauri/src/runtime/chat/context_builder.rs`

---

- [ ] **Step 1: 写测试（先红）**

在 `context_builder.rs` 的 `mod tests` 末尾追加：

```rust
#[test]
fn test_build_env_info_with_authorized_workspace() {
    let workspace_path = std::path::PathBuf::from("/tmp/test-workspace");
    let authorized = Some(("/tmp/test-workspace/my-project".to_string(), "我的项目".to_string()));
    let result = build_env_info(&workspace_path, authorized.as_ref().map(|(p, n)| (p.as_str(), n.as_str())));
    assert!(result.contains("[当前环境]"), "must have env section header");
    assert!(result.contains("已连接目录"), "must mention authorized dir");
    assert!(result.contains("my-project") || result.contains("我的项目"), "must include dir name");
    assert!(result.contains("Platform:"), "must include platform");
}

#[test]
fn test_build_env_info_without_authorized_workspace() {
    let workspace_path = std::path::PathBuf::from("/tmp/test-workspace");
    let result = build_env_info(&workspace_path, None);
    assert!(result.contains("[当前环境]"), "must have env section header");
    assert!(result.contains("工作目录"), "must include working dir");
    assert!(result.contains("Platform:"), "must include platform");
    assert!(!result.contains("已连接目录"), "must NOT mention authorized dir when absent");
}

#[test]
fn test_build_env_info_platform_info() {
    let workspace_path = std::path::PathBuf::from("/tmp");
    let result = build_env_info(&workspace_path, None);
    // 至少包含 darwin / windows / linux 中的一个
    let has_platform = result.contains("darwin") || result.contains("windows") || result.contains("linux");
    assert!(has_platform, "must include OS type, got: {}", result);
}
```

- [ ] **Step 2: 验证测试失败**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test build_env_info -- --nocapture 2>&1 | grep -E "error\[E\]|FAILED" | head -10
```

预期：`build_env_info` 未定义，编译错误。

- [ ] **Step 3: 实现 `build_env_info()`**

在 `context_builder.rs` 的 `build_iteration_context` 函数之后（`#[cfg(test)]` 之前）插入：

```rust
/// 构建会话级环境信息段落，注入到 dynamic context。
///
/// 对齐 claude-code-best 的 `computeSimpleEnvInfo`：
/// - 当前工作目录 / 已授权目录
/// - git 状态摘要（失败时静默跳过）
/// - 操作系统平台
///
/// `authorized` = `Some((root_path_str, display_name))` 当用户已连接本地目录时。
pub fn build_env_info(
    workspace_path: &std::path::PathBuf,
    authorized: Option<(&str, &str)>,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    // 1. 工作目录 / 已授权目录
    match authorized {
        Some((root_path, display_name)) => {
            parts.push(format!(
                "已连接目录: {} ({})",
                display_name, root_path
            ));
        }
        None => {
            parts.push(format!(
                "工作目录: {}",
                workspace_path.display()
            ));
        }
    }

    // 2. Git 状态（静默失败）
    let effective_path = authorized
        .map(|(p, _)| std::path::PathBuf::from(p))
        .unwrap_or_else(|| workspace_path.clone());

    if let Ok(output) = std::process::Command::new("git")
        .args(["-C", &effective_path.to_string_lossy(), "status", "--short", "--branch"])
        .output()
    {
        if output.status.success() {
            let status_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !status_str.is_empty() {
                // 截取前 10 行避免超长
                let lines: Vec<&str> = status_str.lines().take(10).collect();
                parts.push(format!("Git: {}", lines.join(" | ")));
            }
        }
    }

    // 3. 平台信息
    let platform = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    };
    parts.push(format!("Platform: {}", platform));

    format!("\n\n[当前环境]\n{}", parts.join("\n"))
}
```

- [ ] **Step 4: 验证测试通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test build_env_info -- --nocapture 2>&1 | tail -10
```

预期：3 个测试全部 ok。

- [ ] **Step 5: commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri
git add src/runtime/chat/context_builder.rs
git commit -m "feat(context-builder): 新增 build_env_info() — 工作目录 / git 状态 / 平台信息"
```

---

## Task 2: RuntimeLlmExecutor trait 新增 `get_env_info()` + `run_chat_turn_s4` 接入

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Test: `src-tauri/tests/s4_driver_loop_test.rs`

---

- [ ] **Step 1: 写测试（先红）**

在 `s4_driver_loop_test.rs` 末尾追加：

```rust
struct EnvInfoCapturingExecutor {
    env_info: String,
    captured_dynamic_contexts: std::sync::Mutex<Vec<String>>,
}

impl EnvInfoCapturingExecutor {
    fn new(env_info: impl Into<String>) -> Self {
        Self {
            env_info: env_info.into(),
            captured_dynamic_contexts: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl RuntimeLlmExecutor for EnvInfoCapturingExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        self.captured_dynamic_contexts
            .lock()
            .unwrap()
            .push(input.dynamic_context.to_string());
        Ok(LlmStepResult::ContentComplete {
            content: "ok".to_string(),
            tokens_in: 0,
            tokens_out: 0,
        })
    }

    async fn get_env_info(&self) -> Result<String, TurnError> {
        Ok(self.env_info.clone())
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("mock-id".to_string())
    }
}

#[tokio::test]
async fn driver_s4_env_info_appears_in_dynamic_context() {
    let executor = Arc::new(EnvInfoCapturingExecutor::new("\n\n[当前环境]\n工作目录: /tmp/test\nPlatform: darwin"));
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-env-info");
    let request = ChatTurnRequest::new("conv-env-info", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let captured = executor.captured_dynamic_contexts.lock().unwrap();
    assert!(!captured.is_empty(), "must have captured dynamic_context");
    assert!(
        captured[0].contains("[当前环境]"),
        "dynamic_context must contain env info, got: {}",
        captured[0]
    );
    assert!(
        captured[0].contains("工作目录: /tmp/test"),
        "dynamic_context must contain working dir, got: {}",
        captured[0]
    );
}

#[tokio::test]
async fn driver_s4_empty_env_info_does_not_break_context() {
    // get_env_info 返回空时 dynamic_context 仍然正常工作
    let executor = Arc::new(EnvInfoCapturingExecutor::new(""));
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-env-info-empty");
    let request = ChatTurnRequest::new("conv-env-info-empty", "hello", vec![]);

    let result = driver.run_chat_turn(&mut turn, &request).await;
    assert!(result.is_ok(), "must work when env_info is empty");
}
```

- [ ] **Step 2: 验证测试失败**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test driver_s4_env_info -- --nocapture 2>&1 | grep -E "error\[E\]|FAILED" | head -10
```

预期：`get_env_info` 在 trait 中未定义，编译错误。

- [ ] **Step 3: `RuntimeLlmExecutor` trait 新增 `get_env_info()` 方法**

在 `chat_turn_driver.rs` 的 `RuntimeLlmExecutor` trait 中，在 `load_history` 方法之后插入：

```rust
/// 返回会话级环境信息字符串（工作目录、git 状态、平台）。
///
/// 返回值将被注入到每次 iteration 的 dynamic_context 中。
/// 默认实现返回空字符串（向后兼容旧 mock executor）。
/// 生产 executor 必须 override。
async fn get_env_info(&self) -> Result<String, TurnError> {
    Ok(String::new())
}
```

- [ ] **Step 4: `run_chat_turn_s4` 调用 `get_env_info()` 并拼入 dynamic_context**

在 `run_chat_turn_s4` 函数里，找到以下代码段（**在 TurnConfig 构建之后、迭代循环之前**）：

```rust
// ── Step 5: Iteration loop ────────────────────────────────────────────
let round_driver = ToolRoundDriver::new(self.query_engine.clone());

'turn: for iteration in 0..config.max_iterations {
    // Build a dynamic context string for this iteration.
    // Currently minimal; T14 will wire the full context_builder call.
    let dynamic_context = precompute_result.as_deref().unwrap_or_default().to_string();
```

在 `'turn:` 循环**之前**（在 `let round_driver = ...` 之后）插入一次性 env_info 获取：

```rust
// 获取会话级环境信息（只需获取一次，整个 turn 内不变）
let env_info = executor
    .get_env_info()
    .await
    .unwrap_or_else(|e| {
        log::warn!("[run_chat_turn_s4] get_env_info failed: {}", e);
        String::new()
    });
```

然后将循环内的 `dynamic_context` 构建行替换为：

```rust
'turn: for iteration in 0..config.max_iterations {
    // Build a dynamic context string for this iteration.
    let precompute_ctx = precompute_result.as_deref().unwrap_or_default();
    let dynamic_context = if env_info.is_empty() {
        precompute_ctx.to_string()
    } else if precompute_ctx.is_empty() {
        env_info.clone()
    } else {
        format!("{}\n\n{}", env_info, precompute_ctx)
    };
```

- [ ] **Step 5: 验证测试通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test driver_s4_env_info -- --nocapture 2>&1 | tail -10
```

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test driver_s4 -- --nocapture 2>&1 | tail -10
```

预期：所有 driver_s4 测试通过。

- [ ] **Step 6: commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri
git add src/runtime/chat/chat_turn_driver.rs tests/s4_driver_loop_test.rs
git commit -m "feat(chat-driver): get_env_info() trait 方法 + S4 路径 dynamic_context 注入 env_info"
```

---

## Task 3: TauriLegacyTurnExecutor 实现 `get_env_info()` + legacy 路径 `build_workspace_context` fallback

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`
- Test: `src-tauri/tests/s4_driver_loop_test.rs`（更新 mock 签名）

> **注意：** 本 Task 同时完成 `get_env_info` 实现（含 `conversation_id` 参数）和 legacy fallback，避免 Task 3/4 拆分后出现签名不一致的中间状态。

---

- [ ] **Step 1: 验证当前编译通过（trait 默认实现兜底）**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo build 2>&1 | grep "^error" | head -5
```

预期：0 个错误（使用 trait 默认实现）。

- [ ] **Step 2: 修改 `RuntimeLlmExecutor` trait `get_env_info` 签名加入 `conversation_id`**

在 `chat_turn_driver.rs` 中，找到 Task 2 添加的：

```rust
async fn get_env_info(&self) -> Result<String, TurnError> {
    Ok(String::new())
}
```

改为：

```rust
async fn get_env_info(&self, conversation_id: &str) -> Result<String, TurnError> {
    let _ = conversation_id;
    Ok(String::new())
}
```

在 `run_chat_turn_s4` 中将调用改为：

```rust
let env_info = executor
    .get_env_info(&request.conversation_id)
    .await
    .unwrap_or_else(|e| {
        log::warn!("[run_chat_turn_s4] get_env_info failed: {}", e);
        String::new()
    });
```

同时更新测试文件 `s4_driver_loop_test.rs` 中 `EnvInfoCapturingExecutor` 的签名：

```rust
async fn get_env_info(&self, _conversation_id: &str) -> Result<String, TurnError> {
    Ok(self.env_info.clone())
}
```

- [ ] **Step 3: 实现 `TauriLegacyTurnExecutor::get_env_info()`**

在 `chat.rs` 的 `impl RuntimeLlmExecutor for TauriLegacyTurnExecutor` 中，在 `load_history` 实现之后添加：

```rust
async fn get_env_info(&self, conversation_id: &str) -> Result<String, TurnError> {
    use crate::runtime::chat::context_builder::build_env_info;
    use crate::transport::tauri_commands::chat::chat_runtime_impl::load_authorized_workspace;

    // 先 Read chat.rs 确认 file_mgr 的实际类型，以下假设有 workspace_path() 方法
    // 若不存在，改用 std::env::current_dir().unwrap_or(PathBuf::from("."))
    let workspace_path = self.services.file_mgr.workspace_path().to_path_buf();

    // 加载已授权目录（失败时静默降级到纯 workspace_path）
    let authorized = load_authorized_workspace(&self.services.app, conversation_id);
    let authorized_tuple = authorized.as_ref().map(|aw| {
        (aw.root_path.to_string_lossy().into_owned(), aw.display_name.clone())
    });
    let authorized_ref = authorized_tuple.as_ref().map(|(p, n)| (p.as_str(), n.as_str()));

    let env_info = build_env_info(&workspace_path, authorized_ref);

    log::info!(
        "[get_env_info] conv={} workspace={} authorized={} env_info_len={}",
        conversation_id,
        workspace_path.display(),
        authorized.is_some(),
        env_info.len()
    );

    Ok(env_info)
}
```

- [ ] **Step 4: 修复 legacy 路径 `build_workspace_context` 支持 fallback**

在 `chat_runtime_impl.rs` 中，找到 `build_workspace_context` 函数（第 79-91 行）：

```rust
fn build_workspace_context(
    authorized_workspace: Option<&crate::runtime::store::AuthorizedWorkspaceRef>,
) -> String {
    let Some(authorized_workspace) = authorized_workspace else {
        return String::new();
    };

    format!(
        "\n\n[已连接本地目录]\n- 名称: {}\n- 根目录: {}\n- 当前会话可以直接读取这个目录，不需要先上传或复制文件。\n- 处理本地目录时，优先使用 list_directory / read_workspace_file / search_files / get_file_info。\n- 只有处理用户上传的附件时，才使用 load_file(file_id)。\n- 如果需要进一步计算或生成产物，再结合 execute_python。",
        authorized_workspace.display_name,
        authorized_workspace.root_path.display()
    )
}
```

替换为：

```rust
fn build_workspace_context(
    authorized_workspace: Option<&crate::runtime::store::AuthorizedWorkspaceRef>,
    fallback_workspace_path: Option<&std::path::Path>,
) -> String {
    if let Some(aw) = authorized_workspace {
        return format!(
            "\n\n[已连接本地目录]\n- 名称: {}\n- 根目录: {}\n- 当前会话可以直接读取这个目录，不需要先上传或复制文件。\n- 处理本地目录时，优先使用 list_directory / read_workspace_file / search_files / get_file_info。\n- 只有处理用户上传的附件时，才使用 load_file(file_id)。\n- 如果需要进一步计算或生成产物，再结合 execute_python。",
            aw.display_name,
            aw.root_path.display()
        );
    }

    if let Some(wp) = fallback_workspace_path {
        return format!(
            "\n\n[工作目录]\n- 路径: {}\n- 处理文件时使用 list_directory / read_workspace_file / search_files。",
            wp.display()
        );
    }

    String::new()
}
```

找到第 1686 行的调用处：

```rust
let workspace_context = build_workspace_context(authorized_workspace.as_ref());
```

改为：

```rust
let workspace_context = build_workspace_context(
    authorized_workspace.as_ref(),
    Some(workspace_path.as_path()),
);
```

- [ ] **Step 5: 验证编译 + 全量测试 + 架构约束测试**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo build 2>&1 | grep "^error" | head -10
```

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test -- 2>&1 | tail -5
```

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -10
```

预期：全量测试通过，架构约束无违反。

- [ ] **Step 6: commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri
git add src/transport/tauri_commands/chat.rs \
        src/transport/tauri_commands/chat/chat_runtime_impl.rs \
        src/runtime/chat/chat_turn_driver.rs \
        tests/s4_driver_loop_test.rs
git commit -m "feat(env-info): TauriLegacyTurnExecutor 实现 get_env_info + legacy fallback

get_env_info(conversation_id) 加载 authorized_workspace + workspace_path + git 状态 + 平台
build_workspace_context 新增 fallback_workspace_path 参数，无授权目录时也注入工作目录
legacy 路径和 S4 路径均能向 LLM 提供工作目录信息"
```

---

## Self-Review

### Spec 覆盖度检查

| 目标 | 覆盖 Task |
|---|---|
| `build_env_info()` 函数（工作目录 + git 状态 + 平台） | Task 1 |
| S4 路径动态注入 env_info 到 dynamic_context | Task 2 |
| 生产 executor 实现（workspace_path fallback） | Task 3 |
| authorized_workspace 接入 + legacy fallback 修复 | Task 4 |

### Placeholder 扫描

无 TBD/TODO。Task 3 Step 2 明确说明了若 `workspace_path()` 不存在的 fallback。

### 类型一致性

- Task 4 修改了 `get_env_info` 签名（加 `conversation_id: &str`），Task 2 的测试 mock 也需要同步更新——Task 4 Step 2 明确写了这一点。
- `build_workspace_context` 新增 `fallback_workspace_path: Option<&Path>` 参数，调用处（第 1686 行）同步更新。
- `build_env_info` 的 `authorized` 参数类型 `Option<(&str, &str)>` 在 Task 3/4 中通过 `authorized_tuple` 中间变量保持生命周期安全。

### 任务顺序说明

- Task 3 先实现不带 conversation_id 的版本，Task 4 再修改签名补全——两个 task 改同一函数是有意的，Task 3 让功能先跑起来（workspace 路径），Task 4 再补 authorized_workspace 完整链路。**执行时 Task 3 和 Task 4 之间必须顺序执行。**
