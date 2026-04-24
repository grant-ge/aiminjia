# Memory 写回闭环 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 memory 写回闭环——LLM 能在对话中主动保存和搜索项目记忆，同时清退 legacy memory 代码，并为前端补充 TypeScript 封装。

**Architecture:** 新建 `WriteMemoryRuntimeTool` 和 `SearchMemoryRuntimeTool` 两个 RuntimeTool，注入 `AiJiaHome` 路径（依赖注入模式对标 `WebSearchRuntimeTool` 的 `SearchDeps`），通过 `ProjectMemoryService` 读写记忆文件；同时在 catalog 注册工具定义，在 `DAILY_ALLOWED_TOOLS` 中启用，并为 system prompt 新增 `memory-mechanics` section 指导模型写作格式。Legacy `plugin/builtin/tools/memory_*.rs` 和 `llm/tool_executor/memory.rs` 的 re-export 在最后一步删除。对标 `claude-code-best` 的 `src/memdir/memoryTypes.ts` 格式规范。

**Tech Stack:** Rust, async-trait, serde_json, `ProjectMemoryService`（已有），`AiJiaHome`（已有）

**Spec:** `docs/2026-04-22-memory-gap-vs-claude-code-best.md` 阶段一 T1-T4

> **执行补充（2026-04-24）**
> 为完成 Task 6 的更广 `review_ --tests` 验证，本次一并同步了几条仓库内陈旧 review 测试，使其对齐当前 runtime owner 与数据结构：
> - `review_query_engine_permission_store_injection_test` 改为校验 `SessionRuntime::with_permission_store(...)`
> - `review_worker_ask_control_plane_test` / `review_worker_run_config_permission_mode_test` 改为校验当前 `RuntimeChatTurnDriver` / `WorkerRunConfig` owner 边界
> - `s4_driver_loop_test` 补齐 `RuntimeToolCallOutcome::Completed` 的 `skill_runtime_patch` 字段
>
> 这些调整不改变 memory phase1 设计，只是为了让仓库级 review 回归与当前架构保持一致。

---

## 文件结构

| 操作 | 文件 | 说明 |
|------|------|------|
| 新建 | `src-tauri/src/runtime/tools/builtin/memory.rs` | `WriteMemoryRuntimeTool` + `SearchMemoryRuntimeTool` + `MemoryDeps` |
| 修改 | `src-tauri/src/runtime/tools/builtin/mod.rs` | 新增 `pub mod memory;` |
| 修改 | `src-tauri/src/runtime/tools/catalog.rs` | 新增 `write_memory` / `search_memory` CatalogEntry；`DAILY_ALLOWED_TOOLS` 追加两项 |
| 修改 | `src-tauri/src/plugin/registry.rs` | 将 `write_memory` / `search_memory` 加入 `REQUEST_SCOPED_RUNTIME_TOOL_NAMES` 并在 `try_build_request_scoped_tool()` 中构建 `MemoryDeps` |
| 修改 | `src-tauri/tests/daily_mode_tool_surface_test.rs` | 把 `search_memory` 从“已退场”断言中移出，改为验证 runtime memory tool surface |
| 修改 | `src-tauri/tests/skill_tool_contract_test.rs` | 同步 `DAILY_ALLOWED_TOOLS` 契约常量，追加 `write_memory` / `search_memory` |
| 修改 | `src-tauri/src/llm/prompts.rs` | 新增 `MEMORY_MECHANICS_SECTION` 常量，注入 Daily 模式 system prompt |
| 删除（最后） | `src-tauri/src/plugin/builtin/tools/memory_save.rs` 等 4 个文件 | legacy ToolPlugin 文件整体删除 |
| 修改（最后） | `src-tauri/src/llm/tool_executor/mod.rs` | 移除 `memory` 子模块的 re-export（第 66-69 行） |
| 修改 | `src/lib/tauri.ts` | 新增 `saveProjectMemory` / `distillProjectMemory` 两个 invoke 封装 |

---

## Task 1：新增 `write_memory` / `search_memory` Catalog 条目

**Files:**
- Modify: `src-tauri/src/runtime/tools/catalog.rs`（在 `build_default_catalog()` 末尾、`DAILY_ALLOWED_TOOLS` 定义之前追加）

- [x] **Step 1：在 `build_default_catalog()` 中追加两条 CatalogEntry**

在 `src-tauri/src/runtime/tools/catalog.rs` 的 `build_default_catalog()` 函数末尾（第 613 行 `c` 返回之前）插入：

