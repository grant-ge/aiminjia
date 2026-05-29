# 后端接口扩展 — get_tasks 命令 + TaskStore list 能力

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 暴露 `get_tasks(conversationId)` Tauri 命令，使前端在切换对话时能从后端恢复该对话的完整 task 列表，实现辅助栏 task section 的历史回显。

**Architecture:** `TaskStore` trait 新增 `list_tasks_for_session` 方法；`InMemoryTaskStore` 实现按 session_id 过滤；`TaskRuntime` 提供 `list_for_session` 方法；通过 `SessionRuntime` 持有 `TaskRuntime` 来让 `TauriChatCommandAdapter` 访问 task 数据；注册新 Tauri 命令 `get_tasks`。

**Tech Stack:** Rust, Tauri, `src-tauri/src/runtime/store/task_store.rs`, `src-tauri/src/runtime/task/task_runtime.rs`, `src-tauri/src/commands/chat.rs`

---

## 背景说明

当前 task 架构：
- `InMemoryTaskStore`：按 `task_id` 存储，无 session/conversation 关联
- `TaskRecord`：有 `parent_run_id`，没有直接的 `conversation_id`
- `TaskRuntime`：只有 `create_task` 和 `set_status`，没有查询接口
- `TauriChatCommandAdapter`：持有 `SessionRuntime`，没有直接 task 访问路径

`conversation_id` 等价于 `session_id`（后端用 `SessionId` 标识对话）。task 的 `parent_run_id` 是每次 turn 的 run_id，不直接等于 session_id。

**解决方案**：`TaskRecord` 加 `session_id: SessionId` 字段，`TaskStore` 加按 session_id 过滤的查询。

---

## 文件变更索引

| 文件 | 操作 | 变更内容 |
|------|------|---------|
| `src-tauri/src/runtime/task/task_models.rs` | Modify | `TaskRecord` 加 `session_id: SessionId` |
| `src-tauri/src/runtime/store/task_store.rs` | Modify | `TaskStore` trait 加 `list_for_session`；`InMemoryTaskStore` 实现 |
| `src-tauri/src/runtime/task/task_runtime.rs` | Modify | 加 `list_for_session` 方法；`create_task` 接受 session_id |
| `src-tauri/src/models/message.rs` | Modify | 新增 `TaskRecordFrontend` 序列化结构 |
| `src-tauri/src/commands/chat.rs` | Modify | 新增 `get_tasks` 命令 |
| `src-tauri/src/transport/tauri_commands/chat.rs` | Modify | `TauriChatCommandAdapter` 加 `get_tasks` 方法，持有 `Arc<InMemoryTaskStore>` |
| `src-tauri/src/lib.rs` | Modify | 注册 `get_tasks` 命令；`InMemoryTaskStore` 注册为 Tauri State |
| `src-tauri/tests/review_get_tasks_command_test.rs` | Create | 架构约束测试 |

---

## Task 1：TaskRecord 加 session_id，TaskStore 加查询接口

**Files:**
- Modify: `src-tauri/src/runtime/task/task_models.rs`
- Modify: `src-tauri/src/runtime/store/task_store.rs`

- [ ] **Step 1.1：task_models.rs — TaskRecord 加 session_id**

```rust
use crate::runtime::ids::{AgentId, RunId, SessionId, TaskId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct TaskRecord {
    pub task_id: TaskId,
    pub session_id: SessionId,      // ← 新增：所属对话 id
    pub parent_run_id: RunId,
    pub owner_agent_id: Option<AgentId>,
    pub subject: String,
    pub status: TaskStatus,
    pub active_form: Option<String>,
}
```

（注：若 `active_form` 已在计划 A 中加入则保留，若计划 B 先执行则一并加入。）

- [ ] **Step 1.2：task_store.rs — TaskStore trait 加 list_for_session**

```rust
pub trait TaskStore: Send + Sync {
    fn create_task(&self, record: TaskRecord) -> Result<()>;
    fn get_task(&self, task_id: &TaskId) -> Result<Option<TaskRecord>>;
    fn update_task_status(&self, task_id: &TaskId, status: TaskStatus) -> Result<()>;
    // 新增
    fn list_for_session(&self, session_id: &SessionId) -> Result<Vec<TaskRecord>>;
}
```

- [ ] **Step 1.3：task_store.rs — InMemoryTaskStore 实现 list_for_session**

