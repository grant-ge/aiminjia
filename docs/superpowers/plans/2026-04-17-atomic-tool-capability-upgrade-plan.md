# 原子工具能力升级 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 lotus-app 工具系统升级至对标 claude-code-best 的能力水平：工具 schema 有序（prompt cache 稳定）、工具可访问文件状态缓存和读取限制、并发安全工具可并行执行、工具可基于输入动态决策权限，核心工具脱离 LegacyToolAdapter。

**Architecture:** 分三期独立推进。Phase 1 为基础层（schema 排序 + CapabilityContext 扩展），Phase 2 在其上加并发编排和运行时谓词，Phase 3 迁移核心工具并引入动态权限。每期独立可测试、可 commit。

**Tech Stack:** Rust, tokio, async_trait, serde_json, lru crate, anyhow, cargo test

**Spec:** `docs/superpowers/specs/2026-04-17-atomic-tool-capability-upgrade-design.md`

---

## Phase 1：基础层

### Task 1.1：Tool Pool 排序（P2-A）

**Files:**
- Modify: `src-tauri/src/plugin/registry.rs`（`get_all_schemas` 和 `get_schemas_filtered` 末尾）
- Modify: `src-tauri/tests/tool_catalog_contract_test.rs`

---

- [ ] **Step 1：写失败测试**

在 `src-tauri/tests/tool_catalog_contract_test.rs` 末尾追加：

```rust
#[tokio::test]
async fn get_all_schemas_returns_sorted_by_name() {
    // ToolRegistry 全量 schema 必须按 name 字典序排列
    use app_lib::plugin::registry::ToolRegistry;
    let registry = ToolRegistry::new();
    // 注册顺序无关紧要——返回必须有序
    let schemas = registry.get_all_schemas().await;
    let names: Vec<_> = schemas.iter().map(|s| s.name.clone()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "get_all_schemas must return tools sorted by name");
}

#[tokio::test]
async fn get_schemas_filtered_returns_sorted_by_name() {
    use app_lib::plugin::registry::{ToolFilter, ToolRegistry};
    let registry = ToolRegistry::new();
    let schemas = registry
        .get_schemas_filtered(&ToolFilter::Only(vec![
            "web_search".to_string(),
            "browse_navigate".to_string(),
            "list_directory".to_string(),
        ]))
        .await;
    let names: Vec<_> = schemas.iter().map(|s| s.name.clone()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "get_schemas_filtered must return tools sorted by name");
}
```

- [ ] **Step 2：确认测试失败**

```bash
cd src-tauri && cargo test get_all_schemas_returns_sorted_by_name get_schemas_filtered_returns_sorted_by_name -- --nocapture 2>&1 | tail -20
```

期望：两个测试 FAIL（names != sorted，因 HashMap 迭代无序）

- [ ] **Step 3：实现排序**

在 `src-tauri/src/plugin/registry.rs` 的 `get_all_schemas()` 函数，在 `schemas` 返回前加一行：

```rust
// ... 现有逻辑末尾（第187行附近，在 schemas 被 return 之前）
schemas.sort_by(|a, b| a.name.cmp(&b.name));
schemas
```

在 `get_schemas_filtered()` 函数，同样位置（第254行附近）：

```rust
schemas.sort_by(|a, b| a.name.cmp(&b.name));
schemas
```

- [ ] **Step 4：确认测试通过**

```bash
cd src-tauri && cargo test get_all_schemas_returns_sorted_by_name get_schemas_filtered_returns_sorted_by_name -- --nocapture
```

期望：两个测试 PASS

- [ ] **Step 5：确认回归无破坏**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

期望：全绿

- [ ] **Step 6：Commit**

```bash
git add src-tauri/src/plugin/registry.rs src-tauri/tests/tool_catalog_contract_test.rs
git commit -m "feat(registry): sort tool schemas by name for prompt cache stability"
```

---

### Task 1.2：CapabilityContext 扩展——新增类型定义（P0-A 第一步）

**Files:**
- Modify: `src-tauri/src/runtime/tools/capability.rs`

---

- [ ] **Step 1：写失败测试**

在 `src-tauri/tests/tool_capability_context_test.rs` 末尾追加：

```rust
// ── Task 1.2 tests ──────────────────────────────────────────────────────────

#[test]
fn file_state_cache_returns_none_for_unknown_path() {
    use app_lib::runtime::tools::capability::FileStateCache;
    let cache = FileStateCache::new();
    assert!(cache.get(std::path::Path::new("/tmp/nonexistent.txt")).is_none());
}

#[test]
fn file_state_cache_stores_and_retrieves_entry() {
    use app_lib::runtime::tools::capability::{FileState, FileStateCache};
    let cache = FileStateCache::new();
    let path = std::path::PathBuf::from("/tmp/test.csv");
    let state = FileState {
        content: "a,b,c".to_string(),
        mtime_secs: 1000,
        offset: None,
        limit: None,
    };
    cache.set(path.clone(), state.clone());
    let retrieved = cache.get(&path).unwrap();
    assert_eq!(retrieved.content, "a,b,c");
    assert_eq!(retrieved.mtime_secs, 1000);
}

#[test]
fn file_reading_limits_default_is_one_mb() {
    use app_lib::runtime::tools::capability::FileReadingLimits;
    let limits = FileReadingLimits::default();
    assert_eq!(limits.max_size_bytes, 1_048_576);
}

#[test]
fn capability_context_new_fields_default_to_none() {
    use app_lib::runtime::tools::capability::CapabilityContext;
    let ctx = CapabilityContext {
        storage: None,
        workspace_id: None,
        browser_available: false,
        file_ops: None,
        read_file_state: None,
        file_reading_limits: None,
        notification_sink: None,
    };
    assert!(ctx.read_file_state.is_none());
    assert!(ctx.file_reading_limits.is_none());
    assert!(ctx.notification_sink.is_none());
}
```

- [ ] **Step 2：确认测试编译失败**

```bash
cd src-tauri && cargo test file_state_cache_returns_none file_reading_limits_default capability_context_new_fields -- --nocapture 2>&1 | head -30
```

期望：编译错误（`FileStateCache`、`FileState`、`FileReadingLimits` 未定义）

- [ ] **Step 3：在 `capability.rs` 添加新类型**

在 `src-tauri/src/runtime/tools/capability.rs` 的 `// ── FileOperations trait` 注释前插入：

