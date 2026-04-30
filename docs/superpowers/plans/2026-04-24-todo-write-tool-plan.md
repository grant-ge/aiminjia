# TaskCreate TaskUpdate TaskList Tool Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 对标 claude-code-best 的新版 Task 工具体系，实现持久化的 `TaskCreate` / `TaskUpdate` / `TaskList` 三个后端 agent 工具，并在前端实时展示任务状态。

**Architecture:** 不实现旧 `TodoWrite`。新增 file-backed task store，把任务持久化到 `~/.renlijia/tasks/<taskListId>/<taskId>.json`；三个工具全部实现 `RuntimeTool` 并注册到 `ToolCatalog` / `DAILY_ALLOWED_TOOLS` / `ToolDispatcher`；任务变更通过已有 `RuntimeEventKind::TaskStatusChanged` 和新增 `task:list-updated` 事件通知前端。

**Tech Stack:** Rust / serde_json / std::fs + atomic write / RuntimeTool (后端); React / Zustand / TypeScript (前端)

---

## 背景与对标结论

### 关键修正

不要实现 `TodoWrite`。

排查 claude-code-best 后确认：

- `TodoWriteTool` 是旧版 V1 工具，状态写在进程内 `appState.todos`。
- 新版已切到 Task V2：`TaskCreateTool` / `TaskUpdateTool` / `TaskListTool` / `TaskGetTool`。
- Task V2 持久化到文件系统，不依赖进程内 UI state。

本计划只实现用户要求的三件：

1. `TaskCreate`
2. `TaskUpdate`
3. `TaskList`

`TaskGet` 暂不实现，避免超出范围。

### claude-code-best 参考文件

- `/Users/a20250311/github/claude-code-best/src/tasks.ts`
- `/Users/a20250311/github/claude-code-best/src/tools/TaskCreateTool/TaskCreateTool.ts`
- `/Users/a20250311/github/claude-code-best/src/tools/TaskUpdateTool/TaskUpdateTool.ts`
- `/Users/a20250311/github/claude-code-best/src/tools/TaskListTool/TaskListTool.ts`

### claude-code-best Task V2 核心机制

| 维度 | claude-code-best | lotus-app 本计划 |
|---|---|---|
| 存储位置 | `~/.claude/tasks/<taskListId>/<taskId>.json` | `~/.renlijia/tasks/<taskListId>/<taskId>.json` |
| taskListId | env/team/session fallback | sessionId；后续可扩展 teamId |
| ID 分配 | `.highwatermark` + lockfile | `.highwatermark` + `.lock`（同目录） |
| 并发安全 | proper-lockfile | 文件级/目录级 lock；第一版用 atomic temp+rename + mutex，避免半写 |
| Task schema | id/subject/description/activeForm/owner/status/blocks/blockedBy/metadata | 同字段，status 用 `pending/in_progress/completed` |
| 创建 | TaskCreate | TaskCreate |
| 更新 | TaskUpdate，支持 delete/owner/deps/metadata | TaskUpdate，支持 status/owner/deps/metadata/delete |
| 列表 | TaskList，过滤内部 task，展示 blockedBy | TaskList，同语义 |
| UI 刷新 | in-process signal | Tauri event `task:list-updated` + 既有 `task:status-changed` |

### 当前 lotus-app 状态

已有但不够：

- `src-tauri/src/runtime/task/task_models.rs`
  - 只有 `TaskRecord { task_id, session_id, parent_run_id, owner_agent_id, subject, status, active_form }`
  - 缺少 `description/owner/blocks/blockedBy/metadata`
  - status 不是 claude-code-best 的 `pending/in_progress/completed`
- `src-tauri/src/runtime/store/task_store.rs`
  - 只有 `InMemoryTaskStore`
  - 无持久化
- `src-tauri/src/runtime/task/task_runtime.rs`
  - 目前偏向 agent 子任务生命周期事件，不是 Task V2 工具体系
- `src-tauri/src/runtime/tools/catalog.rs`
  - 无 `TaskCreate` / `TaskUpdate` / `TaskList`
- `src-tauri/src/runtime/tools/builtin/mod.rs`
  - 无 task 工具模块
- 前端已有 `TASK_STATUS_CHANGED` 常量，但没有 Task V2 列表 store/UI

### 架构约束

1. 新工具必须走 `RuntimeTool`，不得新增 `ToolPlugin`。
2. 工具 schema 必须注册到 `ToolCatalog`。
3. runtime 层不得 `use tauri::*`。
4. Task 数据必须持久化到 `~/.renlijia/tasks/`，不能只在内存里。
5. 不要把 Task V2 与 agent 子任务生命周期混成一个概念：可以复用 `TaskStatusChanged` 事件，但存储模型要补齐 Task V2 字段。