```rust
impl TaskStore for InMemoryTaskStore {
    // ... 现有方法不变 ...

    fn list_for_session(&self, session_id: &SessionId) -> Result<Vec<TaskRecord>> {
        let tasks = self.tasks.lock().unwrap();
        let result = tasks
            .values()
            .filter(|r| r.session_id.as_str() == session_id.as_str())
            .cloned()
            .collect();
        Ok(result)
    }
}
```

- [ ] **Step 1.4：修复所有 TaskRecord 构造——加 session_id**

```bash
grep -rn "TaskRecord {" src-tauri/ | grep -v "task_models.rs"
```

对每处补 `session_id: SessionId::new("unknown")` 或从上下文取真实 session_id。

测试文件中的 TaskRecord 构造加 `session_id: SessionId::new("test-session")`。

- [ ] **Step 1.5：编译通过**

```bash
cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | grep "error\[" | head -10
```

预期：0 error。

---

## Task 2：TaskRuntime 加 list_for_session，models 加前端序列化结构

**Files:**
- Modify: `src-tauri/src/runtime/task/task_runtime.rs`
- Modify: `src-tauri/src/models/message.rs`

- [ ] **Step 2.1：task_runtime.rs — 加 list_for_session 方法**

```rust
pub fn list_for_session(&self, session_id: &SessionId) -> Result<Vec<TaskRecord>> {
    self.store.list_for_session(session_id)
}
```

- [ ] **Step 2.2：models/message.rs — 新增 TaskRecordFrontend**

在文件末尾加：

```rust
use crate::runtime::task::task_models::{TaskRecord, TaskStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecordFrontend {
    pub task_id: String,
    pub session_id: String,
    pub subject: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_form: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

impl From<TaskRecord> for TaskRecordFrontend {
    fn from(r: TaskRecord) -> Self {
        let status_str = match r.status {
            TaskStatus::Pending => "pending",
            TaskStatus::Running => "running",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
        };
        Self {
            task_id: r.task_id.as_str().to_string(),
            session_id: r.session_id.as_str().to_string(),
            subject: r.subject,
            status: status_str.to_string(),
            active_form: r.active_form,
            owner: r.owner_agent_id.map(|id| id.as_str().to_string()),
        }
    }
}
```

- [ ] **Step 2.3：编译通过**

```bash
cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | grep "error\[" | head -10
```

预期：0 error。

- [ ] **Step 2.4：Commit Task 1 + 2**

```bash
git add src-tauri/src/runtime/task/task_models.rs \
        src-tauri/src/runtime/store/task_store.rs \
        src-tauri/src/runtime/task/task_runtime.rs \
        src-tauri/src/models/message.rs \
        src-tauri/tests/
git commit -m "feat(task): add session_id to TaskRecord and list_for_session query"
```

---

## Task 3：get_tasks Tauri 命令注册

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: `src-tauri/src/commands/chat.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 3.1：lib.rs — 把 InMemoryTaskStore 注册为 Tauri State**

在 `app.manage(db)` 等行附近，加：

```rust
let task_store = Arc::new(app_lib::runtime::store::InMemoryTaskStore::new());
app.manage(task_store);
```

- [ ] **Step 3.2：transport/tauri_commands/chat.rs — TauriChatCommandAdapter 加 get_tasks**

在 `TauriChatCommandAdapter` 的 impl 块末尾加：

```rust
pub async fn get_tasks(
    &self,
    conversation_id: String,
) -> Result<Vec<crate::models::message::TaskRecordFrontend>, String> {
    use crate::runtime::ids::SessionId;
    use crate::runtime::store::TaskStore;

    let task_store = self
        .services
        .app
        .try_state::<Arc<crate::runtime::store::InMemoryTaskStore>>()
        .ok_or_else(|| "task_store not registered".to_string())?;

    let session_id = SessionId::new(conversation_id.clone());
    task_store
        .inner()
        .list_for_session(&session_id)
        .map(|records| records.into_iter().map(Into::into).collect())
        .map_err(|e| e.to_string())
}
```

- [ ] **Step 3.3：commands/chat.rs — 新增 get_tasks 命令函数**

在文件末尾加：

```rust
#[tauri::command]
pub async fn get_tasks(
    adapter: State<'_, Arc<crate::transport::tauri_commands::chat::TauriChatCommandAdapter>>,
    conversation_id: String,
) -> Result<Vec<crate::models::message::TaskRecordFrontend>, String> {
    adapter.get_tasks(conversation_id).await
}
```

- [ ] **Step 3.4：lib.rs — 注册 get_tasks 命令**

在 `generate_handler!` 列表的 chat 命令区加：

```rust
chat::get_tasks,
```

- [ ] **Step 3.5：编译通过**

```bash
cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | grep "error\[" | head -10
```

预期：0 error。

---

## Task 4：架构约束回归测试

**Files:**
- Create: `src-tauri/tests/review_get_tasks_command_test.rs`

- [ ] **Step 4.1：写测试——list_for_session 按 session_id 过滤**

```rust
// review_get_tasks_command_test.rs