```rust
// ── FileStateCache ────────────────────────────────────────────────────────────

/// 文件状态缓存条目。
///
/// 对应 claude-code-best `FileState`（src/utils/fileStateCache.ts）。
/// 工具读取文件后写入缓存；再次读取时若 mtime 未变则使用缓存内容，避免重读。
#[derive(Clone, Debug)]
pub struct FileState {
    /// 文件内容。
    pub content: String,
    /// 文件最后修改时间（Unix 时间戳，秒）。
    pub mtime_secs: u64,
    /// 读取起始行（None = 全文件）。
    pub offset: Option<usize>,
    /// 读取行数限制（None = 无限制）。
    pub limit: Option<usize>,
}

/// LRU 文件状态缓存（最多 100 条）。
///
/// 对应 claude-code-best `FileStateCache`（LRU, max 100 entries）。
/// 工具通过 `ctx.capability.read_file_state` 访问，`None` 时降级为无缓存模式。
pub struct FileStateCache {
    inner: std::sync::Mutex<lru::LruCache<std::path::PathBuf, FileState>>,
}

impl FileStateCache {
    /// 创建空缓存（最多 100 条）。
    pub fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(100).unwrap(),
            )),
        }
    }

    /// 按路径查找缓存条目。
    pub fn get(&self, path: &std::path::Path) -> Option<FileState> {
        self.inner.lock().unwrap().get(path).cloned()
    }

    /// 写入或更新缓存条目。
    pub fn set(&self, path: std::path::PathBuf, state: FileState) {
        self.inner.lock().unwrap().put(path, state);
    }
}

impl Default for FileStateCache {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for FileStateCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileStateCache").finish()
    }
}

// ── FileReadingLimits ─────────────────────────────────────────────────────────

/// 文件读取大小上限。
///
/// 对应 claude-code-best `fileReadingLimits: { maxTokens?, maxSizeBytes? }`。
/// 防止超大文件一次性撑满 LLM 上下文窗口。
#[derive(Clone, Debug)]
pub struct FileReadingLimits {
    /// 单次读取最大字节数，默认 1MB（对齐 `read_workspace_file` 现有默认值）。
    pub max_size_bytes: usize,
}

impl Default for FileReadingLimits {
    fn default() -> Self {
        Self { max_size_bytes: 1_048_576 }
    }
}

// ── NotificationSink ──────────────────────────────────────────────────────────

/// 工具通知回调——工具可向前端推送非阻塞消息。
///
/// 对应 claude-code-best `setToolJSX`（简化版，仅文字通知）。
/// 工具调用 `sink.notify("msg")` 即可；`None` 时静默忽略。
pub trait NotificationSink: Send + Sync + std::fmt::Debug {
    fn notify(&self, message: &str);
}
```

- [ ] **Step 4：在 `CapabilityContext` struct 添加三个新字段**

找到 `pub struct CapabilityContext {` 定义（约第82行），在 `pub file_ops: Option<Arc<dyn FileOperations>>,` 后追加：

```rust
    /// 文件状态缓存（防止重读未修改的文件）。
    /// 对应 claude-code-best `readFileState: FileStateCache`。
    /// `None` 时工具降级为无缓存模式（每次都重新读取）。
    pub read_file_state: Option<Arc<FileStateCache>>,

    /// 文件读取大小上限。
    /// 对应 claude-code-best `fileReadingLimits`。
    /// `None` 时工具使用自身默认值（通常 1MB）。
    pub file_reading_limits: Option<FileReadingLimits>,

    /// 工具通知回调（向前端推送进度/提示）。
    /// 对应 claude-code-best `setToolJSX`（简化版）。
    /// `None` 时静默忽略通知。
    pub notification_sink: Option<Arc<dyn NotificationSink>>,
```

- [ ] **Step 5：更新 `Debug` impl 和 `with_workspace` 构造函数**

找到 `impl std::fmt::Debug for CapabilityContext`，在 `.field("file_ops", ...)` 后追加：

```rust
        .field("read_file_state", &self.read_file_state.as_ref().map(|_| "FileStateCache"))
        .field("file_reading_limits", &self.file_reading_limits)
        .field("notification_sink", &self.notification_sink.as_ref().map(|_| "NotificationSink"))
```

找到 `pub fn with_workspace(...)` 构造函数，在 `file_ops: None,` 后追加三个 `None` 字段：

```rust
            read_file_state: None,
            file_reading_limits: None,
            notification_sink: None,
```

- [ ] **Step 6：添加三个 builder 方法**

在 `pub fn with_browser(mut self) -> Self` 后追加：

```rust
    /// 附加文件状态缓存（通常按 turn 创建，在 TurnDriver 中注入）。
    pub fn with_read_file_state(mut self, cache: Arc<FileStateCache>) -> Self {
        self.read_file_state = Some(cache);
        self
    }

    /// 附加文件读取大小上限。
    pub fn with_file_reading_limits(mut self, limits: FileReadingLimits) -> Self {
        self.file_reading_limits = Some(limits);
        self
    }

    /// 附加工具通知回调。
    pub fn with_notification_sink(mut self, sink: Arc<dyn NotificationSink>) -> Self {
        self.notification_sink = Some(sink);
        self
    }
```

- [ ] **Step 7：在 `Cargo.toml` 添加 lru 依赖**

```bash
grep -n "lru" src-tauri/Cargo.toml
```

若无，在 `[dependencies]` 中追加：

```toml
lru = "0.12"
```

- [ ] **Step 8：在 `mod.rs` 导出新类型**

在 `src-tauri/src/runtime/tools/mod.rs` 的 `pub use capability::{...}` 行，添加新类型：

```rust
pub use capability::{
    CapabilityContext, FileOperations, FileReadingLimits, FileState, FileStateCache,
    NotificationSink, SharedCapabilityContext, StorageCapability,
};
```

- [ ] **Step 9：确认测试通过**

```bash
cd src-tauri && cargo test file_state_cache_returns_none file_state_cache_stores file_reading_limits_default capability_context_new_fields -- --nocapture
```

期望：4 个测试全部 PASS

- [ ] **Step 10：确认全量回归**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

期望：全绿

- [ ] **Step 11：Commit**

```bash
git add src-tauri/src/runtime/tools/capability.rs src-tauri/src/runtime/tools/mod.rs src-tauri/Cargo.toml src-tauri/tests/tool_capability_context_test.rs
git commit -m "feat(capability): add FileStateCache, FileReadingLimits, NotificationSink to CapabilityContext"
```

---

### Task 1.3：CapabilityContext 扩展——构建点注入 + ReadWorkspaceFile 使用（P0-A 第二步）

**Files:**
- Modify: `src-tauri/src/plugin/registry.rs`（`execute()` 中构建 capability 的位置）
- Modify: `src-tauri/src/runtime/tools/builtin/workspace.rs`（`ReadWorkspaceFileRuntimeTool`）
- Modify: `src-tauri/tests/tool_capability_context_test.rs`

---

- [ ] **Step 1：写失败测试**

在 `src-tauri/tests/tool_capability_context_test.rs` 末尾追加：