---

## File Map

**新建：**
- `src-tauri/src/runtime/task/task_v2_store.rs` — 文件持久化 task store（`~/.renlijia/tasks/<taskListId>/`）
- `src-tauri/src/runtime/tools/builtin/task_tools.rs` — `TaskCreateRuntimeTool` / `TaskUpdateRuntimeTool` / `TaskListRuntimeTool`
- `src-tauri/tests/task_tools_test.rs` — 后端集成测试
- `src/stores/taskListStore.ts` — 前端 Task V2 store
- `src/components/chat-scene/TaskListPanel.tsx` — 前端任务列表展示

**修改：**
- `src-tauri/src/runtime/task/task_models.rs` — 扩展 TaskRecord / TaskStatus
- `src-tauri/src/runtime/task/mod.rs` — 导出 task_v2_store
- `src-tauri/src/runtime/tools/builtin/mod.rs` — 添加 `pub mod task_tools;`
- `src-tauri/src/runtime/tools/catalog.rs` — 注册三个工具 schema，加入 `DAILY_ALLOWED_TOOLS`
- `src-tauri/src/plugin/builtin/tools/mod.rs` — 注册三个 RuntimeTool
- `src-tauri/src/runtime/events.rs` — 新增 `TaskListUpdated`
- `src-tauri/src/transport/tauri_event_adapter.rs` — 映射为 `task:list-updated`
- `src/lib/tauri.ts` — 新增事件常量、payload 类型和 listener
- `src/App.tsx` — 订阅 task:list-updated

---

## Task 1: 扩展 Task V2 数据模型

**Files:**
- Modify: `src-tauri/src/runtime/task/task_models.rs`

- [ ] **Step 1: 替换 TaskStatus 为 claude-code-best 兼容状态**

打开 `src-tauri/src/runtime/task/task_models.rs`，把内容替换为：

```rust
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::runtime::ids::{AgentId, RunId, SessionId, TaskId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Completed => "completed",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub id: String,
    pub subject: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_form: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub status: TaskStatus,
    pub blocks: Vec<String>,
    pub blocked_by: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,

    // lotus runtime context fields; persisted for recovery and event correlation.
    pub session_id: SessionId,
    pub parent_run_id: RunId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_agent_id: Option<AgentId>,
}

impl TaskRecord {
    pub fn task_id(&self) -> TaskId {
        TaskId::new(self.id.clone())
    }
}
```

- [ ] **Step 2: 修复编译错误**

现有代码可能还访问 `record.task_id` 或 `TaskStatus::Running/Failed/Cancelled`。

把：

```rust
record.task_id
```

改为：

```rust
record.task_id()
```

把旧 status 映射改为：

```rust
TaskStatus::Pending => "pending",
TaskStatus::InProgress => "in_progress",
TaskStatus::Completed => "completed",
```

`Failed/Cancelled` 不再属于 Task V2；如果 agent 子任务仍需要这些状态，不要复用 `TaskStatus`，改为在原调用处转成字符串事件，不进入 Task V2 store。

- [ ] **Step 3: 编译验证**

```bash
cd /Users/a20250311/.codex/worktrees/7526/lotus-app/src-tauri
cargo check 2>&1 | head -80
```

预期：修完后无编译错误。

---

## Task 2: 文件持久化 TaskV2Store

**Files:**
- Create: `src-tauri/src/runtime/task/task_v2_store.rs`
- Modify: `src-tauri/src/runtime/task/mod.rs`

- [ ] **Step 1: 创建 task_v2_store.rs**

新建 `src-tauri/src/runtime/task/task_v2_store.rs`：

