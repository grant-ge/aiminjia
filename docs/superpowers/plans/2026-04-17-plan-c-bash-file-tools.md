# bash/file 基础工具集实施计划（Plan-C）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 `write_file`、`edit_file`、`bash`、`grep_content` 四个基础工具，补齐 lotus-app 对标 claude-code-best 的核心工具集缺口。

**Architecture:** 每个工具独立实现为 `RuntimeTool`，在 `catalog.rs` 注册，在 `builtin/` 目录实现。`BashTool` 使用 `tokio::process` 实现异步可取消命令执行。`EditFileTool` 基于 old/new string 替换模式（最简实现，无 diff）。`GrepTool` 使用 `regex` crate 实现递归内容搜索。

**Tech Stack:** Rust, tokio::process, regex, async_trait

**Worktree branch:** `feat/bash-file-tools`

---

## 关键架构约束（每个 Task 都必须遵守）

1. **`builtin/` 模块不得 `use tauri::*`** — 能力通过 `CapabilityContext` 注入
2. **写操作完成后必须更新 `ctx.capability.read_file_state`** — 保持 FileStateCache 一致性（与 `read_workspace_file` 的缓存对齐）
3. **路径解析统一调用 `file_manager::resolve_local_reference(root, rel)`** — 防止路径穿越
4. **`require_workspace_root` 是获取 workspace 根的标准方式** — 复用 `workspace.rs` 已有的实现
5. **工具定义先在 `catalog.rs` 的 `build_default_catalog()` 中注册，再在 `builtin/` 中实现** — catalog 是单一真相源
6. **集成测试一律用 `tempfile::TempDir`** — 隔离文件副作用，不污染真实 workspace

---

## Task 1：WriteFileTool（`write_file`）

### 1.0 工作目录准备

- [ ] 从 `main` 分支创建 `feat/bash-file-tools` worktree，后续所有 Task 在该 worktree 完成

### 1.1 Catalog 注册

在 `src-tauri/src/runtime/tools/catalog.rs` 的 `build_default_catalog()` 函数末尾（在 Primitive workspace tools 区块内），紧接 `get_file_info` 条目后添加：

```rust
c.insert(CatalogEntry::new(
    ToolDefinition::new("write_file", "在授权工作目录中创建或覆盖写入文本文件")
        .with_kind(ToolKind::Primitive)
        .with_capability_scope(["workspace:write"]),
    json!({
        "type": "object",
        "required": ["path", "content"],
        "properties": {
            "path": { "type": "string", "description": "相对于授权工作目录的目标文件路径" },
            "content": { "type": "string", "description": "要写入的文件内容（UTF-8 文本）" }
        }
    }),
));
```

- [ ] 在 `catalog.rs` 中添加以上条目

### 1.2 RuntimeTool 实现

在 `src-tauri/src/runtime/tools/builtin/workspace.rs` 末尾新增 `WriteFileRuntimeTool`：

```rust
// ── WriteFileRuntimeTool ──────────────────────────────────────────────────

pub struct WriteFileRuntimeTool;

#[async_trait]
impl RuntimeTool for WriteFileRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("write_file")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("write_file", "Write workspace file"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let root = require_workspace_root(&ctx)?;
        let rel = input
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: path".into()))?;
        let content = input
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: content".into()))?;

        let resolved = resolve_path(&root, rel)?;

        // 创建父目录（如不存在）
        if let Some(parent) = resolved.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to create dirs: {e}")))?;
        }

        std::fs::write(&resolved, content.as_bytes())
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write file: {e}")))?;

        // 更新 FileStateCache（写操作后使缓存与磁盘一致）
        if let Some(cap) = ctx.capability.as_ref() {
            if let Some(cache) = cap.read_file_state.as_ref() {
                let mtime_secs = std::fs::metadata(&resolved)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                cache.set(
                    resolved.clone(),
                    FileState {
                        content: content.to_string(),
                        mtime_secs,
                        offset: None,
                        limit: None,
                    },
                );
            }
        }

        let size = content.len();
        Ok(tool_result(
            "write_file",
            json!({ "path": rel, "size": size, "created": true }),
        ))
    }
}
```

- [ ] 在 `workspace.rs` 中添加 `WriteFileRuntimeTool` 实现

### 1.3 注册到 `builtin_runtime_tools()`

在 `builtin/` 模块中找到将 builtin 工具注册到 `ToolDispatcher` 的入口函数（通常是 `src-tauri/src/runtime/tools/builtin.rs` 或通过 `builtin_runtime_registration_test.rs` 引用的路径），添加：

```rust
dispatcher.register(Arc::new(WriteFileRuntimeTool));
```

- [ ] 将 `WriteFileRuntimeTool` 注册到生产 dispatcher

### 1.4 集成测试

新建文件 `src-tauri/tests/write_file_tool_test.rs`：

```rust
//! Integration tests for WriteFileRuntimeTool.

use app_lib::runtime::tools::builtin::workspace::WriteFileRuntimeTool;
use app_lib::runtime::tools::capability::{
    CapabilityContext, FileStateCache,
};
use app_lib::runtime::tools::{AllowAllPermissionPipeline, RuntimeTool, ToolDispatcher};
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

fn make_ctx_with_workspace(tmp: &TempDir) -> app_lib::runtime::tools::ToolExecutionContext {
    let cap = Arc::new(
        CapabilityContext::with_workspace(tmp.path().to_path_buf(), "test-ws")
            .with_read_file_state(Arc::new(FileStateCache::new())),
    );
    app_lib::runtime::tools::ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_capability(cap)
}

/// 写入新文件成功，内容匹配
#[tokio::test]
async fn write_file_creates_new_file() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx_with_workspace(&tmp);

    let tool = WriteFileRuntimeTool;
    let result = tool
        .execute(
            json!({ "path": "hello.txt", "content": "hello world" }),
            ctx,
        )
        .await
        .unwrap();

    assert!(result.content.contains("hello.txt"));
    let written = std::fs::read_to_string(tmp.path().join("hello.txt")).unwrap();
    assert_eq!(written, "hello world");
}

/// 覆盖已存在文件
#[tokio::test]
async fn write_file_overwrites_existing_file() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("existing.txt"), b"old content").unwrap();
    let ctx = make_ctx_with_workspace(&tmp);

    let tool = WriteFileRuntimeTool;
    tool.execute(
        json!({ "path": "existing.txt", "content": "new content" }),
        ctx,
    )
    .await
    .unwrap();

    let written = std::fs::read_to_string(tmp.path().join("existing.txt")).unwrap();
    assert_eq!(written, "new content");
}

/// 自动创建父目录
#[tokio::test]
async fn write_file_creates_parent_dirs() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx_with_workspace(&tmp);

    let tool = WriteFileRuntimeTool;
    tool.execute(
        json!({ "path": "subdir/nested/file.txt", "content": "nested" }),
        ctx,
    )
    .await
    .unwrap();

    assert!(tmp.path().join("subdir/nested/file.txt").exists());
}

/// 路径穿越被拒绝
#[tokio::test]
async fn write_file_rejects_path_traversal() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx_with_workspace(&tmp);

    let tool = WriteFileRuntimeTool;
    let result = tool
        .execute(
            json!({ "path": "../escape.txt", "content": "evil" }),
            ctx,
        )
        .await;

    assert!(result.is_err(), "Path traversal should be rejected");
}

/// 写入后 FileStateCache 被更新
#[tokio::test]
async fn write_file_updates_file_state_cache() {
    let tmp = TempDir::new().unwrap();
    let cache = Arc::new(FileStateCache::new());
    let cap = Arc::new(
        CapabilityContext::with_workspace(tmp.path().to_path_buf(), "test-ws")
            .with_read_file_state(cache.clone()),
    );
    let ctx = app_lib::runtime::tools::ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_capability(cap);

    let tool = WriteFileRuntimeTool;
    tool.execute(
        json!({ "path": "cached.txt", "content": "cached content" }),
        ctx,
    )
    .await
    .unwrap();

    let resolved = tmp.path().join("cached.txt");
    let state = cache.get(&resolved);
    assert!(state.is_some(), "FileStateCache should be updated after write");
    assert_eq!(state.unwrap().content, "cached content");
}

/// missing required field 返回 ExecutionFailed
#[tokio::test]
async fn write_file_missing_path_returns_error() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx_with_workspace(&tmp);

    let tool = WriteFileRuntimeTool;
    let result = tool
        .execute(json!({ "content": "no path given" }), ctx)
        .await;
    assert!(result.is_err());
}
```