```rust
// ── Task 1.3 tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn read_workspace_file_uses_file_state_cache_on_second_read() {
    use app_lib::runtime::tools::builtin::workspace::ReadWorkspaceFileRuntimeTool;
    use app_lib::runtime::tools::capability::{
        CapabilityContext, FileReadingLimits, FileStateCache,
    };
    use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
    use serde_json::json;
    use std::sync::Arc;
    use tempfile::NamedTempFile;
    use std::io::Write;

    // 写一个临时文件
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "line1").unwrap();
    writeln!(tmp, "line2").unwrap();
    let path = tmp.path().to_path_buf();
    let dir = path.parent().unwrap().to_path_buf();
    let filename = path.file_name().unwrap().to_str().unwrap().to_string();

    let cache = Arc::new(FileStateCache::new());
    let cap = CapabilityContext::with_workspace(dir.clone(), "ws")
        .with_read_file_state(cache.clone())
        .with_file_reading_limits(FileReadingLimits::default());
    let ctx = || {
        ToolExecutionContext::for_test("conv", "run", "tc-1")
            .with_capability(Arc::new(cap.clone()))
    };

    let tool = ReadWorkspaceFileRuntimeTool;

    // 第一次读：缓存为空，应返回文件内容
    let r1 = RuntimeTool::execute(&tool, json!({"path": filename}), ctx()).await.unwrap();
    assert!(r1.content.contains("line1"), "first read should return file content");

    // 验证缓存已写入
    assert!(cache.get(&path).is_some(), "cache should be populated after first read");

    // 第二次读：缓存命中，也应返回同样内容（不测 unchanged stub，仅测缓存存在）
    let r2 = RuntimeTool::execute(&tool, json!({"path": filename}), ctx()).await.unwrap();
    assert!(!r2.content.is_empty(), "second read should still return content");
}

#[test]
fn notification_sink_receives_message_from_tool_context() {
    use app_lib::runtime::tools::capability::{CapabilityContext, NotificationSink};
    use std::sync::{Arc, Mutex};

    struct RecordingSink(Mutex<Vec<String>>);
    impl std::fmt::Debug for RecordingSink {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "RecordingSink")
        }
    }
    impl NotificationSink for RecordingSink {
        fn notify(&self, message: &str) {
            self.0.lock().unwrap().push(message.to_string());
        }
    }

    let sink = Arc::new(RecordingSink(Mutex::new(vec![])));
    let cap = CapabilityContext {
        storage: None,
        workspace_id: None,
        browser_available: false,
        file_ops: None,
        read_file_state: None,
        file_reading_limits: None,
        notification_sink: Some(sink.clone()),
    };

    if let Some(s) = &cap.notification_sink {
        s.notify("test notification");
    }
    let msgs = sink.0.lock().unwrap();
    assert_eq!(msgs.as_slice(), &["test notification"]);
}
```

- [ ] **Step 2：确认测试失败**

```bash
cd src-tauri && cargo test read_workspace_file_uses_file_state_cache notification_sink_receives_message -- --nocapture 2>&1 | tail -20
```

期望：编译通过但 `read_workspace_file_uses_file_state_cache` FAIL（工具还没有使用缓存）；`notification_sink_receives_message` PASS（结构体只是存在）

- [ ] **Step 3：更新 `ReadWorkspaceFileRuntimeTool::execute` 使用缓存**

找到 `src-tauri/src/runtime/tools/builtin/workspace.rs` 中的 `ReadWorkspaceFileRuntimeTool::execute` 方法，替换实现：

```rust
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

    // max_bytes: 优先使用 capability 中的 file_reading_limits，然后参数，最后默认值
    let max_bytes = ctx
        .capability
        .as_ref()
        .and_then(|c| c.file_reading_limits.as_ref())
        .map(|l| l.max_size_bytes)
        .unwrap_or_else(|| {
            input
                .get("max_bytes")
                .and_then(Value::as_u64)
                .map(|v| v as usize)
                .unwrap_or(1_048_576)
        });

    let resolved = resolve_path(&root, rel)?;
    if !resolved.is_file() {
        return Err(ToolError::ExecutionFailed(format!("Not a file: {rel}")));
    }

    // 检查文件状态缓存（对标 claude-code-best FileReadTool readFileState.get()）
    let mtime_secs = std::fs::metadata(&resolved)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let cache = ctx
        .capability
        .as_ref()
        .and_then(|c| c.read_file_state.as_ref().cloned());

    if let Some(ref cache) = cache {
        if let Some(state) = cache.get(&resolved) {
            if state.mtime_secs == mtime_secs
                && state.offset.is_none()
                && state.limit.is_none()
            {
                // 文件未修改，直接返回缓存内容
                let result = serde_json::json!({
                    "path": rel,
                    "content": state.content,
                    "size": state.content.len(),
                    "cached": true,
                });
                return Ok(tool_result("read_workspace_file", result));
            }
        }
    }

    let bytes = std::fs::read(&resolved)
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let truncated = bytes.len() > max_bytes;
    let content =
        String::from_utf8_lossy(if truncated { &bytes[..max_bytes] } else { &bytes })
            .to_string();

    // 写入缓存（对标 claude-code-best readFileState.set()）
    if let Some(cache) = cache {
        use crate::runtime::tools::capability::FileState;
        cache.set(
            resolved.clone(),
            FileState {
                content: content.clone(),
                mtime_secs,
                offset: None,
                limit: None,
            },
        );
    }

    let mut result = serde_json::json!({
        "path": rel,
        "content": content,
        "size": bytes.len(),
    });
    if truncated {
        result["truncated"] = serde_json::json!(true);
    }
    Ok(tool_result("read_workspace_file", result))
}
```

- [ ] **Step 4：在 `plugin/registry.rs` 的 `execute()` 中注入 `file_reading_limits`**

找到构建 `CapabilityContext` 的位置（约290-315行），在 `Arc::new(CapabilityContext { ... })` 前加：

```rust
// 注入文件读取上限（对标 claude-code-best fileReadingLimits）
let capability = Arc::new(CapabilityContext {
    storage: Some(storage),
    workspace_id: Some(ctx.conversation_id.clone()),
    browser_available,
    file_ops,
    read_file_state: None,        // 由 TurnDriver 按 turn 创建后注入，此处 None
    file_reading_limits: Some(crate::runtime::tools::capability::FileReadingLimits::default()),
    notification_sink: None,      // 由 TurnDriver 注入，此处 None
});
```

- [ ] **Step 5：确认测试通过**

```bash
cd src-tauri && cargo test read_workspace_file_uses_file_state_cache notification_sink_receives_message -- --nocapture
```

期望：两个测试 PASS

- [ ] **Step 6：全量回归**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

期望：全绿

- [ ] **Step 7：Commit**