```rust
//! File-backed Task V2 store.
//!
//! Mirrors claude-code-best src/tasks.ts:
//! - task files live under ~/.renlijia/tasks/<taskListId>/<id>.json
//! - .highwatermark tracks max assigned id
//! - writes use temp file + rename to avoid partial writes

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};

use crate::runtime::task::task_models::TaskRecord;

const HIGH_WATER_MARK_FILE: &str = ".highwatermark";

#[derive(Debug)]
pub struct FileTaskV2Store {
    root: PathBuf,
    lock: Mutex<()>,
}

impl FileTaskV2Store {
    pub fn new(aijia_home: PathBuf) -> Self {
        Self {
            root: aijia_home.join("tasks"),
            lock: Mutex::new(()),
        }
    }

    fn sanitize(input: &str) -> String {
        input
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
            .collect()
    }

    fn list_dir(&self, task_list_id: &str) -> PathBuf {
        self.root.join(Self::sanitize(task_list_id))
    }

    fn task_path(&self, task_list_id: &str, task_id: &str) -> PathBuf {
        self.list_dir(task_list_id)
            .join(format!("{}.json", Self::sanitize(task_id)))
    }

    fn highwatermark_path(&self, task_list_id: &str) -> PathBuf {
        self.list_dir(task_list_id).join(HIGH_WATER_MARK_FILE)
    }

    fn ensure_dir(&self, task_list_id: &str) -> Result<()> {
        fs::create_dir_all(self.list_dir(task_list_id))?;
        Ok(())
    }

    fn read_highwatermark(&self, task_list_id: &str) -> Result<u64> {
        let path = self.highwatermark_path(task_list_id);
        if !path.exists() {
            return Ok(0);
        }
        let s = fs::read_to_string(path)?;
        Ok(s.trim().parse::<u64>().unwrap_or(0))
    }

    fn write_highwatermark(&self, task_list_id: &str, value: u64) -> Result<()> {
        self.atomic_write(&self.highwatermark_path(task_list_id), value.to_string().as_bytes())
    }

    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, bytes)?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn next_id(&self, task_list_id: &str) -> Result<String> {
        let _guard = self.lock.lock().unwrap();
        self.ensure_dir(task_list_id)?;
        let next = self.read_highwatermark(task_list_id)? + 1;
        self.write_highwatermark(task_list_id, next)?;
        Ok(next.to_string())
    }

    pub fn create(&self, task_list_id: &str, task: &TaskRecord) -> Result<()> {
        let _guard = self.lock.lock().unwrap();
        self.ensure_dir(task_list_id)?;
        let path = self.task_path(task_list_id, &task.id);
        if path.exists() {
            return Err(anyhow!("task already exists: {}", task.id));
        }
        let bytes = serde_json::to_vec_pretty(task)?;
        self.atomic_write(&path, &bytes)
    }

    pub fn get(&self, task_list_id: &str, task_id: &str) -> Result<Option<TaskRecord>> {
        let path = self.task_path(task_list_id, task_id);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read task file {}", path.display()))?;
        let task: TaskRecord = serde_json::from_str(&content)?;
        Ok(Some(task))
    }

    pub fn list(&self, task_list_id: &str) -> Result<Vec<TaskRecord>> {
        let dir = self.list_dir(task_list_id);
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut tasks = vec![];
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let content = fs::read_to_string(&path)?;
            let task: TaskRecord = serde_json::from_str(&content)?;
            tasks.push(task);
        }
        tasks.sort_by_key(|t| t.id.parse::<u64>().unwrap_or(u64::MAX));
        Ok(tasks)
    }

    pub fn update(&self, task_list_id: &str, task: &TaskRecord) -> Result<()> {
        let _guard = self.lock.lock().unwrap();
        self.ensure_dir(task_list_id)?;
        let path = self.task_path(task_list_id, &task.id);
        if !path.exists() {
            return Err(anyhow!("task not found: {}", task.id));
        }
        let bytes = serde_json::to_vec_pretty(task)?;
        self.atomic_write(&path, &bytes)
    }

    pub fn delete(&self, task_list_id: &str, task_id: &str) -> Result<bool> {
        let _guard = self.lock.lock().unwrap();
        let path = self.task_path(task_list_id, task_id);
        if !path.exists() {
            return Ok(false);
        }
        fs::remove_file(path)?;
        Ok(true)
    }
}
```

- [ ] **Step 2: 导出模块**

打开 `src-tauri/src/runtime/task/mod.rs`，添加：

```rust
pub mod task_v2_store;
```

并导出：

```rust
pub use task_v2_store::FileTaskV2Store;
```

- [ ] **Step 3: 编译验证**

```bash
cd /Users/a20250311/.codex/worktrees/7526/lotus-app/src-tauri
cargo check 2>&1 | head -60
```

预期：无编译错误。

---

## Task 3: ToolExecutionContext 注入 TaskV2 store 根路径

**Files:**
- Modify: `src-tauri/src/runtime/tools/context.rs`

- [ ] **Step 1: 增加 task_store_root 字段**

打开 `src-tauri/src/runtime/tools/context.rs`，在 `ToolExecutionContext` 结构体末尾添加：

```rust
    /// Task V2 持久化根目录（AiJiaHome），工具用它构造 FileTaskV2Store。
    pub task_store_root: Option<std::path::PathBuf>,
```

在 `ToolExecutionContext::new()` 初始化中添加：

```rust
task_store_root: None,
```

- [ ] **Step 2: 在构造 ToolExecutionContext 的地方填充 root**

找到 `src-tauri/src/runtime/state.rs` 中 `ToolExecutionContext::new(...)` 的调用。构造后，如果 `ctx.capability.storage.workspace_path` 可用，则把 `~/.renlijia` 根目录填入。