use std::sync::Arc;
use app_lib::runtime::ids::{RunId, SessionId, TaskId};
use app_lib::runtime::store::{InMemoryTaskStore, TaskStore};
use app_lib::runtime::task::task_models::{TaskRecord, TaskStatus};

/// list_for_session 只返回指定 session 的 tasks，不泄漏其他 session 数据。
#[test]
fn review_list_for_session_filters_by_session_id() {
    let store = InMemoryTaskStore::new();

    store.create_task(TaskRecord {
        task_id: TaskId::new("t1"),
        session_id: SessionId::new("conv-abc"),
        parent_run_id: RunId::new("run-1"),
        owner_agent_id: None,
        subject: "Task in conv-abc".to_string(),
        status: TaskStatus::Pending,
        active_form: None,
    }).unwrap();

    store.create_task(TaskRecord {
        task_id: TaskId::new("t2"),
        session_id: SessionId::new("conv-xyz"),
        parent_run_id: RunId::new("run-2"),
        owner_agent_id: None,
        subject: "Task in conv-xyz".to_string(),
        status: TaskStatus::Running,
        active_form: Some("探索中…".to_string()),
    }).unwrap();

    let result = store.list_for_session(&SessionId::new("conv-abc")).unwrap();

    assert_eq!(result.len(), 1, "must only return tasks for conv-abc");
    assert_eq!(result[0].task_id.as_str(), "t1");
    assert_eq!(result[0].subject, "Task in conv-abc");
}

/// 空 session 返回空列表，不 panic。
#[test]
fn review_list_for_session_returns_empty_for_unknown_session() {
    let store = InMemoryTaskStore::new();
    let result = store.list_for_session(&SessionId::new("no-such-session")).unwrap();
    assert!(result.is_empty());
}

/// TaskRecordFrontend::from 正确序列化所有字段。
#[test]
fn review_task_record_frontend_serialization() {
    use app_lib::models::message::TaskRecordFrontend;

    let record = TaskRecord {
        task_id: TaskId::new("t3"),
        session_id: SessionId::new("conv-s"),
        parent_run_id: RunId::new("run-3"),
        owner_agent_id: None,
        subject: "导出数据".to_string(),
        status: TaskStatus::Completed,
        active_form: Some("导出中…".to_string()),
    };

    let frontend: TaskRecordFrontend = record.into();
    let json = serde_json::to_value(&frontend).unwrap();

    assert_eq!(json["taskId"], "t3");
    assert_eq!(json["sessionId"], "conv-s");
    assert_eq!(json["subject"], "导出数据");
    assert_eq!(json["status"], "completed");
    assert_eq!(json["activeForm"], "导出中…");
}
```

- [ ] **Step 4.2：运行新测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml \
  review_list_for_session_filters_by_session_id \
  review_list_for_session_returns_empty_for_unknown_session \
  review_task_record_frontend_serialization \
  -- --nocapture 2>&1 | tail -10
```

预期：3 tests passed。

- [ ] **Step 4.3：运行 review_ 全量**

```bash
cargo test --manifest-path src-tauri/Cargo.toml review_ --tests --no-fail-fast 2>&1 | tail -10
```

预期：全部通过。

- [ ] **Step 4.4：Commit**

```bash
git add src-tauri/src/transport/tauri_commands/chat.rs \
        src-tauri/src/commands/chat.rs \
        src-tauri/src/lib.rs \
        src-tauri/tests/review_get_tasks_command_test.rs
git commit -m "feat(task): expose get_tasks Tauri command for conversation task list"
```

---

## 自检

- [x] TaskRecord 加 session_id，所有构造处补齐
- [x] TaskStore trait 加 list_for_session，InMemoryTaskStore 实现
- [x] TaskRecordFrontend 序列化对齐前端 ConversationTaskState 字段名（camelCase）
- [x] get_tasks 命令注册到 lib.rs generate_handler!
- [x] 测试覆盖：跨 session 隔离、空 session、字段序列化
- [x] 已知限制：InMemoryTaskStore 重启后丢失。前端 get_tasks 在重启后返回空列表，辅助栏 task section 显示"暂无待办"——这是预期行为，持久化 task store 属于后续专项。