- [ ] 新建 `src-tauri/tests/write_file_tool_test.rs` 并通过全部用例

### 1.5 回归测试命令

```bash
cd src-tauri && cargo test --test write_file_tool_test -- --nocapture
cd src-tauri && cargo test review_ --tests --no-fail-fast
```

- [ ] 全部测试绿灯

### 1.6 Git Commit

```
feat(tools): WriteFileTool — write_file RuntimeTool with FileStateCache update
```

---

## Task 2：EditFileTool（`edit_file`）

### 2.1 Catalog 注册

在 `catalog.rs` 的 `write_file` 条目之后添加：

```rust
c.insert(CatalogEntry::new(
    ToolDefinition::new(
        "edit_file",
        "基于 old_string/new_string 精确替换编辑授权工作目录中的文件（要求 old_string 在文件中唯一存在）",
    )
    .with_kind(ToolKind::Primitive)
    .with_capability_scope(["workspace:read", "workspace:write"]),
    json!({
        "type": "object",
        "required": ["path", "old_string", "new_string"],
        "properties": {
            "path": { "type": "string", "description": "相对于授权工作目录的文件路径" },
            "old_string": {
                "type": "string",
                "description": "要替换的原始字符串，必须在文件中唯一存在。若为空字符串，则视为向空文件追加内容（文件必须为空或不存在）"
            },
            "new_string": { "type": "string", "description": "替换后的新字符串" }
        }
    }),
));
```

- [ ] 在 `catalog.rs` 中添加以上条目

### 2.2 RuntimeTool 实现

在 `workspace.rs` 中继 `WriteFileRuntimeTool` 之后新增：

```rust
// ── EditFileRuntimeTool ───────────────────────────────────────────────────

pub struct EditFileRuntimeTool;

#[async_trait]
impl RuntimeTool for EditFileRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("edit_file")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("edit_file", "Edit workspace file"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let root = require_workspace_root(&ctx)?;
        let rel = input
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: path".into()))?;
        let old_string = input
            .get("old_string")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: old_string".into()))?;
        let new_string = input
            .get("new_string")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: new_string".into()))?;

        let resolved = resolve_path(&root, rel)?;

        // 读取现有内容（old_string 为空且文件不存在 → 创建新文件）
        let original_content = if resolved.is_file() {
            std::fs::read_to_string(&resolved)
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read file: {e}")))?
        } else if old_string.is_empty() {
            String::new()
        } else {
            return Err(ToolError::ExecutionFailed(format!(
                "File does not exist: {rel}"
            )));
        };

        // old_string 为空 → 文件必须为空（或刚创建）
        if old_string.is_empty() {
            if !original_content.trim().is_empty() {
                return Err(ToolError::ExecutionFailed(
                    "old_string is empty but file already has content. Use write_file to overwrite, or provide old_string to match existing content.".into(),
                ));
            }
            // 向空文件写入
            let content = new_string;
            if let Some(parent) = resolved.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| ToolError::ExecutionFailed(format!("Failed to create dirs: {e}")))?;
            }
            std::fs::write(&resolved, content.as_bytes())
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write file: {e}")))?;
            update_file_state_cache(&ctx, &resolved, content);
            return Ok(tool_result(
                "edit_file",
                json!({ "path": rel, "operation": "create", "bytes_written": content.len() }),
            ));
        }

        // 验证 old_string 唯一存在
        let matches = original_content.matches(old_string).count();
        if matches == 0 {
            return Err(ToolError::ExecutionFailed(format!(
                "old_string not found in file: {rel}\nString: {old_string}"
            )));
        }
        if matches > 1 {
            return Err(ToolError::ExecutionFailed(format!(
                "old_string found {matches} times in file: {rel}. Provide more context to uniquely identify the target.\nString: {old_string}"
            )));
        }

        // 执行替换
        let updated_content = original_content.replacen(old_string, new_string, 1);

        std::fs::write(&resolved, updated_content.as_bytes())
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write file: {e}")))?;

        update_file_state_cache(&ctx, &resolved, &updated_content);

        Ok(tool_result(
            "edit_file",
            json!({
                "path": rel,
                "operation": "edit",
                "bytes_written": updated_content.len(),
            }),
        ))
    }
}

fn update_file_state_cache(ctx: &ToolExecutionContext, resolved: &std::path::Path, content: &str) {
    if let Some(cap) = ctx.capability.as_ref() {
        if let Some(cache) = cap.read_file_state.as_ref() {
            let mtime_secs = std::fs::metadata(resolved)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            cache.set(
                resolved.to_path_buf(),
                FileState {
                    content: content.to_string(),
                    mtime_secs,
                    offset: None,
                    limit: None,
                },
            );
        }
    }
}
```

注意：`update_file_state_cache` 是一个模块级��有辅助函数，同时被 `WriteFileRuntimeTool` 和 `EditFileRuntimeTool` 复用（需同时重构 `WriteFileRuntimeTool` 的缓存更新逻辑，改为调用该函数）。