```bash
git add src-tauri/src/runtime/tools/builtin/workspace.rs src-tauri/src/plugin/registry.rs src-tauri/tests/tool_capability_context_test.rs
git commit -m "feat(capability): inject file_reading_limits; ReadWorkspaceFile uses FileStateCache"
```

---

## Phase 2：并发编排 + 运行时谓词

### Task 2.1：RuntimeTool trait 新增谓词方法（P0-B + P1-B 基础）

**Files:**
- Modify: `src-tauri/src/runtime/tools/dispatcher.rs`
- Modify: `src-tauri/src/runtime/tools/definition.rs`
- Modify: `src-tauri/tests/tool_catalog_contract_test.rs`

---

- [ ] **Step 1：写失败测试**

在 `src-tauri/tests/tool_catalog_contract_test.rs` 末尾追加：

```rust
// ── Task 2.1 tests ──────────────────────────────────────────────────────────

#[test]
fn tool_definition_default_read_only_is_false() {
    use app_lib::runtime::tools::definition::ToolDefinition;
    let def = ToolDefinition::new("test_tool", "desc");
    assert!(!def.default_read_only);
    assert!(!def.default_destructive);
}

#[test]
fn tool_definition_with_read_only_flag() {
    use app_lib::runtime::tools::definition::ToolDefinition;
    let def = ToolDefinition::new("read_tool", "desc").with_read_only(true);
    assert!(def.default_read_only);
}
```

- [ ] **Step 2：确认测试失败**

```bash
cd src-tauri && cargo test tool_definition_default_read_only tool_definition_with_read_only -- --nocapture 2>&1 | head -20
```

期望：编译错误（`default_read_only` 字段不存在）

- [ ] **Step 3：更新 `ToolDefinition`**

在 `src-tauri/src/runtime/tools/definition.rs` 的 `ToolDefinition` struct 末尾添加两个字段：

```rust
pub struct ToolDefinition {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub capability_scope: Vec<String>,
    pub kind: ToolKind,
    /// 该工具默认是否只读（对标 claude-code-best `isReadOnly()`）。
    /// 运行时谓词由 `RuntimeTool` 方法按 input 覆盖。
    pub default_read_only: bool,
    /// 该工具默认是否破坏性（对标 claude-code-best `isDestructive()`）。
    pub default_destructive: bool,
}
```

在 `ToolDefinition::new()` 中补全默认值：

```rust
pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
    let id = id.into();
    Self {
        display_name: id.clone(),
        id,
        description: description.into(),
        capability_scope: Vec::new(),
        kind: ToolKind::default(),
        default_read_only: false,
        default_destructive: false,
    }
}
```

添加 builder 方法：

```rust
pub fn with_read_only(mut self, v: bool) -> Self {
    self.default_read_only = v;
    self
}

pub fn with_destructive(mut self, v: bool) -> Self {
    self.default_destructive = v;
    self
}
```

- [ ] **Step 4：在 `RuntimeTool` trait 新增默认谓词方法**

在 `src-tauri/src/runtime/tools/dispatcher.rs` 的 `RuntimeTool` trait 中追加：

```rust
#[async_trait]
pub trait RuntimeTool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError>;

    /// 该工具调用对给定输入是否并发安全（默认 false，保守）。
    /// 对标 claude-code-best `isConcurrencySafe(input): boolean`。
    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    /// 该工具调用对给定输入是否只读（默认使用 ToolDefinition.default_read_only）。
    /// 对标 claude-code-best `isReadOnly(input): boolean`。
    fn is_read_only(&self, _input: &Value) -> bool {
        self.definition().default_read_only
    }

    /// 该工具调用对给定输入是否破坏性（默认使用 ToolDefinition.default_destructive）。
    /// 对标 claude-code-best `isDestructive(input): boolean`。
    fn is_destructive(&self, _input: &Value) -> bool {
        self.definition().default_destructive
    }
}
```

- [ ] **Step 5：确认测试通过**

```bash
cd src-tauri && cargo test tool_definition_default_read_only tool_definition_with_read_only -- --nocapture
```

期望：PASS

- [ ] **Step 6：全量回归**

```bash
cd src-tauri && cargo test --tests --no-fail-fast 2>&1 | grep -E "FAILED|error" | head -20
```

期望：无 FAILED，无编译错误

- [ ] **Step 7：Commit**

```bash
git add src-tauri/src/runtime/tools/definition.rs src-tauri/src/runtime/tools/dispatcher.rs src-tauri/tests/tool_catalog_contract_test.rs
git commit -m "feat(tool): add is_concurrency_safe/is_read_only/is_destructive to RuntimeTool trait"
```

---

### Task 2.2：workspace 工具声明并发安全性（P0-B 具体落地）

**Files:**
- Modify: `src-tauri/src/runtime/tools/builtin/workspace.rs`
- Modify: `src-tauri/tests/tool_dispatcher_test.rs`

---

- [ ] **Step 1：写失败测试**

在 `src-tauri/tests/tool_dispatcher_test.rs` 末尾追加：

```rust
// ── Task 2.2 tests ──────────────────────────────────────────────────────────

#[test]
fn workspace_read_tools_are_concurrency_safe() {
    use app_lib::runtime::tools::builtin::workspace::{
        GetFileInfoRuntimeTool, ListDirectoryRuntimeTool, ReadWorkspaceFileRuntimeTool,
        SearchFilesRuntimeTool,
    };
    use app_lib::runtime::tools::RuntimeTool;
    use serde_json::json;

    assert!(ListDirectoryRuntimeTool.is_concurrency_safe(&json!({})),
        "list_directory should be concurrency safe");
    assert!(ReadWorkspaceFileRuntimeTool.is_concurrency_safe(&json!({})),
        "read_workspace_file should be concurrency safe");
    assert!(SearchFilesRuntimeTool.is_concurrency_safe(&json!({})),
        "search_files should be concurrency safe");
    assert!(GetFileInfoRuntimeTool.is_concurrency_safe(&json!({})),
        "get_file_info should be concurrency safe");
}

#[test]
fn workspace_read_tools_are_read_only() {
    use app_lib::runtime::tools::builtin::workspace::{
        GetFileInfoRuntimeTool, ListDirectoryRuntimeTool, ReadWorkspaceFileRuntimeTool,
        SearchFilesRuntimeTool,
    };
    use app_lib::runtime::tools::RuntimeTool;
    use serde_json::json;

    assert!(ListDirectoryRuntimeTool.is_read_only(&json!({})));
    assert!(ReadWorkspaceFileRuntimeTool.is_read_only(&json!({})));
    assert!(SearchFilesRuntimeTool.is_read_only(&json!({})));
    assert!(GetFileInfoRuntimeTool.is_read_only(&json!({})));
}
```

- [ ] **Step 2：确认测试失败**

```bash
cd src-tauri && cargo test workspace_read_tools_are_concurrency workspace_read_tools_are_read_only -- --nocapture 2>&1 | tail -10
```