如果当前已有 `AiJiaHome` 或 app_data_dir 传入 session runtime，请优先用该权威路径。不要用 workspace 子目录代替 `~/.renlijia`。

代码形态：

```rust
ctx.task_store_root = Some(aijia_home_path.clone());
```

如果当前 `state.rs` 拿不到 `aijia_home_path`，则先在 `ToolExecutionContext` 中不填 root，让 Task 工具从 `CapabilityContext.storage.workspace_path` 回退到 workspace root，并在后续 review 中把 root 接到 `AiJiaHome::from_home()`。

- [ ] **Step 3: 编译验证**

```bash
cd /Users/a20250311/.codex/worktrees/7526/lotus-app/src-tauri
cargo check 2>&1 | head -80
```

---

## Task 4: 注册 Task 工具 schema

**Files:**
- Modify: `src-tauri/src/runtime/tools/catalog.rs`

- [ ] **Step 1: 新增 TaskCreate catalog entry**

在 `build_default_catalog()` 的 Support tools 区块添加：

```rust
c.insert(CatalogEntry::new(
    ToolDefinition::new(
        "TaskCreate",
        "创建一条持久化任务，用于当前 session/agent 工作清单。",
    )
    .with_kind(ToolKind::Support),
    json!({
        "type": "object",
        "required": ["subject", "description"],
        "properties": {
            "subject": { "type": "string", "description": "任务短标题" },
            "description": { "type": "string", "description": "任务详细说明" },
            "activeForm": { "type": "string", "description": "进行中展示文案，如 Running tests" },
            "metadata": { "type": "object", "description": "可选元数据" }
        }
    }),
));
```

- [ ] **Step 2: 新增 TaskUpdate catalog entry**

```rust
c.insert(CatalogEntry::new(
    ToolDefinition::new(
        "TaskUpdate",
        "更新、删除或设置任务依赖、owner、status、metadata。",
    )
    .with_kind(ToolKind::Support),
    json!({
        "type": "object",
        "required": ["taskId"],
        "properties": {
            "taskId": { "type": "string", "description": "任务 ID" },
            "subject": { "type": "string", "description": "新的任务标题" },
            "description": { "type": "string", "description": "新的任务描述" },
            "activeForm": { "type": "string", "description": "进行中展示文案" },
            "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "deleted"] },
            "owner": { "type": "string", "description": "任务 owner agent/name" },
            "addBlocks": { "type": "array", "items": { "type": "string" } },
            "addBlockedBy": { "type": "array", "items": { "type": "string" } },
            "metadata": { "type": "object", "description": "metadata merge；value=null 表示删除 key" }
        }
    }),
));
```

- [ ] **Step 3: 新增 TaskList catalog entry**

```rust
c.insert(CatalogEntry::new(
    ToolDefinition::new(
        "TaskList",
        "列出当前 task list 的所有任务及阻塞状态。",
    )
    .with_kind(ToolKind::Support)
    .with_read_only(true),
    json!({
        "type": "object",
        "properties": {},
        "required": []
    }),
));
```

- [ ] **Step 4: 加入 DAILY_ALLOWED_TOOLS**

把以下三项加入 `DAILY_ALLOWED_TOOLS`：

```rust
"TaskCreate",
"TaskUpdate",
"TaskList",
```

- [ ] **Step 5: 编译验证**

```bash
cd /Users/a20250311/.codex/worktrees/7526/lotus-app/src-tauri
cargo check 2>&1 | head -60
```

---

## Task 5: 实现 TaskCreate / TaskList 工具

**Files:**
- Create: `src-tauri/src/runtime/tools/builtin/task_tools.rs`
- Modify: `src-tauri/src/runtime/tools/builtin/mod.rs`

- [ ] **Step 1: 创建 task_tools.rs 基础代码**

新建 `src-tauri/src/runtime/tools/builtin/task_tools.rs`：