- [ ] 在 `workspace.rs` 中添加 `EditFileRuntimeTool` 和 `update_file_state_cache`
- [ ] 将 `WriteFileRuntimeTool` 中的内联缓存更新逻辑重构为调用 `update_file_state_cache`

### 2.3 注册到生产 dispatcher

```rust
dispatcher.register(Arc::new(EditFileRuntimeTool));
```

- [ ] 将 `EditFileRuntimeTool` 注册到生产 dispatcher

### 2.4 集成测试

新建文件 `src-tauri/tests/edit_file_tool_test.rs`：

```rust
//! Integration tests for EditFileRuntimeTool.

use app_lib::runtime::tools::builtin::workspace::EditFileRuntimeTool;
use app_lib::runtime::tools::capability::{
    CapabilityContext, FileStateCache,
};
use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

fn make_ctx(tmp: &TempDir) -> ToolExecutionContext {
    let cap = Arc::new(
        CapabilityContext::with_workspace(tmp.path().to_path_buf(), "test-ws")
            .with_read_file_state(Arc::new(FileStateCache::new())),
    );
    ToolExecutionContext::for_test("conv-1", "run-1", "tc-1").with_capability(cap)
}

/// 正常替换成功
#[tokio::test]
async fn edit_file_replaces_unique_string() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "hello world").unwrap();
    let ctx = make_ctx(&tmp);

    let tool = EditFileRuntimeTool;
    tool.execute(
        json!({ "path": "file.txt", "old_string": "world", "new_string": "rust" }),
        ctx,
    )
    .await
    .unwrap();

    let content = std::fs::read_to_string(tmp.path().join("file.txt")).unwrap();
    assert_eq!(content, "hello rust");
}

/// old_string 不存在 → 返回错误
#[tokio::test]
async fn edit_file_fails_when_old_string_not_found() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "hello world").unwrap();
    let ctx = make_ctx(&tmp);

    let tool = EditFileRuntimeTool;
    let result = tool
        .execute(
            json!({ "path": "file.txt", "old_string": "NONEXISTENT", "new_string": "x" }),
            ctx,
        )
        .await;
    assert!(result.is_err());
}

/// old_string 出现多次 → 返回错误
#[tokio::test]
async fn edit_file_fails_when_old_string_not_unique() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("dup.txt"), "foo foo foo").unwrap();
    let ctx = make_ctx(&tmp);

    let tool = EditFileRuntimeTool;
    let result = tool
        .execute(
            json!({ "path": "dup.txt", "old_string": "foo", "new_string": "bar" }),
            ctx,
        )
        .await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("3 times") || msg.contains("times"), "error should mention count: {msg}");
}

/// 文件不存在且 old_string 非空 → 错误
#[tokio::test]
async fn edit_file_fails_when_file_does_not_exist() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = EditFileRuntimeTool;
    let result = tool
        .execute(
            json!({ "path": "missing.txt", "old_string": "anything", "new_string": "x" }),
            ctx,
        )
        .await;
    assert!(result.is_err());
}

/// old_string 为空 + 文件不存在 → 创建新文件
#[tokio::test]
async fn edit_file_creates_new_file_when_old_string_empty_and_file_missing() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = EditFileRuntimeTool;
    tool.execute(
        json!({ "path": "new.txt", "old_string": "", "new_string": "brand new" }),
        ctx,
    )
    .await
    .unwrap();

    let content = std::fs::read_to_string(tmp.path().join("new.txt")).unwrap();
    assert_eq!(content, "brand new");
}

/// 写入后 FileStateCache 更新为新内容
#[tokio::test]
async fn edit_file_updates_cache_after_edit() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("cache.txt"), "original text").unwrap();
    let cache = Arc::new(FileStateCache::new());
    let cap = Arc::new(
        CapabilityContext::with_workspace(tmp.path().to_path_buf(), "test-ws")
            .with_read_file_state(cache.clone()),
    );
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1").with_capability(cap);

    let tool = EditFileRuntimeTool;
    tool.execute(
        json!({ "path": "cache.txt", "old_string": "original", "new_string": "updated" }),
        ctx,
    )
    .await
    .unwrap();

    let resolved = tmp.path().join("cache.txt");
    let state = cache.get(&resolved).expect("cache should be populated");
    assert_eq!(state.content, "updated text");
}

/// 路径穿越被拒绝
#[tokio::test]
async fn edit_file_rejects_path_traversal() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = EditFileRuntimeTool;
    let result = tool
        .execute(
            json!({ "path": "../etc/passwd", "old_string": "root", "new_string": "evil" }),
            ctx,
        )
        .await;
    assert!(result.is_err());
}

/// 多行文件替换中间段落
#[tokio::test]
async fn edit_file_replaces_multiline_string() {
    let tmp = TempDir::new().unwrap();
    let original = "line one\nline two\nline three\n";
    std::fs::write(tmp.path().join("multi.txt"), original).unwrap();
    let ctx = make_ctx(&tmp);

    let tool = EditFileRuntimeTool;
    tool.execute(
        json!({ "path": "multi.txt", "old_string": "line two\n", "new_string": "LINE TWO\n" }),
        ctx,
    )
    .await
    .unwrap();

    let result = std::fs::read_to_string(tmp.path().join("multi.txt")).unwrap();
    assert_eq!(result, "line one\nLINE TWO\nline three\n");
}
```

- [ ] 新建 `src-tauri/tests/edit_file_tool_test.rs` 并通过全部用例

### 2.5 回归测试命令

```bash
cd src-tauri && cargo test --test edit_file_tool_test -- --nocapture
cd src-tauri && cargo test --test write_file_tool_test -- --nocapture
cd src-tauri && cargo test review_ --tests --no-fail-fast
```

- [ ] 全部测试绿灯

### 2.6 Git Commit

```
feat(tools): EditFileTool — edit_file with old/new string replace + cache update
```

---

## Task 3：BashTool（`bash`）

BashTool 是四个工具中复杂度最高的，核心难点在于：**异步进程执行 + CancellationToken 联动 + timeout 处理**。对照 claude-code-best `BashTool.tsx` / `ShellCommand.ts` 之后，Task 3 需要先做一处设计校准：**claude-code-best 并不是“所有 timeout 都转后台”**，而是“只有具备 shell background task 基础设施、且命令允许 auto-background 时，timeout 才转后台；否则 timeout 直接终止前台进程”。lotus 当前还没有 `LocalShellTask` / `TaskOutput` / background notification 这一整套 shell 后台生命周期，所以本 Task 的正确对标落点应当是：**先实现前台 BashTool**——timeout/cancel 都终止子进程，保留 claude-code-best 的前台执行语义；`run_in_background` / timeout auto-background 作为后续 shell task 基础设施建设再补，不在本 Task 伪造。

### 3.1 Catalog 注册

在 `catalog.rs` 的 `edit_file` 条目之后添加：