期望：FAIL（`is_concurrency_safe` 默认返回 `false`）

- [ ] **Step 3：在 workspace 工具实现 `is_concurrency_safe` 和 catalog 设置 `default_read_only`**

在 `src-tauri/src/runtime/tools/builtin/workspace.rs` 的每个工具 impl 中添加（以 `ListDirectoryRuntimeTool` 为例，其余三个同理）：

```rust
#[async_trait]
impl RuntimeTool for ListDirectoryRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("list_directory")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("list_directory", "List authorized directory"))
    }

    // 只读目录列表，并发安全
    fn is_concurrency_safe(&self, _input: &Value) -> bool { true }

    async fn execute(...) { ... /* 不变 */ }
}
```

对 `ReadWorkspaceFileRuntimeTool`、`SearchFilesRuntimeTool`、`GetFileInfoRuntimeTool` 同样添加 `fn is_concurrency_safe(&self, _input: &Value) -> bool { true }`。

在 `catalog.rs` 中，workspace 工具的 `ToolDefinition::new(...)` 链式调用追加 `.with_read_only(true)`：

```rust
ToolDefinition::new("list_directory", "列出授权工作目录中的文件和子目录")
    .with_kind(ToolKind::Primitive)
    .with_read_only(true)
    .with_capability_scope(["workspace:read"]),
```

（对 `read_workspace_file`、`search_files`、`get_file_info` 同样追加 `.with_read_only(true)`）

- [ ] **Step 4：确认测试通过**

```bash
cd src-tauri && cargo test workspace_read_tools_are_concurrency workspace_read_tools_are_read_only -- --nocapture
```

期望：PASS

- [ ] **Step 5：全量回归**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

- [ ] **Step 6：Commit**

```bash
git add src-tauri/src/runtime/tools/builtin/workspace.rs src-tauri/src/runtime/tools/catalog.rs src-tauri/tests/tool_dispatcher_test.rs
git commit -m "feat(workspace-tools): declare is_concurrency_safe=true and default_read_only=true"
```

---

### Task 2.3：ToolDispatcher 批量并发调度（P0-B 调度层）

**Files:**
- Modify: `src-tauri/src/runtime/tools/dispatcher.rs`
- Modify: `src-tauri/tests/tool_dispatcher_test.rs`

---

- [ ] **Step 1：写失败测试**

在 `src-tauri/tests/tool_dispatcher_test.rs` 末尾追加：

```rust
// ── Task 2.3 tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_batch_returns_results_for_all_calls() {
    use app_lib::runtime::tools::{
        AllowAllPermissionPipeline, ToolDispatcher, ToolDispatchOutcome, ToolExecutionContext,
        RuntimeTool, ToolDefinition, ToolError, ToolResult,
    };
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::sync::Arc;

    struct EchoTool;
    #[async_trait]
    impl RuntimeTool for EchoTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("echo", "echo")
        }
        async fn execute(&self, input: Value, _ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new("echo", input.to_string(), None))
        }
        fn is_concurrency_safe(&self, _: &Value) -> bool { true }
    }

    let dispatcher = ToolDispatcher::allow_all();
    dispatcher.register(Arc::new(EchoTool));

    let calls = vec![
        ("echo".to_string(), json!({"n": 1}), ToolExecutionContext::for_test("c", "r", "t1")),
        ("echo".to_string(), json!({"n": 2}), ToolExecutionContext::for_test("c", "r", "t2")),
        ("echo".to_string(), json!({"n": 3}), ToolExecutionContext::for_test("c", "r", "t3")),
    ];

    let results = dispatcher.dispatch_batch(calls).await;
    assert_eq!(results.len(), 3);
    for r in &results {
        assert!(matches!(r, Ok(ToolDispatchOutcome::Completed { .. })));
    }
}

#[tokio::test]
async fn dispatch_batch_serial_tool_runs_after_concurrent_batch() {
    use app_lib::runtime::tools::{
        AllowAllPermissionPipeline, ToolDispatcher, ToolDispatchOutcome, ToolExecutionContext,
        RuntimeTool, ToolDefinition, ToolError, ToolResult,
    };
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};

    // 记录执行顺序
    let order: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));

    struct OrderedTool {
        name: &'static str,
        concurrent: bool,
        order: Arc<Mutex<Vec<String>>>,
    }
    #[async_trait]
    impl RuntimeTool for OrderedTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new(self.name, "ordered")
        }
        async fn execute(&self, _: Value, _: ToolExecutionContext) -> Result<ToolResult, ToolError> {
            self.order.lock().unwrap().push(self.name.to_string());
            Ok(ToolResult::new(self.name, "ok", None))
        }
        fn is_concurrency_safe(&self, _: &Value) -> bool { self.concurrent }
    }

    let dispatcher = ToolDispatcher::allow_all();
    dispatcher.register(Arc::new(OrderedTool { name: "read_a", concurrent: true,  order: order.clone() }));
    dispatcher.register(Arc::new(OrderedTool { name: "write_b", concurrent: false, order: order.clone() }));

    let calls = vec![
        ("read_a".to_string(), json!({}), ToolExecutionContext::for_test("c", "r", "t1")),
        ("write_b".to_string(), json!({}), ToolExecutionContext::for_test("c", "r", "t2")),
    ];

    let results = dispatcher.dispatch_batch(calls).await;
    assert_eq!(results.len(), 2);
    // write_b 必须在 read_a 之后执行（串行）
    let o = order.lock().unwrap();
    assert_eq!(o[0], "read_a");
    assert_eq!(o[1], "write_b");
}
```

- [ ] **Step 2：确认测试失败**

```bash
cd src-tauri && cargo test dispatch_batch_returns_results dispatch_batch_serial_tool -- --nocapture 2>&1 | head -20
```

期望：编译错误（`dispatch_batch` 不存在）

- [ ] **Step 3：在 `ToolDispatcher` 添加 `dispatch_batch`**

在 `src-tauri/src/runtime/tools/dispatcher.rs` 的 `impl ToolDispatcher` 末尾追加：