```rust
    // ── Support: memory tools ───────────────────────────────────────────────
    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "write_memory",
            "保存一条项目记忆到本地记忆库。记忆按 workspace 分桶存储，跨对话持久化。\
            \n\n类型说明：\
            \n- user_preference：用户偏好（如"喜欢用箱型图"）\
            \n- project_constraint：项目约束（如"不 mock 数据库"）\
            \n- reference_info：外部系统指针（如"Linear 项目 INGEST 追踪 pipeline bug"）\
            \n- feedback：AI 行为纠正或确认（如"单 PR 更好，已确认"）",
        )
        .with_kind(ToolKind::Support)
        json!({
            "type": "object",
            "required": ["name", "memory_type", "description", "content"],
            "properties": {
                "name": {
                    "type": "string",
                    "description": "记忆条目名称，简短唯一，用于索引（如 'user-prefers-boxplot'）"
                },
                "memory_type": {
                    "type": "string",
                    "enum": ["user_preference", "project_constraint", "reference_info", "feedback"],
                    "description": "记忆类型"
                },
                "description": {
                    "type": "string",
                    "description": "一句话描述，用于未来相关性判断（给召回系统看，不是给人看的摘要）"
                },
                "content": {
                    "type": "string",
                    "description": "记忆正文。feedback 类型建议包含：规则本体 + **Why:** 原因 + **How to apply:** 适用场景"
                }
            }
        }),
    ));

    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "search_memory",
            "在本地记忆库中按关键词搜索相关记忆条目，返回最多 5 条最相关结果。",
        )
        .with_kind(ToolKind::Support)
        .with_read_only(true)
        json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {
                    "type": "string",
                    "description": "搜索关键词或问题描述"
                }
            }
        }),
    ));
```

> **计划修订说明（权限层）**：Phase 1 不新增 `memory:read` / `memory:write` 自定义 capability scope。当前权限管线对未知 scope 默认 deny / ask，若直接引入新 scope 会把 runtime memory 工具误判为不可用。memory 工具本阶段沿用 Support tool 的默认权限路径，依赖 request-scoped `MemoryDeps` 访问项目记忆目录。

- [x] **Step 2：在 `DAILY_ALLOWED_TOOLS` 中追加两项**

找到 `DAILY_ALLOWED_TOOLS` 常量（第 631 行），在 `"grep_content"` 后追加：

```rust
pub const DAILY_ALLOWED_TOOLS: &[&str] = &[
    "bash",
    "read_workspace_file",
    "write_file",
    "edit_file",
    "list_directory",
    "search_files",
    "get_file_info",
    "grep_content",
    "write_memory",    // 新增
    "search_memory",   // 新增
];
```

- [x] **Step 2.5：同步现有 daily tool surface / skill 契约测试**

修改 `src-tauri/tests/daily_mode_tool_surface_test.rs`：

- 把 `search_memory` 从 `retired_memory_tools_are_not_in_daily_catalog` 的退场名单中移除
- 保留/更新 `runtime_memory_tools_are_in_catalog_and_daily_allowlist`，明确 `write_memory` / `search_memory` 应存在于 catalog 与 daily allowlist

修改 `src-tauri/tests/skill_tool_contract_test.rs`：

- 在本地 `DAILY_ALLOWED_TOOLS` 常量中追加 `write_memory` / `search_memory`
- 保持与 runtime 常量完全一致，避免契约测试误报

- [x] **Step 3：运行编译确认 catalog 无报错**

```bash
cd src-tauri && cargo check 2>&1 | head -30
```

Expected: 无 error（可能有 warning，可忽略）

- [ ] **Step 4：Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add src-tauri/src/runtime/tools/catalog.rs
git commit -m "feat(memory): add write_memory/search_memory to tool catalog and DAILY_ALLOWED_TOOLS"
```

---

## Task 2：新建 `memory.rs` RuntimeTool 实现

**Files:**
- Create: `src-tauri/src/runtime/tools/builtin/memory.rs`
- Modify: `src-tauri/src/runtime/tools/builtin/mod.rs`

- [x] **Step 1：新建 `src-tauri/src/runtime/tools/builtin/memory.rs`**

```rust
//! Memory RuntimeTools — write_memory and search_memory.
//!
//! Does NOT use PluginContext. AiJiaHome path is injected at construction
//! via `MemoryDeps` (same pattern as `SearchDeps` in network.rs).

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::runtime::project_memory::{
    ProjectMemoryEntryDraft, ProjectMemoryService, ProjectMemoryType,
};
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

// ── MemoryDeps ───────────────────────────────────────────────────────────────

/// Narrow memory dependencies injected at construction.
/// CapabilityContext is intentionally NOT extended with these fields.
pub struct MemoryDeps {
    /// Root of the AiJia data directory (e.g. ~/Library/Application Support/com.aijia.app).
    pub app_data_dir: std::path::PathBuf,
    /// Current workspace path (used to resolve the memory bucket).
    pub workspace_path: std::path::PathBuf,
}

// ── WriteMemoryRuntimeTool ───────────────────────────────────────────────────