```rust
//! Task V2 RuntimeTools — TaskCreate, TaskUpdate, TaskList.
//!
//! Mirrors claude-code-best Task V2 tools, backed by ~/.renlijia/tasks/<taskListId>/.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::runtime::task::task_models::{TaskRecord, TaskStatus};
use crate::runtime::task::FileTaskV2Store;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct TaskCreateRuntimeTool;
pub struct TaskUpdateRuntimeTool;
pub struct TaskListRuntimeTool;

fn task_list_id(ctx: &ToolExecutionContext) -> String {
    ctx.session_id.as_str().to_string()
}

fn store_for(ctx: &ToolExecutionContext) -> Result<FileTaskV2Store, ToolError> {
    let root = ctx
        .task_store_root
        .clone()
        .or_else(|| {
            ctx.capability
                .as_ref()
                .and_then(|c| c.storage.as_ref())
                .map(|s| s.workspace_path.clone())
        })
        .ok_or_else(|| ToolError::ExecutionFailed("Task tools require a storage root".into()))?;
    Ok(FileTaskV2Store::new(root))
}

fn required_str<'a>(input: &'a Value, key: &str, tool: &str) -> Result<&'a str, ToolError> {
    input.get(key).and_then(|v| v.as_str()).ok_or_else(|| {
        ToolError::InputValidationError {
            tool_name: tool.into(),
            message: format!("missing or invalid '{}'", key),
        }
    })
}

fn parse_status(value: &str) -> Result<TaskStatus, ToolError> {
    match value {
        "pending" => Ok(TaskStatus::Pending),
        "in_progress" => Ok(TaskStatus::InProgress),
        "completed" => Ok(TaskStatus::Completed),
        other => Err(ToolError::InputValidationError {
            tool_name: "TaskUpdate".into(),
            message: format!("invalid status: {}", other),
        }),
    }
}

fn task_to_json(task: &TaskRecord) -> Value {
    json!({
        "id": task.id,
        "subject": task.subject,
        "description": task.description,
        "activeForm": task.active_form,
        "owner": task.owner,
        "status": task.status.as_str(),
        "blocks": task.blocks,
        "blockedBy": task.blocked_by,
        "metadata": task.metadata,
    })
}
```

- [ ] **Step 2: 实现 TaskCreateRuntimeTool**

在同文件追加：

```rust
#[async_trait]
impl RuntimeTool for TaskCreateRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("TaskCreate")
            .unwrap_or_else(|| ToolDefinition::new("TaskCreate", "创建任务"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool { true }

    async fn execute(&self, input: Value, ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        let store = store_for(&ctx)?;
        let list_id = task_list_id(&ctx);
        let id = store.next_id(&list_id).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let subject = required_str(&input, "subject", "TaskCreate")?.to_string();
        let description = required_str(&input, "description", "TaskCreate")?.to_string();
        let active_form = input.get("activeForm").and_then(|v| v.as_str()).map(str::to_string);
        let metadata = input
            .get("metadata")
            .and_then(|v| serde_json::from_value::<HashMap<String, Value>>(v.clone()).ok());

        let task = TaskRecord {
            id: id.clone(),
            subject: subject.clone(),
            description,
            active_form,
            owner: None,
            status: TaskStatus::Pending,
            blocks: vec![],
            blocked_by: vec![],
            metadata,
            session_id: ctx.session_id.clone(),
            parent_run_id: ctx.run_id.clone(),
            owner_agent_id: ctx.agent_id.clone(),
        };

        store.create(&list_id, &task).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let data = json!({
            "task": {
                "id": id,
                "subject": subject,
            }
        });
        Ok(ToolResult::new(
            "TaskCreate",
            format!("Task #{} created successfully: {}", data["task"]["id"].as_str().unwrap_or(""), data["task"]["subject"].as_str().unwrap_or("")),
            Some(data),
        ))
    }
}
```

- [ ] **Step 3: 实现 TaskListRuntimeTool**

```rust
#[async_trait]
impl RuntimeTool for TaskListRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("TaskList")
            .unwrap_or_else(|| ToolDefinition::new("TaskList", "列出任务"))
    }

    fn is_read_only(&self, _input: &Value) -> bool { true }
    fn is_concurrency_safe(&self, _input: &Value) -> bool { true }

    async fn execute(&self, _input: Value, ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        let store = store_for(&ctx)?;
        let list_id = task_list_id(&ctx);
        let tasks = store.list(&list_id).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let completed: std::collections::HashSet<String> = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Completed)
            .map(|t| t.id.clone())
            .collect();

        let payload: Vec<Value> = tasks
            .iter()
            .filter(|t| !t.metadata.as_ref().and_then(|m| m.get("_internal")).and_then(|v| v.as_bool()).unwrap_or(false))
            .map(|t| json!({
                "id": t.id,
                "subject": t.subject,
                "status": t.status.as_str(),
                "owner": t.owner,
                "blockedBy": t.blocked_by.iter().filter(|id| !completed.contains(*id)).cloned().collect::<Vec<_>>(),
            }))
            .collect();

        let content = if payload.is_empty() {
            "No tasks found".to_string()
        } else {
            payload.iter().map(|t| {
                let owner = t.get("owner").and_then(|v| v.as_str()).map(|o| format!(" ({})", o)).unwrap_or_default();
                let blocked_by = t.get("blockedBy").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                let blocked = if blocked_by.is_empty() {
                    String::new()
                } else {
                    let ids = blocked_by.iter().filter_map(|v| v.as_str()).map(|id| format!("#{}", id)).collect::<Vec<_>>().join(", ");
                    format!(" [blocked by {}]", ids)
                };
                format!(
                    "#{} [{}] {}{}{}",
                    t["id"].as_str().unwrap_or(""),
                    t["status"].as_str().unwrap_or(""),
                    t["subject"].as_str().unwrap_or(""),
                    owner,
                    blocked,
                )
            }).collect::<Vec<_>>().join("\n")
        };

        Ok(ToolResult::new("TaskList", content, Some(json!({ "tasks": payload }))))
    }
}
```