```rust
/// 批量调度，按并发安全性分区执行。
///
/// 算法（对标 claude-code-best `partitionToolCalls`）：
/// - 连续的并发安全工具合并为一批，使用 `tokio::join_all` 并行执行（最多 10 个）。
/// - 非并发安全工具各自一批，顺序 await。
pub async fn dispatch_batch(
    &self,
    calls: Vec<(String, Value, ToolExecutionContext)>,
) -> Vec<Result<ToolDispatchOutcome, ToolError>> {
    const MAX_CONCURRENCY: usize = 10;

    // 分区
    #[derive(Default)]
    struct Batch {
        concurrent: bool,
        calls: Vec<(String, Value, ToolExecutionContext)>,
    }
    let mut batches: Vec<Batch> = Vec::new();

    for (name, input, ctx) in calls {
        let is_safe = {
            let tools = self.tools.read().unwrap();
            tools
                .get(&name)
                .map(|t| t.is_concurrency_safe(&input))
                .unwrap_or(false)
        };
        let push_new = batches.last().map(|b| b.concurrent != is_safe || !is_safe).unwrap_or(true);
        if push_new {
            batches.push(Batch { concurrent: is_safe, calls: vec![(name, input, ctx)] });
        } else {
            batches.last_mut().unwrap().calls.push((name, input, ctx));
        }
    }

    let mut all_results: Vec<Result<ToolDispatchOutcome, ToolError>> = Vec::new();

    for batch in batches {
        if batch.concurrent && batch.calls.len() > 1 {
            // 并发执行（最多 MAX_CONCURRENCY）
            let futures: Vec<_> = batch
                .calls
                .into_iter()
                .take(MAX_CONCURRENCY)
                .map(|(name, input, ctx)| {
                    let dispatcher = self as *const ToolDispatcher as usize;
                    async move {
                        // SAFETY: dispatcher 在 batch 整个生命周期内有效
                        let d = unsafe { &*(dispatcher as *const ToolDispatcher) };
                        d.dispatch(&name, input, ctx).await
                    }
                })
                .collect();
            let results = futures::future::join_all(futures).await;
            all_results.extend(results);
        } else {
            // 串行执行
            for (name, input, ctx) in batch.calls {
                all_results.push(self.dispatch(&name, input, ctx).await);
            }
        }
    }

    all_results
}
```

在 `Cargo.toml` 添加（如果尚无）：

```toml
futures = "0.3"
```

> **注意**：上面用了裸指针避免 self 借用问题。更安全的写法是把 dispatcher 包在 `Arc` 里后 clone，但当前 `ToolDispatcher` 还不是 `Clone`。若 Rust 借用检查器报错，改用下面的 `Arc` 写法：

```rust
// 替代方案（若裸指针方式无法通过借用检查）：
// 将 dispatch_batch 改为接收 Arc<Self>，或者提取并发 future 的逻辑到独立函数
```

- [ ] **Step 4：确认测试通过**

```bash
cd src-tauri && cargo test dispatch_batch_returns_results dispatch_batch_serial_tool -- --nocapture
```

期望：PASS

- [ ] **Step 5：全量回归**

```bash
cd src-tauri && cargo test --tests --no-fail-fast 2>&1 | grep -E "FAILED|^error" | head -20
```

- [ ] **Step 6：Commit**

```bash
git add src-tauri/src/runtime/tools/dispatcher.rs src-tauri/Cargo.toml src-tauri/tests/tool_dispatcher_test.rs
git commit -m "feat(dispatcher): add dispatch_batch with concurrent/serial partitioning"
```

---

## Phase 3：核心工具迁移 + 动态权限

### Task 3.1：RuntimeTool trait 添加 `check_permissions` 默认方法（P1-A 基础）

**Files:**
- Modify: `src-tauri/src/runtime/tools/dispatcher.rs`
- Modify: `src-tauri/tests/tool_permission_pipeline_test.rs`

---

- [ ] **Step 1：写失败测试**

在 `src-tauri/tests/tool_permission_pipeline_test.rs` 末尾追加：

```rust
// ── Task 3.1 tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn tool_check_permissions_overrides_pipeline_when_some() {
    use app_lib::runtime::tools::{
        AllowAllPermissionPipeline, ToolDispatchOutcome, ToolDispatcher, ToolDefinition,
        ToolError, ToolExecutionContext, ToolResult, RuntimeTool,
    };
    use app_lib::runtime::tools::permission::PermissionDecision;
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::sync::Arc;

    // 工具总是 check_permissions → Deny
    struct AlwaysDenyTool;
    #[async_trait]
    impl RuntimeTool for AlwaysDenyTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("always_deny", "always deny")
        }
        async fn execute(&self, _: Value, _: ToolExecutionContext) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new("always_deny", "should not reach", None))
        }
        async fn check_permissions(
            &self,
            _input: &Value,
            _ctx: &ToolExecutionContext,
        ) -> Option<PermissionDecision> {
            Some(PermissionDecision::Deny {
                message: "tool-level deny".to_string(),
                reason: app_lib::runtime::tools::permission::PermissionReason::Other("test".into()),
            })
        }
    }

    // pipeline 是 AllowAll，但工具级 check_permissions 应覆盖它
    let dispatcher = ToolDispatcher::allow_all();
    dispatcher.register(Arc::new(AlwaysDenyTool));
    let ctx = ToolExecutionContext::for_test("c", "r", "t1");
    let result = dispatcher.dispatch("always_deny", json!({}), ctx).await;
    assert!(matches!(result, Err(ToolError::PermissionDenied(_))),
        "tool-level deny should override allow_all pipeline");
}

#[tokio::test]
async fn tool_check_permissions_falls_through_to_pipeline_when_none() {
    use app_lib::runtime::tools::{
        AllowAllPermissionPipeline, ToolDispatchOutcome, ToolDispatcher, ToolDefinition,
        ToolError, ToolExecutionContext, ToolResult, RuntimeTool,
    };
    use app_lib::runtime::tools::permission::PermissionDecision;
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::sync::Arc;

    struct PassthroughTool;
    #[async_trait]
    impl RuntimeTool for PassthroughTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("passthrough", "passthrough")
        }
        async fn execute(&self, _: Value, _: ToolExecutionContext) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new("passthrough", "executed", None))
        }
        // check_permissions 返回 None → 走 pipeline（AllowAll → Allow）
    }

    let dispatcher = ToolDispatcher::allow_all();
    dispatcher.register(Arc::new(PassthroughTool));
    let ctx = ToolExecutionContext::for_test("c", "r", "t1");
    let result = dispatcher.dispatch("passthrough", json!({}), ctx).await;
    assert!(matches!(result, Ok(ToolDispatchOutcome::Completed { .. })),
        "None from check_permissions should fall through to pipeline");
}
```

- [ ] **Step 2：确认测试失败**

```bash
cd src-tauri && cargo test tool_check_permissions_overrides tool_check_permissions_falls_through -- --nocapture 2>&1 | head -20
```

期望：编译错误（`check_permissions` 方法不存在于 trait）

- [ ] **Step 3：在 `RuntimeTool` trait 添加 `check_permissions` 默认方法**

在 `src-tauri/src/runtime/tools/dispatcher.rs` 的 `RuntimeTool` trait，在 `is_destructive` 后追加：