pub struct WriteMemoryRuntimeTool {
    deps: Arc<MemoryDeps>,
}

impl WriteMemoryRuntimeTool {
    pub fn new(deps: MemoryDeps) -> Self {
        Self {
            deps: Arc::new(deps),
        }
    }
}

#[async_trait]
impl RuntimeTool for WriteMemoryRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("write_memory")
            .unwrap_or_else(|| ToolDefinition::new("write_memory", "保存项目记忆"))
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        false
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let name = input
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("missing 'name'".into()))?
            .to_string();

        let memory_type_str = input
            .get("memory_type")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("missing 'memory_type'".into()))?;

        let memory_type = parse_memory_type(memory_type_str)?;

        let description = input
            .get("description")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("missing 'description'".into()))?
            .to_string();

        let content = input
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("missing 'content'".into()))?
            .to_string();

        let draft = ProjectMemoryEntryDraft {
            memory_type,
            name: name.clone(),
            description,
            content,
            source: None,
        };

        let service =
            ProjectMemoryService::new(&self.deps.app_data_dir, &self.deps.workspace_path);
        let saved = service
            .save_memory(draft)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let result = json!({
            "status": "saved",
            "name": name,
            "path": saved.relative_path.display().to_string(),
        });

        Ok(ToolResult::new(
            "write_memory",
            serde_json::to_string_pretty(&result).unwrap_or_default(),
            Some(result),
        ))
    }
}

// ── SearchMemoryRuntimeTool ──────────────────────────────────────────────────

pub struct SearchMemoryRuntimeTool {
    deps: Arc<MemoryDeps>,
}

impl SearchMemoryRuntimeTool {
    pub fn new(deps: MemoryDeps) -> Self {
        Self {
            deps: Arc::new(deps),
        }
    }
}