- [ ] **Step 4: 导出模块**

打开 `src-tauri/src/runtime/tools/builtin/mod.rs`，添加：

```rust
pub mod task_tools;
```

- [ ] **Step 5: 编译验证**

```bash
cd /Users/a20250311/.codex/worktrees/7526/lotus-app/src-tauri
cargo check 2>&1 | head -80
```

---

## Task 6: 实现 TaskUpdate 工具

**Files:**
- Modify: `src-tauri/src/runtime/tools/builtin/task_tools.rs`

- [ ] **Step 1: 追加 TaskUpdateRuntimeTool 实现**

在 `task_tools.rs` 末尾添加：

```rust
#[async_trait]
impl RuntimeTool for TaskUpdateRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("TaskUpdate")
            .unwrap_or_else(|| ToolDefinition::new("TaskUpdate", "更新任务"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool { true }

    async fn execute(&self, input: Value, ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        let store = store_for(&ctx)?;
        let list_id = task_list_id(&ctx);
        let task_id = required_str(&input, "taskId", "TaskUpdate")?;

        let existing = store
            .get(&list_id, task_id)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let Some(mut task) = existing else {
            return Ok(ToolResult::new(
                "TaskUpdate",
                format!("Task #{} not found", task_id),
                Some(json!({
                    "success": false,
                    "taskId": task_id,
                    "updatedFields": [],
                    "error": "Task not found"
                })),
            ));
        };

        let mut updated_fields = Vec::<String>::new();
        let old_status = task.status.clone();

        if let Some(status) = input.get("status").and_then(|v| v.as_str()) {
            if status == "deleted" {
                let deleted = store.delete(&list_id, task_id).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                return Ok(ToolResult::new(
                    "TaskUpdate",
                    if deleted { format!("Deleted task #{}", task_id) } else { format!("Task #{} not found", task_id) },
                    Some(json!({
                        "success": deleted,
                        "taskId": task_id,
                        "updatedFields": if deleted { vec!["deleted"] } else { vec![] },
                        "statusChange": if deleted { json!({"from": old_status.as_str(), "to": "deleted"}) } else { Value::Null }
                    })),
                ));
            }
            let new_status = parse_status(status)?;
            if task.status != new_status {
                task.status = new_status;
                updated_fields.push("status".into());
            }
        }

        if let Some(subject) = input.get("subject").and_then(|v| v.as_str()) {
            if task.subject != subject {
                task.subject = subject.to_string();
                updated_fields.push("subject".into());
            }
        }
        if let Some(description) = input.get("description").and_then(|v| v.as_str()) {
            if task.description != description {
                task.description = description.to_string();
                updated_fields.push("description".into());
            }
        }
        if let Some(active_form) = input.get("activeForm").and_then(|v| v.as_str()) {
            if task.active_form.as_deref() != Some(active_form) {
                task.active_form = Some(active_form.to_string());
                updated_fields.push("activeForm".into());
            }
        }
        if let Some(owner) = input.get("owner").and_then(|v| v.as_str()) {
            if task.owner.as_deref() != Some(owner) {
                task.owner = Some(owner.to_string());
                updated_fields.push("owner".into());
            }
        }
        if let Some(add_blocks) = input.get("addBlocks").and_then(|v| v.as_array()) {
            for block_id in add_blocks.iter().filter_map(|v| v.as_str()) {
                if !task.blocks.iter().any(|id| id == block_id) {
                    task.blocks.push(block_id.to_string());
                    updated_fields.push("blocks".into());
                }
            }
        }
        if let Some(add_blocked_by) = input.get("addBlockedBy").and_then(|v| v.as_array()) {
            for blocker_id in add_blocked_by.iter().filter_map(|v| v.as_str()) {
                if !task.blocked_by.iter().any(|id| id == blocker_id) {
                    task.blocked_by.push(blocker_id.to_string());
                    updated_fields.push("blockedBy".into());
                }
            }
        }
        if let Some(metadata) = input.get("metadata").and_then(|v| v.as_object()) {
            let mut merged = task.metadata.take().unwrap_or_default();
            for (key, value) in metadata {
                if value.is_null() {
                    merged.remove(key);
                } else {
                    merged.insert(key.clone(), value.clone());
                }
            }
            task.metadata = Some(merged);
            updated_fields.push("metadata".into());
        }

        store.update(&list_id, &task).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let status_change = if old_status != task.status {
            Some(json!({ "from": old_status.as_str(), "to": task.status.as_str() }))
        } else {
            None
        };

        Ok(ToolResult::new(
            "TaskUpdate",
            format!("Updated task #{} {}", task_id, updated_fields.join(", ")),
            Some(json!({
                "success": true,
                "taskId": task_id,
                "updatedFields": updated_fields,
                "statusChange": status_change,
                "task": task_to_json(&task),
            })),
        ))
    }
}
```