```rust
c.insert(CatalogEntry::new(
    ToolDefinition::new(
        "bash",
        "在授权工作目录中执行 shell 命令。默认 timeout 120s；当前前台路径在 timeout/cancel 时终止进程并返回错误。\
        \n\n安全约束：仅对明显危险 pattern（`rm -rf /`、向 /etc/ 写入等）做 hard deny。\
        \n\n stdout + stderr 合并返回；非零 exit code 默认按错误处理，grep/rg/find/diff/test 等遵循 claude-code-best 的语义豁免。",
    )
    .with_kind(ToolKind::Primitive)
    .with_destructive(true)
    .with_capability_scope(["workspace:write"]),
    json!({
        "type": "object",
        "required": ["command"],
        "properties": {
            "command": { "type": "string", "description": "要执行的 shell 命令" },
            "timeout_secs": {
                "type": "integer",
                "description": "超时秒数，默认 120，最大 600",
                "default": 120
            }
        }
    }),
));
```

- [ ] 在 `catalog.rs` 中添加以上条目

### 3.2 RuntimeTool 实现

新建文件 `src-tauri/src/runtime/tools/builtin/bash.rs`：

```rust
//! BashTool — 在授权工作目录中执行 shell 命令（tokio::process，可取消）。
//!
//! 设计原则：
//! - CancellationToken 通过 `tokio::select!` 监听；当前 lotus 无 shell background infra，任意 cancel 都直接 kill 子进程
//! - Timeout 走 claude-code-best 的“前台路径”语义：终止前台进程并返回 timeout 错误；不伪造 background task
//! - 非零 exit code 默认按错误处理；`grep`/`rg`/`find`/`diff`/`test`/`[` 保留 claude-code-best 的 command semantics 例外
//! - stdout/stderr 分管道并发读取、最终拼接，避免 pipe buffer 死锁
//! - is_destructive = true（默认），仅对不可恢复/越权 pattern 做最小 hard deny

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::permission::{PermissionDecision, PermissionReason};
use crate::runtime::tools::RuntimeTool;

use super::workspace::require_workspace_root;

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 600;
/// 单次命令输出最大字节数（约 512KB）
const MAX_OUTPUT_BYTES: usize = 512 * 1024;

/// 明显危险的命令 pattern（最小集，仅覆盖不可恢复/越权操作）。
/// 与 claude-code-best 的 parser-driven ask 流程不同，这里仅保留最小 hard deny；
/// 更细的 ask/classifier 语义留待后续 shell permission 基础设施补齐。
static DANGEROUS_PATTERNS: &[(&str, &str)] = &[
    ("rm -rf /", "Refusing: rm -rf / would destroy the entire filesystem"),
    ("rm -rf /*", "Refusing: rm -rf /* would destroy the entire filesystem"),
    ("> /etc/", "Refusing: writing to /etc/ is not allowed"),
    (">> /etc/", "Refusing: writing to /etc/ is not allowed"),
    ("> /bin/", "Refusing: writing to /bin/ is not allowed"),
    ("> /usr/bin/", "Refusing: writing to /usr/bin/ is not allowed"),
    ("mkfs", "Refusing: mkfs formats filesystems"),
    ("dd if=", "Refusing: dd with if= can be dangerous; use with caution"),
];

pub struct BashTool;

fn tool_result_bash(content: impl Into<String>, data: Value) -> ToolResult {
    ToolResult {
        tool_name: "bash".to_string(),
        content: content.into(),
        data: Some(data),
        file_meta: None,
        is_degraded: false,
        degradation_notice: None,
    }
}

#[async_trait]
impl RuntimeTool for BashTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("bash")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("bash", "Execute shell command"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
    }

    async fn check_permissions(
        &self,
        input: &Value,
        _ctx: &ToolExecutionContext,
    ) -> Option<PermissionDecision> {
        let command = input.get("command").and_then(Value::as_str).unwrap_or("");
        for (pattern, message) in DANGEROUS_PATTERNS {
            if command.contains(pattern) {
                return Some(PermissionDecision::Deny {
                    message: message.to_string(),
                    reason: PermissionReason::Other("dangerous_pattern".to_string()),
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
        let root = require_workspace_root(&ctx)?;

        let command = input
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: command".into()))?;

        let timeout_secs = input
            .get("timeout_secs")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .min(MAX_TIMEOUT_SECS);

        // 启动进程：在 authorized workspace root 目录下执行 shell 命令。
        // 当前阶段不引入持久 shell/cwd；每次调用都从 workspace root 开始。
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(command)
            .current_dir(&root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to spawn process: {e}")))?;

        let mut stdout = child.stdout.take().expect("stdout piped");
        let mut stderr = child.stderr.take().expect("stderr piped");

        // 用 cancellation token + timeout 竞争等待
        let timeout_duration = Duration::from_secs(timeout_secs);
        let cancellation = ctx.cancellation.clone();

        // 读取输出（限制最大字节）+ 等待进程结束，三路 select：
        //   1. 进程自然结束（读完 stdout/stderr + wait）
        //   2. timeout → kill 子进程并返回 ToolError
        //   3. CancellationToken cancel → kill 子进程并返回 ToolError

        tokio::select! {
            result = async {
                let mut out_buf = Vec::new();
                let mut err_buf = Vec::new();
                // 并发读取 stdout/stderr，避免管道缓冲区死锁
                let (out_res, err_res) = tokio::join!(
                    async {
                        let mut limited = stdout.take(MAX_OUTPUT_BYTES as u64);
                        limited.read_to_end(&mut out_buf).await.map(|_| &out_buf)
                    },
                    async {
                        let mut limited = stderr.take(MAX_OUTPUT_BYTES as u64);
                        limited.read_to_end(&mut err_buf).await.map(|_| &err_buf)
                    }
                );
                let status = child.wait().await;
                (out_res.map(|_| out_buf), err_res.map(|_| err_buf), status)
            } => {
                let (out_res, err_res, status_res) = result;
                let stdout_str = out_res.map(|b| String::from_utf8_lossy(&b).to_string())
                    .unwrap_or_default();
                let stderr_str = err_res.map(|b| String::from_utf8_lossy(&b).to_string())
                    .unwrap_or_default();
                let combined = if stderr_str.is_empty() {
                    stdout_str.clone()
                } else {
                    format!("{stdout_str}{stderr_str}")
                };
                let exit_code = status_res.ok().and_then(|s| s.code());
                let exit_code = exit_code.unwrap_or(-1);
                let semantics = interpret_command_result(command, exit_code);
                if semantics.is_error {
                    return Err(ToolError::ExecutionFailed(format_command_failure(
                        command,
                        exit_code,
                        &combined,
                        semantics.message.as_deref(),
                    )));
                }

                let truncated = combined.len() >= MAX_OUTPUT_BYTES;
                Ok(tool_result_bash(
                    &combined,
                    json!({
                        "command": command,
                        "exit_code": exit_code,
                        "stdout_stderr": combined,
                        "truncated": truncated,
                        "semantic_message": semantics.message,
                    }),
                ))
            }
            _ = tokio::time::sleep(timeout_duration) => {
                let _ = child.kill().await;
                Err(ToolError::ExecutionFailed(format!(
                    "Command timed out after {timeout_secs}s"
                )))
            }
            _ = async {
                while !cancellation.is_cancelled() {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            } => {
                // CancellationToken 触发：当前 lotus 无 shell background infra，直接 kill 进程
                let _ = child.kill().await;
                Err(ToolError::ExecutionFailed("Command cancelled".into()))
            }
        }
    }
}
```

**注意**：上述实现使用 `tokio::select!` 三路竞争。stdout/stderr 并发读取避免管道死锁。CancellationToken polling 间隔 50ms（不引入额外依赖）。

- [ ] 新建 `src-tauri/src/runtime/tools/builtin/bash.rs`

### 3.3 注册到 `builtin/mod.rs`

在 `src-tauri/src/runtime/tools/builtin/mod.rs` 添加：

```rust
pub mod bash;
```

以及在生产 dispatcher 注册函数中添加：

```rust
dispatcher.register(Arc::new(bash::BashTool));
```

- [ ] 在 `mod.rs` 中添加 `pub mod bash;`
- [ ] 将 `BashTool` 注册到生产 dispatcher

### 3.4 集成测试

新建文件 `src-tauri/tests/bash_tool_test.rs`：

```rust
//! Integration tests for BashTool.
//! 所有测试仅使用安全命令（echo, ls, cat 等），不执行危险操作。

use app_lib::runtime::tools::builtin::bash::BashTool;
use app_lib::runtime::tools::capability::CapabilityContext;
use app_lib::runtime::tools::permission::PermissionDecision;
use app_lib::runtime::tools::{RuntimeTool, ToolDispatcher, AllowAllPermissionPipeline, ToolExecutionContext};
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

fn make_ctx(tmp: &TempDir) -> ToolExecutionContext {
    let cap = Arc::new(CapabilityContext::with_workspace(tmp.path().to_path_buf(), "test-ws"));
    ToolExecutionContext::for_test("conv-1", "run-1", "tc-1").with_capability(cap)
}

/// 执行简单 echo 命令，返回输出
#[tokio::test]
async fn bash_executes_echo_command() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = BashTool;
    let result = tool
        .execute(json!({ "command": "echo hello" }), ctx)
        .await
        .unwrap();

    assert!(result.content.contains("hello"), "Output should contain 'hello': {}", result.content);
}

/// 默认语义下，非零 exit code 应返回 ToolError（对标 claude-code-best）
#[tokio::test]
async fn bash_returns_error_for_nonzero_exit_code() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = BashTool;
    let result = tool
        .execute(json!({ "command": "exit 42" }), ctx)
        .await;

    assert!(result.is_err(), "exit 42 should surface as tool error");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("42") || err.contains("exit code"),
        "Should mention exit code: {err}"
    );
}

/// grep/rg/find/diff/test 等沿用 claude-code-best command semantics：
/// exit 1 可作为“无匹配/不同/条件为假”的非错误结果返回
#[tokio::test]
async fn bash_allows_grep_exit_one_as_non_error() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("sample.txt"), "hello world\n").unwrap();
    let ctx = make_ctx(&tmp);

    let tool = BashTool;
    let result = tool
        .execute(
            json!({ "command": "grep needle sample.txt" }),
            ctx,
        )
        .await
        .unwrap();

    let data = result.data.unwrap();
    assert_eq!(data["exit_code"], json!(1));
    assert_eq!(data["semantic_message"], json!("No matches found"));
}

/// 命令在工作目录下执行（cwd = workspace root）
#[tokio::test]
async fn bash_runs_in_workspace_root() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("sentinel.txt"), b"marker").unwrap();
    let ctx = make_ctx(&tmp);

    let tool = BashTool;
    let result = tool
        .execute(json!({ "command": "ls sentinel.txt" }), ctx)
        .await
        .unwrap();

    assert!(
        result.content.contains("sentinel.txt"),
        "Should see sentinel.txt in workspace: {}",
        result.content
    );
}

/// stdout + stderr 合并返回
#[tokio::test]
async fn bash_merges_stdout_and_stderr() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = BashTool;
    let result = tool
        .execute(
            json!({ "command": "echo STDOUT && echo STDERR >&2" }),
            ctx,
        )
        .await
        .unwrap();

    // 合并输出中应含有两者
    assert!(result.content.contains("STDOUT"), "Should contain STDOUT: {}", result.content);
    assert!(result.content.contains("STDERR"), "Should contain STDERR: {}", result.content);
}

/// timeout 走前台 kill 路径：返回 ToolError，而不是伪造 background result
#[tokio::test]
async fn bash_returns_error_on_timeout() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = BashTool;
    let result = tool
        .execute(
            json!({ "command": "sleep 10", "timeout_secs": 1 }),
            ctx,
        )
        .await;

    assert!(result.is_err(), "timeout should surface as tool error");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("timed out") || err.contains("timeout"),
        "Should indicate timeout: {err}"
    );
    assert!(
        !err.contains("background"),
        "Current lotus Task 3 must not claim background semantics: {err}"
    );
}

/// CancellationToken cancel → ToolError::ExecutionFailed("Command cancelled")
#[tokio::test]
async fn bash_returns_error_when_cancelled() {
    use app_lib::runtime::cancellation::CancellationToken;
    use app_lib::runtime::ids::{RunId, SessionId, ToolCallId};

    let tmp = TempDir::new().unwrap();
    let token = CancellationToken::new();
    let cap = Arc::new(CapabilityContext::with_workspace(tmp.path().to_path_buf(), "test-ws"));

    let ctx = ToolExecutionContext::new(
        SessionId::new("conv-1"),
        RunId::new("run-1"),
        None,
        ToolCallId::new("tc-1"),
        token.clone(),
    )
    .with_capability(cap);

    // 在另一个任务里立即 cancel
    let token_clone = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        token_clone.cancel();
    });

    let tool = BashTool;
    let result = tool
        .execute(json!({ "command": "sleep 10" }), ctx)
        .await;

    assert!(result.is_err(), "Cancelled command should return error");
    let err = result.unwrap_err().to_string();
    assert!(err.contains("cancelled") || err.contains("cancel"), "Error should mention cancellation: {err}");
}

/// 危险 pattern rm -rf / → check_permissions 返回 Deny
#[tokio::test]
async fn bash_denies_rm_rf_slash() {
    let tmp = TempDir::new().unwrap();
    let cap = Arc::new(CapabilityContext::with_workspace(tmp.path().to_path_buf(), "test-ws"));
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1").with_capability(cap);

    let tool = BashTool;
    let input = json!({ "command": "rm -rf /" });
    let decision = tool.check_permissions(&input, &ctx).await;

    assert!(
        matches!(decision, Some(PermissionDecision::Deny { .. })),
        "rm -rf / should be denied by check_permissions"
    );
}

/// 危险 pattern > /etc/ → Deny
#[tokio::test]
async fn bash_denies_write_to_etc() {
    let tmp = TempDir::new().unwrap();
    let cap = Arc::new(CapabilityContext::with_workspace(tmp.path().to_path_buf(), "test-ws"));
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1").with_capability(cap);

    let tool = BashTool;
    let input = json!({ "command": "echo evil > /etc/passwd" });
    let decision = tool.check_permissions(&input, &ctx).await;

    assert!(
        matches!(decision, Some(PermissionDecision::Deny { .. })),
        "Writing to /etc/ should be denied"
    );
}

/// missing capability → PermissionDenied
#[tokio::test]
async fn bash_fails_without_capability_context() {
    let tool = BashTool;
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1"); // no capability
    let result = tool
        .execute(json!({ "command": "echo hi" }), ctx)
        .await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("permission") || err.contains("capability"), "Should mention permission/capability: {err}");
}
```

- [ ] 新建 `src-tauri/tests/bash_tool_test.rs` 并通过全部用例

### 3.5 Cargo.toml 依赖检查

确认 `src-tauri/Cargo.toml` 中已有以下依赖（`tokio` 需要 `process` feature）：

```toml
tokio = { version = "...", features = ["...", "process", "time", "io-util"] }
```

- [ ] 确认 `tokio` 的 `process`、`time`、`io-util` features 已启用，若无则添加

### 3.6 回归测试命令

```bash
cd src-tauri && cargo test --test bash_tool_test -- --nocapture
cd src-tauri && cargo test review_ --tests --no-fail-fast
```

- [ ] 全部测试绿灯

### 3.7 Git Commit

```
feat(tools): BashTool — bash RuntimeTool with tokio::process + cancellation + timeout
```

---

## Task 4：GrepTool（`grep_content`）

GrepTool 对标 claude-code-best 的 GrepTool（`output_mode: content|files_with_matches|count`），使用 Rust `regex` crate 实现递归文件内容搜索。

### 4.1 Cargo.toml 依赖

在 `src-tauri/Cargo.toml` 中确认或添加：

```toml
regex = "1"
```

- [ ] 确认 `regex` crate 已在 `Cargo.toml` 中声明

### 4.2 Catalog 注册

在 `catalog.rs` 的 `bash` 条目之后添加：

```rust
c.insert(CatalogEntry::new(
    ToolDefinition::new(
        "grep_content",
        "在授权工作目录中用正则表达式搜索文件内容（对标 claude-code-best GrepTool）",
    )
    .with_kind(ToolKind::Primitive)
    .with_read_only(true)
    .with_capability_scope(["workspace:read"]),
    json!({
        "type": "object",
        "required": ["pattern"],
        "properties": {
            "pattern": { "type": "string", "description": "正则表达式搜索模式" },
            "path": {
                "type": "string",
                "description": "搜索起始目录（相对于 workspace root），默认 '.'",
                "default": "."
            },
            "glob": {
                "type": "string",
                "description": "文件名 glob 过滤（仅支持简单 * 通配符，如 '*.rs'），为空则搜索所有文件"
            },
            "output_mode": {
                "type": "string",
                "enum": ["content", "files_with_matches", "count"],
                "description": "输出模式：content 返回匹配行内容，files_with_matches 仅返回文件路径，count 返回匹配数统计",
                "default": "files_with_matches"
            }
        }
    }),
));
```

- [ ] 在 `catalog.rs` 中添加以上条目

### 4.3 RuntimeTool 实现

新建文件 `src-tauri/src/runtime/tools/builtin/grep.rs`：

```rust
//! GrepTool — 在授权工作目录中用正则表达式搜索文件内容。
//!
//! 三种输出模式（对标 claude-code-best GrepTool）：
//! - content：返回每个匹配行（文件名:行号:内容）
//! - files_with_matches：仅返回包含匹配的文件路径列表
//! - count：返回每个文件的匹配行数统计

use anyhow::Result;
use async_trait::async_trait;
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;

use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;
use crate::storage::file_manager;

use super::workspace::require_workspace_root;

const MAX_RESULTS: usize = 1000;
const MAX_FILE_SIZE_BYTES: usize = 2 * 1024 * 1024; // 2MB

pub struct GrepContentTool;

fn tool_result(value: Value) -> ToolResult {
    ToolResult {
        tool_name: "grep_content".to_string(),
        content: serde_json::to_string_pretty(&value).unwrap_or_default(),
        data: Some(value),
        file_meta: None,
        is_degraded: false,
        degradation_notice: None,
    }
}

/// 简单文件名 glob 匹配（支持 * 通配符，复用 workspace.rs 中的 matches_glob 逻辑）
fn matches_glob(name: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return name == pattern;
    }
    let mut remaining = name;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !remaining.starts_with(part) {
                return false;
            }
            remaining = &remaining[part.len()..];
        } else if i == parts.len() - 1 {
            if !remaining.ends_with(part) {
                return false;
            }
        } else if let Some(pos) = remaining.find(part) {
            remaining = &remaining[pos + part.len()..];
        } else {
            return false;
        }
    }
    true
}

struct GrepResult {
    files_with_matches: Vec<String>,
    content_matches: Vec<Value>,
    count_by_file: Vec<Value>,
    total_matches: usize,
    files_searched: usize,
    truncated: bool,
}

fn grep_recursive(
    dir: &Path,
    root: &Path,
    regex: &Regex,
    glob: &str,
    results: &mut GrepResult,
) {
    if results.total_matches >= MAX_RESULTS {
        results.truncated = true;
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut sorted_entries: Vec<_> = entries.flatten().collect();
    sorted_entries.sort_by_key(|e| e.file_name());

    for entry in sorted_entries {
        if results.total_matches >= MAX_RESULTS {
            results.truncated = true;
            return;
        }
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_symlink() {
            continue;
        }
        let path = entry.path();
        if ft.is_dir() {
            // 跳过隐藏目录（.git 等）
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            grep_recursive(&path, root, regex, glob, results);
        } else if ft.is_file() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !glob.is_empty() && !matches_glob(&name, glob) {
                continue;
            }
            results.files_searched += 1;

            // 跳过过大文件
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0) as usize;
            if size > MAX_FILE_SIZE_BYTES {
                continue;
            }

            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let content = String::from_utf8_lossy(&bytes);
            let rel = path
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| path.to_string_lossy().to_string());

            let mut file_matches = 0usize;
            let mut file_content_matches = Vec::new();

            for (line_no, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    file_matches += 1;
                    results.total_matches += 1;
                    file_content_matches.push(json!({
                        "path": rel,
                        "line_number": line_no + 1,
                        "line": line,
                    }));
                    if results.total_matches >= MAX_RESULTS {
                        results.truncated = true;
                        break;
                    }
                }
            }

            if file_matches > 0 {
                results.files_with_matches.push(rel.clone());
                results.content_matches.extend(file_content_matches);
                results.count_by_file.push(json!({ "path": rel, "count": file_matches }));
            }
        }
    }
}

#[async_trait]
impl RuntimeTool for GrepContentTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("grep_content")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("grep_content", "Grep file content"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let root = require_workspace_root(&ctx)?;

        let pattern = input
            .get("pattern")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: pattern".into()))?;

        let sub = input.get("path").and_then(Value::as_str).unwrap_or(".");
        let glob = input.get("glob").and_then(Value::as_str).unwrap_or("");
        let output_mode = input
            .get("output_mode")
            .and_then(Value::as_str)
            .unwrap_or("files_with_matches");

        let base = file_manager::resolve_local_reference(&root, sub)
            .map_err(|e| ToolError::PermissionDenied(e.to_string()))?;

        if !base.is_dir() {
            return Err(ToolError::ExecutionFailed(format!("Not a directory: {sub}")));
        }

        let regex = Regex::new(pattern)
            .map_err(|e| ToolError::ExecutionFailed(format!("Invalid regex: {e}")))?;

        let mut results = GrepResult {
            files_with_matches: Vec::new(),
            content_matches: Vec::new(),
            count_by_file: Vec::new(),
            total_matches: 0,
            files_searched: 0,
            truncated: false,
        };

        grep_recursive(&base, &root, &regex, glob, &mut results);

        let response = match output_mode {
            "content" => json!({
                "pattern": pattern,
                "path": sub,
                "output_mode": "content",
                "matches": results.content_matches,
                "total_matches": results.total_matches,
                "files_searched": results.files_searched,
                "truncated": results.truncated,
            }),
            "count" => json!({
                "pattern": pattern,
                "path": sub,
                "output_mode": "count",
                "counts": results.count_by_file,
                "total_matches": results.total_matches,
                "files_searched": results.files_searched,
                "truncated": results.truncated,
            }),
            _ => json!({ // files_with_matches (default)
                "pattern": pattern,
                "path": sub,
                "output_mode": "files_with_matches",
                "files": results.files_with_matches,
                "num_files": results.files_with_matches.len(),
                "total_matches": results.total_matches,
                "files_searched": results.files_searched,
                "truncated": results.truncated,
            }),
        };

        Ok(tool_result(response))
    }
}
```

- [ ] 新建 `src-tauri/src/runtime/tools/builtin/grep.rs`

### 4.4 注册到 `builtin/mod.rs`

```rust
pub mod grep;
```

以及在生产 dispatcher 注册函数中添加：

```rust
dispatcher.register(Arc::new(grep::GrepContentTool));
```

- [ ] 在 `mod.rs` 中添加 `pub mod grep;`
- [ ] 将 `GrepContentTool` 注册到生产 dispatcher

### 4.5 集成测试

新建文件 `src-tauri/tests/grep_content_tool_test.rs`：

```rust
//! Integration tests for GrepContentTool.

use app_lib::runtime::tools::builtin::grep::GrepContentTool;
use app_lib::runtime::tools::capability::CapabilityContext;
use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

fn make_ctx(tmp: &TempDir) -> ToolExecutionContext {
    let cap = Arc::new(CapabilityContext::with_workspace(tmp.path().to_path_buf(), "test-ws"));
    ToolExecutionContext::for_test("conv-1", "run-1", "tc-1").with_capability(cap)
}

fn setup_test_files(tmp: &TempDir) {
    std::fs::write(tmp.path().join("a.rs"), "fn main() {\n    println!(\"hello\");\n}\n").unwrap();
    std::fs::write(tmp.path().join("b.rs"), "fn foo() {}\nfn bar() {}\n").unwrap();
    std::fs::write(tmp.path().join("c.txt"), "hello world\nno match here\n").unwrap();
    std::fs::create_dir_all(tmp.path().join("subdir")).unwrap();
    std::fs::write(
        tmp.path().join("subdir/d.rs"),
        "// hello from subdir\nfn baz() {}\n",
    )
    .unwrap();
}

/// files_with_matches 模式：返回匹配文件路径列表
#[tokio::test]
async fn grep_files_with_matches_mode() {
    let tmp = TempDir::new().unwrap();
    setup_test_files(&tmp);
    let ctx = make_ctx(&tmp);

    let tool = GrepContentTool;
    let result = tool
        .execute(
            json!({ "pattern": "hello", "output_mode": "files_with_matches" }),
            ctx,
        )
        .await
        .unwrap();

    let data = result.data.unwrap();
    let files = data["files"].as_array().unwrap();
    // a.rs, c.txt, subdir/d.rs 都包含 hello
    assert!(files.len() >= 3, "Should find at least 3 files: {:?}", files);
}

/// content 模式：返回匹配行内容和行号
#[tokio::test]
async fn grep_content_mode_returns_line_numbers() {
    let tmp = TempDir::new().unwrap();
    setup_test_files(&tmp);
    let ctx = make_ctx(&tmp);

    let tool = GrepContentTool;
    let result = tool
        .execute(
            json!({ "pattern": "fn main", "output_mode": "content" }),
            ctx,
        )
        .await
        .unwrap();

    let data = result.data.unwrap();
    let matches = data["matches"].as_array().unwrap();
    assert!(!matches.is_empty(), "Should find fn main match");
    let first = &matches[0];
    assert!(first["line_number"].as_u64().unwrap() > 0);
    assert!(first["line"].as_str().unwrap().contains("fn main"));
}

/// count 模式：返回每个文件的匹配数
#[tokio::test]
async fn grep_count_mode() {
    let tmp = TempDir::new().unwrap();
    setup_test_files(&tmp);
    let ctx = make_ctx(&tmp);

    let tool = GrepContentTool;
    let result = tool
        .execute(
            json!({ "pattern": "fn ", "output_mode": "count" }),
            ctx,
        )
        .await
        .unwrap();

    let data = result.data.unwrap();
    let counts = data["counts"].as_array().unwrap();
    assert!(!counts.is_empty());
    // b.rs 有 2 个 fn，应有 count:2
    let b_entry = counts.iter().find(|e| e["path"].as_str().map(|p| p.contains("b.rs")).unwrap_or(false));
    assert!(b_entry.is_some());
    assert_eq!(b_entry.unwrap()["count"], json!(2));
}

/// glob 过滤：只搜索 .rs 文件
#[tokio::test]
async fn grep_glob_filter_rs_files_only() {
    let tmp = TempDir::new().unwrap();
    setup_test_files(&tmp);
    let ctx = make_ctx(&tmp);

    let tool = GrepContentTool;
    let result = tool
        .execute(
            json!({ "pattern": "hello", "glob": "*.rs", "output_mode": "files_with_matches" }),
            ctx,
        )
        .await
        .unwrap();

    let data = result.data.unwrap();
    let files = data["files"].as_array().unwrap();
    // c.txt 不应出现（被 glob 过滤掉）
    let has_txt = files.iter().any(|f| f.as_str().map(|s| s.ends_with(".txt")).unwrap_or(false));
    assert!(!has_txt, "Glob *.rs should exclude .txt files: {:?}", files);
}

/// 子目录搜索
#[tokio::test]
async fn grep_searches_subdirectories() {
    let tmp = TempDir::new().unwrap();
    setup_test_files(&tmp);
    let ctx = make_ctx(&tmp);

    let tool = GrepContentTool;
    let result = tool
        .execute(
            json!({ "pattern": "baz", "output_mode": "files_with_matches" }),
            ctx,
        )
        .await
        .unwrap();

    let data = result.data.unwrap();
    let files = data["files"].as_array().unwrap();
    let has_subdir = files.iter().any(|f| {
        f.as_str()
            .map(|s| s.contains("subdir") && s.contains("d.rs"))
            .unwrap_or(false)
    });
    assert!(has_subdir, "Should find file in subdir: {:?}", files);
}

/// 无匹配时返回空结果，不报错
#[tokio::test]
async fn grep_no_matches_returns_empty() {
    let tmp = TempDir::new().unwrap();
    setup_test_files(&tmp);
    let ctx = make_ctx(&tmp);

    let tool = GrepContentTool;
    let result = tool
        .execute(
            json!({ "pattern": "ZZZZZNOMATCH", "output_mode": "files_with_matches" }),
            ctx,
        )
        .await
        .unwrap();

    let data = result.data.unwrap();
    let files = data["files"].as_array().unwrap();
    assert!(files.is_empty(), "No match should return empty files list");
}

/// 无效正则 → ExecutionFailed
#[tokio::test]
async fn grep_invalid_regex_returns_error() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = GrepContentTool;
    let result = tool
        .execute(json!({ "pattern": "[invalid" }), ctx)
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Invalid regex") || err.contains("regex"), "Should mention regex: {err}");
}

/// 路径穿越被拒绝
#[tokio::test]
async fn grep_rejects_path_traversal() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = GrepContentTool;
    let result = tool
        .execute(json!({ "pattern": "root", "path": "../.." }), ctx)
        .await;

    assert!(result.is_err(), "Path traversal should be rejected");
}

/// is_concurrency_safe 和 is_read_only 应返回 true
#[test]
fn grep_is_read_only_and_concurrency_safe() {
    use serde_json::json;
    let tool = GrepContentTool;
    let input = json!({});
    assert!(tool.is_concurrency_safe(&input));
    assert!(tool.is_read_only(&input));
}
```

- [ ] 新建 `src-tauri/tests/grep_content_tool_test.rs` 并通过全部用例

### 4.6 回归测试命令

```bash
cd src-tauri && cargo test --test grep_content_tool_test -- --nocapture
cd src-tauri && cargo test review_ --tests --no-fail-fast
cd src-tauri && cargo test --test builtin_runtime_registration_test -- --nocapture
```

- [ ] 全部测试绿灯

### 4.7 Git Commit

```
feat(tools): GrepContentTool — grep_content with regex, glob, output_mode
```

---

## Task 5：注册验证 + 完整回归

### 5.1 `tool_catalog_contract_test` 验证

确保 `src-tauri/tests/tool_catalog_contract_test.rs`（或等价测试）中能通过以下断言（如果该测试不存在则新增）：

```rust
#[test]
fn all_new_plan_c_tools_are_in_catalog() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;
    for id in &["write_file", "edit_file", "bash", "grep_content"] {
        assert!(
            TOOL_CATALOG.get(id).is_some(),
            "Tool '{id}' should be registered in TOOL_CATALOG"
        );
    }
}
```

- [ ] 验证四个工具均在 `TOOL_CATALOG` 中注册

### 5.2 `builtin_runtime_registration_test` 验证

确认生产 dispatcher 中四个新工具均已注册（通过已有的 registration test 覆盖，或新增检查）。

- [ ] `builtin_runtime_registration_test` 通过

### 5.3 全量回归

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
cd src-tauri && cargo test --test tool_catalog_contract_test -- --nocapture
cd src-tauri && cargo test --test builtin_runtime_registration_test -- --nocapture
cd src-tauri && cargo test --test write_file_tool_test -- --nocapture
cd src-tauri && cargo test --test edit_file_tool_test -- --nocapture
cd src-tauri && cargo test --test bash_tool_test -- --nocapture
cd src-tauri && cargo test --test grep_content_tool_test -- --nocapture
```

- [ ] 所有测试通过，无 review_ 系列测试回归

### 5.4 最终 Git Commit

```
feat(tools): Plan-C complete — write_file, edit_file, bash, grep_content registered and tested
```

---

## 实现注意事项

### FileStateCache 一致性

`WriteFileTool` 和 `EditFileTool` 写入完成后必须调用 `update_file_state_cache`。原因：`read_workspace_file` 有读缓存逻辑——如果写后不更新，下一次 `read_workspace_file` 调用会命中旧缓存，LLM 看到的内容与磁盘不一致。

### BashTool tokio::process 依赖

`src-tauri/Cargo.toml` 的 `tokio` 依赖必须开启 `process` feature：

```toml
tokio = { version = "1", features = ["rt", "rt-multi-thread", "macros", "process", "time", "io-util"] }
```

若未开启则 `tokio::process::Command` 无法编译。

### BashTool stdout/stderr 合并策略

实现中 stdout 和 stderr 分管道读取后字符串拼接。另一种方式是将子进程的 stderr 重定向到 stdout（`Command::stderr(Stdio::from(stdout_raw))`）在 tokio 异步场景下操作较繁琐，且会丢失区分信息；分管道并发读取后合并是更简洁的选择。

### GrepTool 隐藏目录跳过

`grep_recursive` 跳过以 `.` 开头的目录（`.git`、`.cargo` 等），避免搜索 VCS 内部文件导致噪音或性能问题。

### Timeout 语义

对标 claude-code-best 的真实实现，timeout 并不是天然“转后台”。`ShellCommand` 的默认前台路径是：**timeout → 终止进程**；只有在 `run_in_background` / auto-background + `LocalShellTask` / `TaskOutput` / notification 全链路都存在时，才会进入 background continuation。lotus 当前 Task 3 还没有这套 shell 后台基础设施，所以 timeout 必须返回 `Err(ToolError)`，而不是伪造 `Ok(ToolResult)` 的 background 提示。