#[async_trait]
impl RuntimeTool for SearchMemoryRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("search_memory")
            .unwrap_or_else(|| ToolDefinition::new("search_memory", "搜索项目记忆"))
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let query = input
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("missing 'query'".into()))?;

        let service =
            ProjectMemoryService::new(&self.deps.app_data_dir, &self.deps.workspace_path);
        let ctx = service
            .load_context(query)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let entries: Vec<Value> = ctx
            .recalled_entries
            .iter()
            .map(|e| {
                json!({
                    "name": e.name,
                    "type": e.memory_type.as_str(),
                    "description": e.description,
                    "content": e.content,
                })
            })
            .collect();

        let result = json!({
            "status": "ok",
            "count": entries.len(),
            "results": entries,
        });

        Ok(ToolResult::new(
            "search_memory",
            serde_json::to_string_pretty(&result).unwrap_or_default(),
            Some(result),
        ))
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn parse_memory_type(s: &str) -> Result<ProjectMemoryType, ToolError> {
    match s {
        "user_preference" => Ok(ProjectMemoryType::UserPreference),
        "project_constraint" => Ok(ProjectMemoryType::ProjectConstraint),
        "reference_info" => Ok(ProjectMemoryType::ReferenceInfo),
        "feedback" => Ok(ProjectMemoryType::Feedback),
        other => Err(ToolError::ExecutionFailed(format!(
            "unknown memory_type '{}'. Valid: user_preference, project_constraint, reference_info, feedback",
            other
        ))),
    }
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tools::context::ToolExecutionContext;
    use crate::runtime::ids::{RunId, SessionId, ToolCallId};
    use std::sync::Arc;

    fn make_deps(dir: &std::path::Path) -> MemoryDeps {
        let workspace = dir.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        MemoryDeps {
            app_data_dir: dir.to_path_buf(),
            workspace_path: workspace,
        }
    }

    fn make_ctx() -> ToolExecutionContext {
        ToolExecutionContext {
            session_id: SessionId::new("test-session"),
            run_id: RunId::new("test-run"),
            agent_id: None,
            tool_call_id: ToolCallId::new("test-call"),
            cancellation: crate::runtime::cancellation::CancellationToken::new(),
            event_sink: Arc::new(crate::runtime::tools::context::EventCollectingSink::default()),
            capability: None,
            permission_override: None,
            permission_mode: crate::runtime::tools::permission::PermissionMode::Default,
            permission_store: None,
            hook_registry: None,
        }
    }

    #[tokio::test]
    async fn test_write_memory_saves_entry() {
        let dir = tempfile::TempDir::new().unwrap();
        let deps = make_deps(dir.path());
        let tool = WriteMemoryRuntimeTool::new(deps);

        let input = json!({
            "name": "user-prefers-boxplot",
            "memory_type": "user_preference",
            "description": "用户偏好用箱型图展示薪资分布",
            "content": "用户明确表示喜欢用箱型图（box plot）展示薪资分布数据，不喜欢柱状图。"
        });

        let result = tool.execute(input, make_ctx()).await.unwrap();
        let parsed: Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["status"], "saved");
        assert_eq!(parsed["name"], "user-prefers-boxplot");
        assert!(parsed["path"].as_str().unwrap().ends_with(".md"));
    }

    #[tokio::test]
    async fn test_write_memory_invalid_type() {
        let dir = tempfile::TempDir::new().unwrap();
        let deps = make_deps(dir.path());
        let tool = WriteMemoryRuntimeTool::new(deps);

        let input = json!({
            "name": "test",
            "memory_type": "invalid_type",
            "description": "test",
            "content": "test content"
        });

        let result = tool.execute(input, make_ctx()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown memory_type"));
    }

    #[tokio::test]
    async fn test_search_memory_returns_results() {
        let dir = tempfile::TempDir::new().unwrap();

        // Write a memory first
        let write_deps = make_deps(dir.path());
        let write_tool = WriteMemoryRuntimeTool::new(write_deps);
        let write_input = json!({
            "name": "user-prefers-boxplot",
            "memory_type": "user_preference",
            "description": "用户偏好用箱型图展示薪资分布",
            "content": "用户明确表示喜欢用箱型图（box plot）展示薪资分布数据。"
        });
        write_tool.execute(write_input, make_ctx()).await.unwrap();

        // Search for it
        let search_deps = make_deps(dir.path());
        let search_tool = SearchMemoryRuntimeTool::new(search_deps);
        let search_input = json!({ "query": "boxplot 箱型图" });
        let result = search_tool.execute(search_input, make_ctx()).await.unwrap();
        let parsed: Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["status"], "ok");
        assert!(parsed["count"].as_i64().unwrap() > 0);
    }

    #[tokio::test]
    async fn test_search_memory_empty_results() {
        let dir = tempfile::TempDir::new().unwrap();
        let deps = make_deps(dir.path());
        let tool = SearchMemoryRuntimeTool::new(deps);

        let input = json!({ "query": "完全不相关的查询词语" });
        let result = tool.execute(input, make_ctx()).await.unwrap();
        let parsed: Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["status"], "ok");
        assert_eq!(parsed["count"], 0);
    }
}
```

- [x] **Step 2：在 `builtin/mod.rs` 中声明新模块**

在 `src-tauri/src/runtime/tools/builtin/mod.rs` 末尾追加：

```rust
pub mod memory;
```

- [x] **Step 3：编译检查**

```bash
cd src-tauri && cargo check 2>&1 | grep "^error" | head -20
```

Expected：无 error

- [x] **Step 4：运行新测试（此时会编译失败，因为 `ProjectMemoryType::Feedback` 尚未定义）**

```bash
cd src-tauri && cargo test runtime::tools::builtin::memory 2>&1 | tail -20
```

Expected：编译错误，提示 `Feedback` variant 不存在 — 这是预期的，下一步修复

---

## Task 3：为 `ProjectMemoryType` 添加 `Feedback` variant

**Files:**
- Modify: `src-tauri/src/runtime/project_memory.rs`（第 14-37 行）

- [x] **Step 1：先写测试（在 `project_memory.rs` 已有 test 模块或新建）**

在 `src-tauri/src/runtime/project_memory.rs` 末尾的 `#[cfg(test)]` 块中（如果没有则新增）添加：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_service(dir: &std::path::Path) -> ProjectMemoryService {
        let workspace = dir.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        ProjectMemoryService::new(dir, &workspace)
    }

    #[test]
    fn test_feedback_type_roundtrip() {
        let dir = TempDir::new().unwrap();
        let service = make_service(dir.path());

        let draft = ProjectMemoryEntryDraft {
            memory_type: ProjectMemoryType::Feedback,
            name: "no-mock-db".to_string(),
            description: "不 mock 数据库，上季度因 mock/prod 差异导致迁移失败".to_string(),
            content: "集成测试必须连接真实数据库，不使用 mock。\n\n**Why:** 上季度 mock 测试全部通过但生产迁移失败。\n**How to apply:** 任何涉及数据库的测试一律走真实连接。".to_string(),
            source: None,
        };

        let saved = service.save_memory(draft).unwrap();
        assert!(saved.path.exists());

        // reload and verify type preserved
        let ctx = service.load_context("mock database").unwrap();
        let entry = ctx.recalled_entries.iter().find(|e| e.name == "no-mock-db");
        assert!(entry.is_some(), "feedback entry should be recalled");
        assert_eq!(entry.unwrap().memory_type, ProjectMemoryType::Feedback);
    }
}
```

- [x] **Step 2：运行测试确认失败**

```bash
cd src-tauri && cargo test project_memory::tests::test_feedback_type_roundtrip 2>&1 | tail -10
```

Expected：编译错误，`Feedback` variant 不存在

- [x] **Step 3：在 `ProjectMemoryType` 枚举中添加 `Feedback` variant**

在 `src-tauri/src/runtime/project_memory.rs` 第 14-37 行，修改为：

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectMemoryType {
    UserPreference,
    ProjectConstraint,
    ReferenceInfo,
    Feedback,
}

impl ProjectMemoryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UserPreference => "user_preference",
            Self::ProjectConstraint => "project_constraint",
            Self::ReferenceInfo => "reference_info",
            Self::Feedback => "feedback",
        }
    }

    fn from_str(raw: &str) -> Option<Self> {
        match raw.trim() {
            "user_preference" => Some(Self::UserPreference),
            "project_constraint" => Some(Self::ProjectConstraint),
            "reference_info" => Some(Self::ReferenceInfo),
            "feedback" => Some(Self::Feedback),
            _ => None,
        }
    }
}
```