- [ ] **Step 2: 编译验证**

```bash
cd /Users/a20250311/.codex/worktrees/7526/lotus-app/src-tauri
cargo check 2>&1 | head -80
```

---

## Task 7: 注册三个 RuntimeTool

**Files:**
- Modify: `src-tauri/src/plugin/builtin/tools/mod.rs`

- [ ] **Step 1: 注册 RuntimeTool**

在 `register_builtin_tools()` 的 runtime 注册块中添加：

```rust
use crate::runtime::tools::builtin::task_tools::{
    TaskCreateRuntimeTool, TaskListRuntimeTool, TaskUpdateRuntimeTool,
};
registry.register_runtime(Arc::new(TaskCreateRuntimeTool)).await;
registry.register_runtime(Arc::new(TaskUpdateRuntimeTool)).await;
registry.register_runtime(Arc::new(TaskListRuntimeTool)).await;
```

确保放在 `registry.validate_catalog_consistency().await;` 之前。

- [ ] **Step 2: 编译验证**

```bash
cd /Users/a20250311/.codex/worktrees/7526/lotus-app/src-tauri
cargo check 2>&1 | head -80
```

---

## Task 8: 后端集成测试

**Files:**
- Create: `src-tauri/tests/task_tools_test.rs`

- [ ] **Step 1: 编写测试**

新建 `src-tauri/tests/task_tools_test.rs`：

```rust
use tempfile::TempDir;
use serde_json::json;

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::tools::builtin::task_tools::{
    TaskCreateRuntimeTool, TaskListRuntimeTool, TaskUpdateRuntimeTool,
};
use app_lib::runtime::tools::catalog::{DAILY_ALLOWED_TOOLS, TOOL_CATALOG};
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::RuntimeTool;

fn ctx(root: &TempDir) -> ToolExecutionContext {
    let mut ctx = ToolExecutionContext::new(
        "sess-task-test".into(),
        "run-task-test".into(),
        None,
        "tc-task-test",
        CancellationToken::new(),
    );
    ctx.task_store_root = Some(root.path().to_path_buf());
    ctx
}

#[test]
fn task_tools_are_in_catalog_and_daily_allowed() {
    for name in ["TaskCreate", "TaskUpdate", "TaskList"] {
        assert!(TOOL_CATALOG.get_entry(name).is_some(), "{} catalog entry missing", name);
        assert!(DAILY_ALLOWED_TOOLS.contains(&name), "{} missing from DAILY_ALLOWED_TOOLS", name);
    }
}

#[tokio::test]
async fn task_create_persists_and_task_list_reads() {
    let root = TempDir::new().unwrap();
    let create = TaskCreateRuntimeTool;
    let list = TaskListRuntimeTool;

    let create_result = create.execute(json!({
        "subject": "Write test",
        "description": "Write a regression test",
        "activeForm": "Writing test"
    }), ctx(&root)).await.unwrap();

    assert!(create_result.content.contains("Task #1 created"));

    let list_result = list.execute(json!({}), ctx(&root)).await.unwrap();
    assert!(list_result.content.contains("#1 [pending] Write test"));
}

#[tokio::test]
async fn task_update_changes_status_and_owner() {
    let root = TempDir::new().unwrap();
    let create = TaskCreateRuntimeTool;
    let update = TaskUpdateRuntimeTool;
    let list = TaskListRuntimeTool;

    create.execute(json!({
        "subject": "Implement feature",
        "description": "Implement feature details"
    }), ctx(&root)).await.unwrap();

    let update_result = update.execute(json!({
        "taskId": "1",
        "status": "in_progress",
        "owner": "agent-a"
    }), ctx(&root)).await.unwrap();

    assert!(update_result.content.contains("Updated task #1"));

    let list_result = list.execute(json!({}), ctx(&root)).await.unwrap();
    assert!(list_result.content.contains("#1 [in_progress] Implement feature (agent-a)"));
}

#[tokio::test]
async fn task_update_delete_removes_task() {
    let root = TempDir::new().unwrap();
    let create = TaskCreateRuntimeTool;
    let update = TaskUpdateRuntimeTool;
    let list = TaskListRuntimeTool;

    create.execute(json!({
        "subject": "Temporary task",
        "description": "Will be deleted"
    }), ctx(&root)).await.unwrap();

    update.execute(json!({
        "taskId": "1",
        "status": "deleted"
    }), ctx(&root)).await.unwrap();

    let list_result = list.execute(json!({}), ctx(&root)).await.unwrap();
    assert_eq!(list_result.content, "No tasks found");
}
```