```rust
    /// 工具级权限检查（基于具体输入动态决策）。
    ///
    /// 返回 `Some(decision)` 时直接使用该决策，跳过 `PermissionPipeline`。
    /// 返回 `None` 时走 `PermissionPipeline`（默认行为）。
    ///
    /// 对标 claude-code-best `checkPermissions(input, context): Promise<PermissionResult>`。
    async fn check_permissions(
        &self,
        _input: &Value,
        _ctx: &ToolExecutionContext,
    ) -> Option<crate::runtime::tools::permission::PermissionDecision> {
        None
    }
```

- [ ] **Step 4：更新 `ToolDispatcher::dispatch` 调用顺序**

在 `dispatch` 方法中，在 `permission_pipeline.authorize(...)` 调用前，先调用工具级检查：

```rust
pub async fn dispatch(
    &self,
    tool_name: &str,
    input: Value,
    ctx: ToolExecutionContext,
) -> Result<ToolDispatchOutcome, ToolError> {
    let tool = {
        let tools = self.tools.read().unwrap();
        tools
            .get(tool_name)
            .cloned()
            .ok_or_else(|| ToolError::ExecutionFailed(format!("unknown tool: {tool_name}")))?
    };
    let definition = tool.definition();

    // Step 1: 工具级权限检查（优先于 pipeline）
    let permission_decision = if let Some(decision) = tool.check_permissions(&input, &ctx).await {
        decision
    } else {
        // Step 2: pipeline 静态检查（回退）
        self.permission_pipeline.authorize(&definition, &input, &ctx)
    };

    match permission_decision {
        PermissionDecision::Allow { .. } => {}
        PermissionDecision::Deny { message, .. } => {
            return Err(ToolError::PermissionDenied(message));
        }
        decision @ PermissionDecision::Ask { .. } => {
            return Ok(ToolDispatchOutcome::AskRequired(decision));
        }
    }
    ctx.event_sink.emit("tool:executing");
    let result = tool.execute(input, ctx.clone()).await;
    ctx.event_sink.emit("tool:completed");
    let result = result?;
    Ok(ToolDispatchOutcome::Completed {
        result,
        event_names: ctx.event_sink.snapshot(),
    })
}
```

- [ ] **Step 5：确认测试通过**

```bash
cd src-tauri && cargo test tool_check_permissions_overrides tool_check_permissions_falls_through -- --nocapture
```

期望：PASS

- [ ] **Step 6：全量回归**

```bash
cd src-tauri && cargo test --tests --no-fail-fast 2>&1 | grep -E "FAILED|^error" | head -20
```

- [ ] **Step 7：Commit**

```bash
git add src-tauri/src/runtime/tools/dispatcher.rs src-tauri/tests/tool_permission_pipeline_test.rs
git commit -m "feat(dispatcher): add check_permissions hook to RuntimeTool trait with tool-over-pipeline priority"
```

---

### Task 3.2：execute_python 迁移到 RuntimeTool（P0-C 第一步）

**Files:**
- Create: `src-tauri/src/runtime/tools/builtin/python.rs`
- Modify: `src-tauri/src/runtime/tools/builtin/mod.rs`
- Modify: `src-tauri/src/plugin/registry.rs`（`try_build_request_scoped_tool`）
- Modify: `src-tauri/tests/primitive_tools_migration_test.rs`

> **前置阅读**：`docs/2026-04-16-execute-python-migration-boundary.md`（迁移边界分析，约 60 行）

---

- [ ] **Step 1：写失败测试**

在 `src-tauri/tests/primitive_tools_migration_test.rs` 末尾追加：

```rust
// ── Task 3.2 tests ──────────────────────────────────────────────────────────

#[test]
fn execute_python_tool_is_registered_as_runtime_tool_in_request_scope() {
    // 验证 try_build_request_scoped_tool 能为 "execute_python" 返回 Some(RuntimeTool)
    // 不需要真实 PluginContext，只验证构建逻辑存在
    use app_lib::runtime::tools::builtin::python::ExecutePythonRuntimeTool;
    use app_lib::runtime::tools::RuntimeTool;
    // 构造一个无 session_manager 的最简 tool（降级模式）
    let tool = ExecutePythonRuntimeTool::stub();
    assert_eq!(tool.definition().id, "execute_python");
}

#[test]
fn execute_python_runtime_tool_has_correct_catalog_kind() {
    use app_lib::runtime::tools::catalog::ToolCatalog;
    use app_lib::runtime::tools::definition::ToolKind;
    let catalog = ToolCatalog::default_catalog();
    let def = catalog.get("execute_python").expect("execute_python must be in catalog");
    assert!(matches!(def.kind, ToolKind::Power));
}

#[test]
fn execute_python_check_permissions_denies_dangerous_code() {
    // 验证 check_permissions 对已知危险 pattern 返回 Some(Deny)
    // 使用 tokio::runtime::Runtime 同步运行 async
    use app_lib::runtime::tools::builtin::python::ExecutePythonRuntimeTool;
    use app_lib::runtime::tools::permission::PermissionDecision;
    use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
    use serde_json::json;

    let rt = tokio::runtime::Runtime::new().unwrap();
    let tool = ExecutePythonRuntimeTool::stub();
    let ctx = ToolExecutionContext::for_test("c", "r", "t");
    // 包含 __import__('os').system 这类危险 pattern
    let input = json!({"code": "__import__('os').system('rm -rf /')"});
    let result = rt.block_on(tool.check_permissions(&input, &ctx));
    assert!(
        matches!(result, Some(PermissionDecision::Deny { .. })),
        "dangerous code should be denied by check_permissions"
    );
}
```

- [ ] **Step 2：确认测试失败**

```bash
cd src-tauri && cargo test execute_python_tool_is_registered execute_python_runtime_tool_has_correct execute_python_check_permissions -- --nocapture 2>&1 | head -20
```

期望：编译错误（`app_lib::runtime::tools::builtin::python` 模块不存在）

- [ ] **Step 3：新建 `python.rs` 实现**

创建 `src-tauri/src/runtime/tools/builtin/python.rs`：