同时给 `ProjectMemoryEntryDraft` 增加：

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMemoryEntryDraft {
    pub memory_type: ProjectMemoryType,
    pub name: String,
    pub description: String,
    pub content: String,
    pub source: Option<String>,
}
```

> 说明：这样前端 `invoke('save_project_memory', { memory: { memoryType: 'feedback', ... } })` 才能与 Rust 结构体正确对齐。

- [x] **Step 4：运行测试确认通过**

```bash
cd src-tauri && cargo test project_memory::tests::test_feedback_type_roundtrip -- --nocapture
```

Expected：`test project_memory::tests::test_feedback_type_roundtrip ... ok`

- [x] **Step 5：运行 Task 2 的 memory tool 测试**

```bash
cd src-tauri && cargo test runtime::tools::builtin::memory -- --nocapture
```

Expected：4 个 test 全部 ok

- [ ] **Step 6：Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add src-tauri/src/runtime/project_memory.rs \
        src-tauri/src/runtime/tools/builtin/memory.rs \
        src-tauri/src/runtime/tools/builtin/mod.rs \
        src-tauri/src/runtime/tools/catalog.rs
git commit -m "feat(memory): add WriteMemoryRuntimeTool, SearchMemoryRuntimeTool, and Feedback type"
```

---

## Task 4：将 memory RuntimeTool 接入 request-scoped factory

> **计划修订说明（对标当前 runtime-first 架构）**：`workspace_path` 是 request-scoped 数据，不应塞进全局 `register_builtin_tools()`。沿用本仓库已存在的 `REQUEST_SCOPED_RUNTIME_TOOL_NAMES + try_build_request_scoped_tool()` 模式，才与 `web_search` / `load_file` / `execute_python` 的主路径一致。

**Files:**
- Modify: `src-tauri/src/plugin/registry.rs`（`REQUEST_SCOPED_RUNTIME_TOOL_NAMES` + `try_build_request_scoped_tool`）

- [x] **Step 1：把 `write_memory` / `search_memory` 加入 request-scoped 名单**

在 `src-tauri/src/plugin/registry.rs` 的 `REQUEST_SCOPED_RUNTIME_TOOL_NAMES` 常量中追加：

```rust
    "write_memory",
    "search_memory",
```

- [x] **Step 2：在 `try_build_request_scoped_tool()` 中添加 memory factory**

在 `src-tauri/src/plugin/registry.rs` 的 `match name { ... }` 中新增两个分支：

```rust
            "write_memory" => Some(Arc::new(builtin::memory::WriteMemoryRuntimeTool::new(
                builtin::memory::MemoryDeps {
                    app_data_dir: ctx.storage.base_dir().to_path_buf(),
                    workspace_path: ctx.workspace_path.clone(),
                },
            )) as Arc<dyn crate::runtime::tools::RuntimeTool>),
            "search_memory" => Some(Arc::new(builtin::memory::SearchMemoryRuntimeTool::new(
                builtin::memory::MemoryDeps {
                    app_data_dir: ctx.storage.base_dir().to_path_buf(),
                    workspace_path: ctx.workspace_path.clone(),
                },
            )) as Arc<dyn crate::runtime::tools::RuntimeTool>),
```

> 说明：这里直接复用 `RequestScopedRuntimeDeps` 已有的 `storage.base_dir()` 与 `workspace_path`，不改 `register_builtin_tools()` 的公共签名，也不改现有测试调用点。

- [x] **Step 3：编译检查**

```bash
cd src-tauri && cargo check 2>&1 | grep "^error" | head -20
```

Expected：无 error

- [x] **Step 4：运行 runtime registry 回归测试**

```bash
cd src-tauri && cargo test builtin_runtime_registration_test -- --nocapture 2>&1 | tail -30
```

Expected：现有 runtime registration 测试保持通过；若新增 memory request-scoped 回归测试，也应一并通过