- [ ] **Step 2: 运行测试**

```bash
cd /Users/a20250311/.codex/worktrees/7526/lotus-app/src-tauri
cargo test --test task_tools_test -- --nocapture 2>&1
```

预期：4 个测试通过。

---

## Task 9: 后端事件通知 task:list-updated

**Files:**
- Modify: `src-tauri/src/runtime/events.rs`
- Modify: `src-tauri/src/transport/tauri_event_adapter.rs`
- Modify: `src-tauri/src/runtime/tools/builtin/task_tools.rs`

- [ ] **Step 1: 新增 RuntimeEventKind::TaskListUpdated**

在 `src-tauri/src/runtime/events.rs` 的 `RuntimeEventKind` 增加：

```rust
TaskListUpdated {
    task_list_id: String,
    tasks: Vec<serde_json::Value>,
},
```

- [ ] **Step 2: 映射到 Tauri event**

在 `src-tauri/src/transport/tauri_event_adapter.rs` 添加 match arm：

```rust
RuntimeEventKind::TaskListUpdated { task_list_id, tasks } => Some(LegacyEvent {
    name: "task:list-updated".to_string(),
    payload: serde_json::json!({
        "conversationId": conversation_id,
        "runId": event.run_id.as_str(),
        "taskListId": task_list_id,
        "tasks": tasks,
    }),
}),
```

- [ ] **Step 3: 发事件策略**

`RuntimeTool` 当前只有 `event_sink`（无 payload），不能直接发 `RuntimeEvent`。第一版前端可以通过 `TaskList` 工具结果展示，不强制实时事件。若要实时事件，需要把 `RuntimeEventBus` 注入 `ToolExecutionContext`，这会扩大 context。

本计划选择不在第一版发 `task:list-updated`，只保留 `TaskStatusChanged` 现有事件和工具结果展示。删除 Step 1/2 如果实现时发现会造成 context 膨胀。

- [ ] **Step 4: 不实现事件时的验证**

确认前端不依赖 `task:list-updated`，而是在 tool result 中展示 `TaskList` 输出；后续若要右侧常驻 Task 面板，再单独实现事件广播。

---

## Task 10: 前端最小展示（复用工具结果）

**Files:**
- Modify: 无必须修改

Task V2 第一版可以不新增前端 UI：

- `TaskCreate` 返回 `Task #1 created successfully: ...`
- `TaskUpdate` 返回 `Updated task #1 status, owner`
- `TaskList` 返回：

```text
#1 [pending] Write test
#2 [in_progress] Implement feature (agent-a) [blocked by #1]
```

这些会通过现有 `ToolGroupCard` / `ToolGroupStepRow` 通用工具结果展示。

- [ ] **Step 1: 验证前端无类型修改**

```bash
cd /Users/a20250311/.codex/worktrees/7526/lotus-app
pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts 2>&1 | tail -30
```

预期：现有前端事件测试通过。

---

## Task 11: 端到端验证

- [ ] **Step 1: Rust 新增测试**

```bash
cd /Users/a20250311/.codex/worktrees/7526/lotus-app/src-tauri
cargo test --test task_tools_test -- --nocapture 2>&1
```

- [ ] **Step 2: Rust review 测试**

```bash
cd /Users/a20250311/.codex/worktrees/7526/lotus-app/src-tauri
cargo test review_ --tests --no-fail-fast 2>&1 | tail -30
```

- [ ] **Step 3: 前端关键测试**

```bash
cd /Users/a20250311/.codex/worktrees/7526/lotus-app
pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts 2>&1 | tail -30
```

---

## 自查结论

- 已改为对标 claude-code-best Task V2，而非旧 TodoWrite。
- Task 内容持久化到文件系统：`~/.renlijia/tasks/<sessionId>/<taskId>.json`。
- 三个工具均走 `RuntimeTool`：`TaskCreate` / `TaskUpdate` / `TaskList`。
- 不新增 ToolPlugin，不让 runtime 依赖 Tauri。
- 第一版前端不新增常驻任务面板，直接复用通用 tool result 展示，避免过度实现。