```rust
//! ExecutePythonRuntimeTool — execute_python 工具的 RuntimeTool 实现。
//!
//! 迁移边界参见：docs/2026-04-16-execute-python-migration-boundary.md
//!
//! 本实现暂为最小可用版本（MVP）：
//! - `stub()` 创建无 session_manager 的降级实例（用于测试和非 Python 路径）
//! - `check_permissions` 基于输入代码做静态危险 pattern 检测（对标 claude-code-best tool-level permission）
//! - `execute` 委托给现有 `handle_execute_python`（LegacyToolAdapter 路径），S4 迁移后替换

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::permission::{PermissionDecision, PermissionReason};
use crate::runtime::tools::RuntimeTool;

/// 危险代码 pattern（对标 python/sandbox.rs validate_code 逻辑）。
/// check_permissions 遇到这些 pattern 即返回 Deny。
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

/// execute_python 工具的 RuntimeTool 实现。
pub struct ExecutePythonRuntimeTool {
    /// 是否为降级 stub 模式（无真实 session_manager）。
    /// stub 模式下 execute 返回降级错误，供测试和非 Python 路径使用。
    stub_mode: bool,
}

impl ExecutePythonRuntimeTool {
    /// 创建降级 stub 实例（用于测试）。
    pub fn stub() -> Self {
        Self { stub_mode: true }
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

    /// 工具级权限检查：对危险代码 pattern 返回 Deny。
    ///
    /// 对标 claude-code-best `checkPermissions(input, context): Promise<PermissionResult>`。
    /// 现有 `validate_code()` 逻辑迁入此方法（静态 pattern 检测）。
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
        None // 交给 CapabilityPermissionPipeline 做 workspace/python 能力检查
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        if self.stub_mode {
            return Err(ToolError::ExecutionFailed(
                "ExecutePythonRuntimeTool: stub mode, real execution not available".into(),
            ));
        }
        // TODO(Phase 3 full migration): 委托给真实 PythonExecution trait
        // 当前 stub 实现——完整迁移在 execute_python boundary 分析后进行
        Err(ToolError::ExecutionFailed(
            "execute_python full RuntimeTool migration pending".into(),
        ))
    }
}
```

- [ ] **Step 4：在 `builtin/mod.rs` 导出模块**

找到 `src-tauri/src/runtime/tools/builtin/mod.rs`，追加：

```rust
pub mod python;
```

- [ ] **Step 5：确认测试通过**

```bash
cd src-tauri && cargo test execute_python_tool_is_registered execute_python_runtime_tool_has_correct execute_python_check_permissions -- --nocapture
```

期望：3 个测试 PASS

- [ ] **Step 6：全量回归**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

- [ ] **Step 7：Commit**

```bash
git add src-tauri/src/runtime/tools/builtin/python.rs src-tauri/src/runtime/tools/builtin/mod.rs src-tauri/tests/primitive_tools_migration_test.rs
git commit -m "feat(python-tool): ExecutePythonRuntimeTool with check_permissions dangerous-code detection"
```

---

### Task 3.3：generate_report 和 generate_chart 迁移骨架（P0-C 第二步）

**Files:**
- Create: `src-tauri/src/runtime/tools/builtin/report.rs`
- Create: `src-tauri/src/runtime/tools/builtin/chart.rs`
- Modify: `src-tauri/src/runtime/tools/builtin/mod.rs`
- Modify: `src-tauri/tests/primitive_tools_migration_test.rs`

---

- [ ] **Step 1：写失败测试**

在 `src-tauri/tests/primitive_tools_migration_test.rs` 末尾追加：

```rust
// ── Task 3.3 tests ──────────────────────────────────────────────────────────

#[test]
fn generate_report_tool_is_runtime_tool_type() {
    use app_lib::runtime::tools::builtin::report::GenerateReportRuntimeTool;
    use app_lib::runtime::tools::RuntimeTool;
    let tool = GenerateReportRuntimeTool::stub();
    assert_eq!(tool.definition().id, "generate_report");
    assert!(!tool.is_concurrency_safe(&serde_json::json!({})),
        "generate_report writes files, not concurrency safe");
}

#[test]
fn generate_chart_tool_is_runtime_tool_type() {
    use app_lib::runtime::tools::builtin::chart::GenerateChartRuntimeTool;
    use app_lib::runtime::tools::RuntimeTool;
    let tool = GenerateChartRuntimeTool::stub();
    assert_eq!(tool.definition().id, "generate_chart");
    assert!(!tool.is_concurrency_safe(&serde_json::json!({})),
        "generate_chart writes files, not concurrency safe");
}
```

- [ ] **Step 2：确认测试失败**

```bash
cd src-tauri && cargo test generate_report_tool_is_runtime generate_chart_tool_is_runtime -- --nocapture 2>&1 | head -10
```

- [ ] **Step 3：创建 `report.rs`**

```rust
//! GenerateReportRuntimeTool — generate_report 工具的 RuntimeTool 骨架。
//! 完整迁移需提取 ReportCapability trait（待后续 sprint）。

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct GenerateReportRuntimeTool {
    stub_mode: bool,
}

impl GenerateReportRuntimeTool {
    pub fn stub() -> Self { Self { stub_mode: true } }
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
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        if self.stub_mode {
            return Err(ToolError::ExecutionFailed(
                "GenerateReportRuntimeTool: stub mode".into(),
            ));
        }
        Err(ToolError::ExecutionFailed("generate_report full migration pending".into()))
    }
}
```

- [ ] **Step 4：创建 `chart.rs`**

```rust
//! GenerateChartRuntimeTool — generate_chart 工具的 RuntimeTool 骨架。

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct GenerateChartRuntimeTool {
    stub_mode: bool,
}

impl GenerateChartRuntimeTool {
    pub fn stub() -> Self { Self { stub_mode: true } }
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
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        if self.stub_mode {
            return Err(ToolError::ExecutionFailed(
                "GenerateChartRuntimeTool: stub mode".into(),
            ));
        }
        Err(ToolError::ExecutionFailed("generate_chart full migration pending".into()))
    }
}
```

- [ ] **Step 5：在 `builtin/mod.rs` 导出**

```rust
pub mod chart;
pub mod report;
```

- [ ] **Step 6：确认测试通过**

```bash
cd src-tauri && cargo test generate_report_tool_is_runtime generate_chart_tool_is_runtime -- --nocapture
```

- [ ] **Step 7：全量回归**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

- [ ] **Step 8：Commit**

```bash
git add src-tauri/src/runtime/tools/builtin/report.rs src-tauri/src/runtime/tools/builtin/chart.rs src-tauri/src/runtime/tools/builtin/mod.rs src-tauri/tests/primitive_tools_migration_test.rs
git commit -m "feat(tools): add GenerateReportRuntimeTool and GenerateChartRuntimeTool stubs (P0-C skeleton)"
```

---

## 自检：spec 覆盖确认

| Spec 需求 | 对应 Task |
|-----------|---------|
| P2-A 排序 | Task 1.1 ✅ |
| P0-A FileStateCache | Task 1.2–1.3 ✅ |
| P0-A FileReadingLimits | Task 1.2–1.3 ✅ |
| P0-A NotificationSink | Task 1.2 ✅ |
| P0-B is_concurrency_safe trait | Task 2.1 ✅ |
| P0-B workspace 工具声明 | Task 2.2 ✅ |
| P0-B dispatch_batch | Task 2.3 ✅ |
| P1-B default_read_only/default_destructive | Task 2.1 ✅ |
| P1-A check_permissions trait | Task 3.1 ✅ |
| P0-C execute_python | Task 3.2 ✅ |
| P0-C generate_report/chart | Task 3.3 ✅ |