- [ ] **Step 5：Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add src-tauri/src/plugin/registry.rs
git commit -m "feat(memory): expose write_memory/search_memory through request-scoped runtime factory"
```

---

## Task 5：新增 `memory-mechanics` system prompt section

**Files:**
- Modify: `src-tauri/src/llm/prompts.rs`

- [x] **Step 1：在 `prompts.rs` 中新增常量**

在 `TOOL_PREFERENCE_SECTION` 常量之后（第 29 行之后）插入：

```rust
/// Memory mechanics section — 告知模型如何使用 write_memory / search_memory 工具。
/// 对标 claude-code-best 的 memory-mechanics system prompt section。
const MEMORY_MECHANICS_SECTION: &str = r#"

【记忆管理】
你拥有跨对话持久化记忆的能力。以下情况应主动调用 write_memory：

**何时保存记忆**
- 用户明确说"记住"、"下次也这样"
- 用户纠正了你的行为（"不要这样"、"别 mock"）→ 保存为 feedback 类型
- 用户确认了一个非显而易见的方式（"对，就是这样"）→ 同样保存为 feedback 类型
- 了解到用户角色、技术背景、偏好 → 保存为 user_preference 类型
- 了解到项目约束、架构决策 → 保存为 project_constraint 类型
- 了解到外部系统指针（Linear 项目、Slack 频道等）→ 保存为 reference_info 类型

**不要保存**
- 可从当前代码或 git 历史实时推导的信息（文件路径、函数名等）
- 本次对话的临时状态

**feedback 类型写法**（规则 + Why + How to apply）：
```
集成测试必须连接真实数据库，不使用 mock。

**Why:** 上季度 mock 测试全部通过但生产迁移失败，之后约定禁止 mock DB。
**How to apply:** 任何涉及数据库 schema 或查询的测试都用真实连接，不接受 mock 替代。
```

**推荐记忆前验证（防漂移）**
记忆中提到的文件路径或函数名可能已过期。在基于记忆给出建议前：
- 文件路径：先确认文件是否仍然存在
- 函数名/变量：先在代码中确认是否仍然存在
用户说"忽略记忆"时，视 MEMORY.md 为空，不引用、不比较、不提及记忆内容。"#;
```

- [x] **Step 2：将 `MEMORY_MECHANICS_SECTION` 注入 Daily 模式的 `static_section`**

找到 `build_system_prompt_parts` 中构建 `static_section` 的位置，把：

```rust
let static_section = format!("{}{}", base, TOOL_PREFERENCE_SECTION);
```

改为：

```rust
let static_section = format!(
    "{}{}{}",
    base, TOOL_PREFERENCE_SECTION, MEMORY_MECHANICS_SECTION
);
```

> 说明：memory mechanics 属于稳定的 system section，不应放进会话级 `dynamic_section`，这样也更符合 prompt cache 目标。

- [x] **Step 3：编译并检查 prompt 内容**

```bash
cd src-tauri && cargo test prompts 2>&1 | tail -20
```

Expected：已有 prompt 测试通过

- [x] **Step 3.5：同步现有 prompt / compat 回归测试**

修改 `src-tauri/src/llm/prompts.rs` 里的现有回归测试：

- `test_tool_preference_section_omits_retired_tool_names` 不再把 `search_memory` 视为 retired 名称
- 若 `MEMORY_MECHANICS_SECTION` 放入 `static_section`，则允许 `static_section` 包含 `write_memory` / `search_memory`，但仍禁止出现 `save_memory` / `load_core_memory` / `distill_memories`

修改 `src-tauri/tests/plan_u4_memory_runtime_native_test.rs`：

- `u4_compat_tool_definition_helper_excludes_retired_memory_tools` 中，retired 名称改为 `save_memory` / `load_core_memory` / `distill_memories`
- 不再把 `search_memory` 当作 retired 名称

- [ ] **Step 4：Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add src-tauri/src/llm/prompts.rs
git commit -m "feat(memory): add memory-mechanics system prompt section with write guidance and drift defense"
```

---

## Task 6：清退 legacy memory 代码

> **前置条件**：Task 1-5 全部通过，新 RuntimeTool 路径已验证可用。

**Files:**
- Delete: `src-tauri/src/plugin/builtin/tools/memory_save.rs`
- Delete: `src-tauri/src/plugin/builtin/tools/memory_search.rs`
- Delete: `src-tauri/src/plugin/builtin/tools/memory_core.rs`
- Delete: `src-tauri/src/plugin/builtin/tools/memory_distill.rs`
- Modify: `src-tauri/src/plugin/builtin/tools/mod.rs`（移除 4 个 `pub mod` 声明）
- Modify: `src-tauri/src/llm/tool_executor/mod.rs`（移除 memory re-export，第 66-69 行）
- Modify: `src-tauri/plugins/labor-compliance/workflow.toml`
- Modify: `src-tauri/plugins/org-diagnosis/workflow.toml`
- Modify: `src-tauri/plugins/pa-maturity/workflow.toml`
- Modify: `src-tauri/plugins/perf-system-design/workflow.toml`
- Modify: `src-tauri/plugins/labor-compliance/prompts/step0.md`
- Modify: `src-tauri/plugins/org-diagnosis/prompts/step0.md`
- Modify: `src-tauri/plugins/pa-maturity/prompts/step0.md`
- Modify: `src-tauri/plugins/perf-system-design/prompts/step0.md`

- [x] **Step 1：先写一个回归测试确认 legacy 名称退场，但新 runtime 名称仍可见**

新建 `src-tauri/tests/review_memory_legacy_retired_test.rs`：

```rust
//! review_memory_legacy_retired — 验证 legacy memory ToolPlugin 已退场，
//! 同时新的 runtime-first memory 工具名仍然可见。

#[tokio::test]
async fn review_legacy_memory_tool_names_retired_but_runtime_names_available() {
    use app_lib::plugin::builtin::tools::register_builtin_tools;
    use app_lib::plugin::registry::ToolRegistry;

    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let names = registry
        .get_all_schemas()
        .await
        .into_iter()
        .map(|schema| schema.name)
        .collect::<Vec<_>>();

    for retired in ["save_memory", "load_core_memory", "distill_memories"] {
        assert!(
            !names.iter().any(|name| name == retired),
            "legacy memory tool '{}' must not remain visible in runtime schema surface",
            retired
        );
    }

    for current in ["write_memory", "search_memory"] {
        assert!(
            names.iter().any(|name| name == current),
            "runtime memory tool '{}' should stay visible in schema surface",
            current
        );
    }
}

#[test]
fn review_new_memory_tools_in_catalog() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    assert!(
        TOOL_CATALOG.get("write_memory").is_some(),
        "write_memory should be in TOOL_CATALOG"
    );
    assert!(
        TOOL_CATALOG.get("search_memory").is_some(),
        "search_memory should be in TOOL_CATALOG"
    );
}
```

- [x] **Step 2：运行回归测试（此时应通过，因为 legacy ToolPlugin 已禁用）**

```bash
cd src-tauri && cargo test --test review_memory_legacy_retired_test -- --nocapture
```

Expected：2 个测试 ok

- [x] **Step 3：把内置 workflow / prompt 中的 `save_memory` 切到 `write_memory`**

将以下 4 个 workflow 的 `tools_only = ["save_memory", "search_memory"]` 改为 `tools_only = ["write_memory", "search_memory"]`：

- `src-tauri/plugins/labor-compliance/workflow.toml`
- `src-tauri/plugins/org-diagnosis/workflow.toml`
- `src-tauri/plugins/pa-maturity/workflow.toml`
- `src-tauri/plugins/perf-system-design/workflow.toml`

并把以下 4 个 step0 prompt 中提到的 `save_memory` 文案改为 `write_memory`：

- `src-tauri/plugins/labor-compliance/prompts/step0.md`
- `src-tauri/plugins/org-diagnosis/prompts/step0.md`
- `src-tauri/plugins/pa-maturity/prompts/step0.md`
- `src-tauri/plugins/perf-system-design/prompts/step0.md`

- [x] **Step 4：从 `plugin/builtin/tools/mod.rs` 移除 4 个 mod 声明**

删除以下 4 行：

```rust
pub mod memory_core;
pub mod memory_distill;
pub mod memory_save;
pub mod memory_search;
```

- [x] **Step 5：删除 4 个 legacy 文件**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/builtin/tools
rm memory_save.rs memory_search.rs memory_core.rs memory_distill.rs
```

- [x] **Step 6：从 `llm/tool_executor/mod.rs` 移除 memory re-export（第 66-69 行）**

删除以下 4 行：

```rust
pub(crate) use memory::handle_distill_memories;
pub(crate) use memory::handle_load_core_memory;
pub(crate) use memory::handle_save_memory;
pub(crate) use memory::handle_search_memory;
```

同时删除 `mod.rs` 中的 `mod memory;` 声明（第 20 行）。

- [x] **Step 7：编译确认无报错**

```bash
cd src-tauri && cargo build 2>&1 | grep "^error" | head -20
```

Expected：无 error（如有 `handle_save_memory` 等被其他地方引用的编译错误，逐一排查并移除引用）

- [x] **Step 8：运行全套 review 测试**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -30
```

Expected：所有 review_ 测试通过，包括新增的 `review_legacy_memory_tools_not_registered`

- [ ] **Step 9：Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add src-tauri/src/plugin/builtin/tools/mod.rs \
        src-tauri/src/llm/tool_executor/mod.rs \
        src-tauri/tests/review_memory_legacy_retired_test.rs \
        src-tauri/plugins/labor-compliance/workflow.toml \
        src-tauri/plugins/org-diagnosis/workflow.toml \
        src-tauri/plugins/pa-maturity/workflow.toml \
        src-tauri/plugins/perf-system-design/workflow.toml \
        src-tauri/plugins/labor-compliance/prompts/step0.md \
        src-tauri/plugins/org-diagnosis/prompts/step0.md \
        src-tauri/plugins/pa-maturity/prompts/step0.md \
        src-tauri/plugins/perf-system-design/prompts/step0.md
git rm src-tauri/src/plugin/builtin/tools/memory_save.rs \
       src-tauri/src/plugin/builtin/tools/memory_search.rs \
       src-tauri/src/plugin/builtin/tools/memory_core.rs \
       src-tauri/src/plugin/builtin/tools/memory_distill.rs
git commit -m "feat(memory): retire legacy memory_* ToolPlugin files and tool_executor/memory re-exports"
```

---

## Task 7：前端 TypeScript 封装

**Files:**
- Modify: `src/lib/tauri.ts`（末尾追加）

- [x] **Step 1：在 `src/lib/tauri.ts` 末尾追加两个 invoke 封装**

```typescript
// ---------------------------------------------------------------------------
// Project Memory Commands
// ---------------------------------------------------------------------------

export interface ProjectMemoryEntryDraft {
  memoryType: 'user_preference' | 'project_constraint' | 'reference_info' | 'feedback'
  name: string
  description: string
  content: string
  source?: string
}

/** 保存一条 project memory 条目。返回相对路径字符串（如 "entries/foo-bar-abc123.md"）。 */
export async function saveProjectMemory(
  workspacePath: string,
  memory: ProjectMemoryEntryDraft,
): Promise<string> {
  return invoke<string>('save_project_memory', {
    workspacePath,
    memory,
  })
}

/** 重建当前 workspace 的 MEMORY.md 索引，返回条目总数。 */
export async function distillProjectMemory(workspacePath: string): Promise<number> {
  return invoke<number>('distill_project_memory', { workspacePath })
}
```

- [x] **Step 2：编译前端确认无 TypeScript 错误**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm build 2>&1 | grep -E "error|Error" | head -20
```

Expected：无 TypeScript error

- [ ] **Step 3：Commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
git add src/lib/tauri.ts
git commit -m "feat(memory): add saveProjectMemory / distillProjectMemory TypeScript wrappers"
```

---

## Self-Review

**1. Spec 覆盖检查**

| Spec 要求（T1-T4） | 对应任务 |
|-------------------|---------|
| T1：WriteMemoryTool 作为 RuntimeTool | Task 2 + Task 4 |
| T1：注册到 ToolRegistry + DAILY_ALLOWED_TOOLS | Task 1 + Task 4 |
| T1：memory-mechanics system prompt section | Task 5 |
| T2：SearchMemoryTool 作为 RuntimeTool | Task 2 + Task 4 |
| T3：legacy memory_*.rs 删除 | Task 6 |
| T3：tool_executor/memory.rs re-export 移除 | Task 6 |
| T3：ensure_legacy_migrated() 保留 | ✅ 未修改 project_memory.rs 中的迁移逻辑 |
| T4：save_project_memory TS 封装 | Task 7 |
| T4：distill_project_memory TS 封装 | Task 7 |
| Feedback 类型（spec 也要求） | Task 3 |

**2. Placeholder 扫描**

- ✅ 所有步骤均含完整代码
- ✅ 无 TBD / TODO / 待实现
- `plugin/registry.rs` Task 4 Step 2 需按现有 `web_search` / `load_file` / `execute_python` 的 request-scoped factory 写法接入，避免回退到全局注册路径

**3. 类型一致性**

- `ProjectMemoryType::Feedback` — Task 3 定义，Task 2 `parse_memory_type()` 使用 ✅
- `MemoryDeps` — Task 2 定义，Task 4 通过 request-scoped runtime factory 注入 ✅
- `WriteMemoryRuntimeTool::new(deps)` / `SearchMemoryRuntimeTool::new(deps)` — 两处签名一致 ✅
- `ProjectMemoryEntryDraft`（Rust）↔ `ProjectMemoryEntryDraft`（TypeScript）字段对齐：`memoryType`（TS camelCase）↔ `memory_type`（Rust snake_case，serde rename） ✅

---

## 验收标准

1. `cargo test runtime::tools::builtin::memory` — 4 个测试全部通过
2. `cargo test project_memory` — feedback roundtrip 测试通过
3. `cargo test --test review_memory_legacy_retired_test` — 2 个 review 测试通过
4. `cargo test review_ --tests --no-fail-fast` — 所有已有 review_ 测试仍通过
5. `pnpm build` — 无 TypeScript 编译错误
6. legacy 文件 `memory_save.rs` / `memory_search.rs` / `memory_core.rs` / `memory_distill.rs` 不再存在于仓库中
