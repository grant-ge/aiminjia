# 后端事件扩展 — 工具调用与任务 Payload 增强

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 扩展三个 RuntimeEvent 的前端 payload：`tool:executing` 加入参 JSON、`tool:completed` 改为推完整 tool Message、`task:status-changed` 补全 subject/activeForm 等字段，使前端无需额外请求即可渲染工具步骤详情和任务列表。

**Architecture:** 改动集中在两层：`runtime/events.rs` 扩展 RuntimeEventKind 字段携带更多数据；`transport/tauri_event_adapter.rs` 把新字段映射到前端 JSON payload。`task:status-changed` 还需要 `task_runtime.rs` 在 emit 前读取完整 TaskRecord 以获得 subject 等字段。

**Tech Stack:** Rust, serde_json, Tauri runtime events

---

## 文件变更索引

| 文件 | 操作 | 变更内容 |
|------|------|---------|
| `src-tauri/src/runtime/events.rs` | Modify | `ToolCallExecuting` 加 `input: serde_json::Value`；`ToolCallCompleted` 加 `content: String`、`is_error` 已有；`TaskStatusChanged` 加 `subject`、`active_form`、`owner_agent_id` |
| `src-tauri/src/runtime/query_engine.rs` | Modify | 发射 `ToolCallExecuting` 时传 `call.args.clone()`；发射 `ToolCallCompleted` 时传工具输出 content |
| `src-tauri/src/runtime/task/task_models.rs` | Modify | `TaskRecord` 加 `active_form: Option<String>` |
| `src-tauri/src/runtime/task/task_runtime.rs` | Modify | `set_status` 发射 `TaskStatusChanged` 时从 task_record 取 subject/active_form/owner_agent_id |
| `src-tauri/src/transport/tauri_event_adapter.rs` | Modify | 三个事件的 JSON payload 加入新字段 |
| `src-tauri/tests/review_backend_event_payload_test.rs` | Create | 架构约束测试：验证三个事件 payload 包含必要字段 |

---

## Task 1：ToolCallExecuting 加入参字段

**Files:**
- Modify: `src-tauri/src/runtime/events.rs`
- Modify: `src-tauri/src/runtime/query_engine.rs`
- Modify: `src-tauri/src/transport/tauri_event_adapter.rs`

- [ ] **Step 1.1：在 events.rs 的 ToolCallExecuting 变体加 input 字段**

找到 `ToolCallExecuting`（约第 25 行），改为：

```rust
ToolCallExecuting {
    tool_call_id: ToolCallId,
    tool_name: String,
    input: serde_json::Value,
},
```

- [ ] **Step 1.2：确认编译报错位置**

```bash
cd /path/to/lotus-app && cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | grep "error\[" | head -20
```

预期：`query_engine.rs` 报 ToolCallExecuting 缺少 `input` 字段，`tauri_event_adapter.rs` 报模式匹配缺 `input`。

- [ ] **Step 1.3：query_engine.rs — 发射时传 call.args**

找到（约第 411 行）：
```rust
RuntimeEventKind::ToolCallExecuting {
    tool_call_id: crate::runtime::ids::ToolCallId::new(call.tool_call_id.clone()),
    tool_name: call.tool_name.clone(),
},
```

改为：
```rust
RuntimeEventKind::ToolCallExecuting {
    tool_call_id: crate::runtime::ids::ToolCallId::new(call.tool_call_id.clone()),
    tool_name: call.tool_name.clone(),
    input: call.args.clone(),
},
```

同时找到约第 589 行 permission replay 路径的第二处 `ToolCallExecuting`，也加上：
```rust
RuntimeEventKind::ToolCallExecuting {
    tool_call_id: crate::runtime::ids::ToolCallId::new(format!(
        "tool-call-{tool_name}"
    )),
    tool_name: tool_name.clone(),
    input: serde_json::Value::Null,  // permission replay 路径无原始 args
},
```

- [ ] **Step 1.4：tauri_event_adapter.rs — payload 加 input 字段**

找到：
```rust
RuntimeEventKind::ToolCallExecuting {
    tool_call_id,
    tool_name,
} => Some(LegacyEvent {
    name: "tool:executing".to_string(),
    payload: json!({
        "conversationId": conversation_id,
        "toolId": tool_call_id.as_str(),
        "toolName": tool_name,
        "runId": event.run_id.as_str(),
    }),
}),
```

改为：
```rust
RuntimeEventKind::ToolCallExecuting {
    tool_call_id,
    tool_name,
    input,
} => Some(LegacyEvent {
    name: "tool:executing".to_string(),
    payload: json!({
        "conversationId": conversation_id,
        "toolId": tool_call_id.as_str(),
        "toolName": tool_name,
        "runId": event.run_id.as_str(),
        "input": input,
    }),
}),
```

- [ ] **Step 1.5：修复所有测试文件中的 ToolCallExecuting 模式匹配**

```bash
grep -rn "ToolCallExecuting" src-tauri/tests/ src-tauri/src/ | grep -v "events.rs\|query_engine.rs\|tauri_event_adapter.rs" | grep -v "//\|test result"
```

对每个匹配处，在模式里加 `input: _`（测试 mock 中不需要 input 值时忽略）。

- [ ] **Step 1.6：编译通过**

```bash
cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | grep "error\[" | head -10
```

预期：0 error。

---

## Task 2：ToolCallCompleted 改为携带完整 tool Message

**Files:**
- Modify: `src-tauri/src/runtime/events.rs`
- Modify: `src-tauri/src/runtime/query_engine.rs`
- Modify: `src-tauri/src/transport/tauri_event_adapter.rs`

### 背景

前端将 `tool:completed` payload 直接当 `Message`（role: tool）upsert 进消息列表。payload 结构：
```json
{
  "id": "tool-<uuid>",
  "conversationId": "...",
  "role": "tool",
  "createdAt": "...",
  "content": {},
  "toolResult": {
    "toolCallId": "tc-001",
    "name": "browse_navigate",
    "content": "Page ready: ...",
    "isError": false,
    "durationMs": 1200
  }
}
```

注：`id` 需要和 `persist_tool_messages` 写磁盘时生成的 msg_id 一致，否则 upsert 后刷新历史会出现重复。解决方案：在 `run_tool_call_with_bus_internal` 里提前生成 msg_id，写磁盘和发事件用同一个 id。

- [ ] **Step 2.1：events.rs — ToolCallCompleted 加 content 和 msg_id 字段**

找到 `ToolCallCompleted`，改为：

```rust
ToolCallCompleted {
    tool_call_id: ToolCallId,
    tool_name: String,
    is_error: bool,
    content: String,         // 工具输出文本
    msg_id: String,          // 和磁盘写入一致的消息 id
    duration_ms: Option<u64>,
},
```

- [ ] **Step 2.2：query_engine.rs — 发射 ToolCallCompleted 时传 content**

找到成功路径的 `ToolCallCompleted` 发射（约第 433 行）：
```rust
RuntimeEventKind::ToolCallCompleted {
    tool_call_id: ...,
    tool_name: call.tool_name.clone(),
    is_error: false,
},
```

改为（在 dispatch 成功后，tool_result 中拿 content）：
```rust
RuntimeEventKind::ToolCallCompleted {
    tool_call_id: crate::runtime::ids::ToolCallId::new(call.tool_call_id.clone()),
    tool_name: call.tool_name.clone(),
    is_error: false,
    content: tool_result.content.clone(),
    msg_id: format!("tool-{}", uuid::Uuid::new_v4()),
    duration_ms: None,
},
```

错误路径（约第 483 行）改为：
```rust
RuntimeEventKind::ToolCallCompleted {
    tool_call_id: crate::runtime::ids::ToolCallId::new(call.tool_call_id.clone()),
    tool_name: call.tool_name.clone(),
    is_error: true,
    content: content.clone(),
    msg_id: format!("tool-{}", uuid::Uuid::new_v4()),
    duration_ms: None,
},
```

- [ ] **Step 2.3：tauri_event_adapter.rs — 构建完整 Message payload**

找到 `ToolCallCompleted` 映射，改为：

```rust
RuntimeEventKind::ToolCallCompleted {
    tool_call_id,
    tool_name,
    is_error,
    content,
    msg_id,
    duration_ms,
} => Some(LegacyEvent {
    name: "tool:completed".to_string(),
    payload: json!({
        "id": msg_id,
        "conversationId": conversation_id,
        "role": "tool",
        "createdAt": chrono::Utc::now().to_rfc3339(),
        "content": {},
        "toolResult": {
            "toolCallId": tool_call_id.as_str(),
            "name": tool_name,
            "content": content,
            "isError": is_error,
            "durationMs": duration_ms,
        },
        "runId": event.run_id.as_str(),
    }),
}),
```

- [ ] **Step 2.4：修复测试文件中的 ToolCallCompleted 模式匹配**

```bash
grep -rn "ToolCallCompleted" src-tauri/tests/ src-tauri/src/ | grep -v "events.rs\|query_engine.rs\|tauri_event_adapter.rs" | grep -v "//"
```

对每处加 `content: _, msg_id: _, duration_ms: _`。

- [ ] **Step 2.5：编译通过**

```bash
cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | grep "error\[" | head -10
```

预期：0 error。

- [ ] **Step 2.6：Commit Task 1 + 2**

```bash
git add src-tauri/src/runtime/events.rs \
        src-tauri/src/runtime/query_engine.rs \
        src-tauri/src/transport/tauri_event_adapter.rs \
        src-tauri/tests/
git commit -m "feat(events): add input to tool:executing and full Message payload to tool:completed"
```

---

## Task 3：TaskStatusChanged 补全 subject/activeForm 字段

**Files:**
- Modify: `src-tauri/src/runtime/task/task_models.rs`
- Modify: `src-tauri/src/runtime/events.rs`
- Modify: `src-tauri/src/runtime/task/task_runtime.rs`
- Modify: `src-tauri/src/transport/tauri_event_adapter.rs`

- [ ] **Step 3.1：task_models.rs — TaskRecord 加 active_form 字段**

```rust
#[derive(Clone, Debug)]
pub struct TaskRecord {
    pub task_id: TaskId,
    pub parent_run_id: RunId,
    pub owner_agent_id: Option<AgentId>,
    pub subject: String,
    pub status: TaskStatus,
    pub active_form: Option<String>,  // ← 新增：spinner 显示文字
}
```

- [ ] **Step 3.2：修复 TaskRecord 构造**

```bash
grep -rn "TaskRecord {" src-tauri/ | grep -v test | grep -v "task_models.rs"
```

对每处加 `active_form: None`。

- [ ] **Step 3.3：events.rs — TaskStatusChanged 加 subject/active_form/owner_agent_id**

找到 `TaskStatusChanged`，改为：

```rust
TaskStatusChanged {
    task_id: TaskId,
    status: String,
    subject: String,
    active_form: Option<String>,
    owner_agent_id: Option<AgentId>,
},
```

- [ ] **Step 3.4：task_runtime.rs — set_status 时从 task_record 取字段填入事件**

找到 `RuntimeEventKind::TaskStatusChanged` 构建处（约第 64 行），改为：

```rust
let event = RuntimeEvent::new(
    session_id,
    run_id,
    RuntimeEventKind::TaskStatusChanged {
        task_id: task_id.clone(),
        status: status_str.to_string(),
        subject: task_record
            .as_ref()
            .map(|r| r.subject.clone())
            .unwrap_or_default(),
        active_form: task_record
            .as_ref()
            .and_then(|r| r.active_form.clone()),
        owner_agent_id: task_record
            .as_ref()
            .and_then(|r| r.owner_agent_id.clone()),
    },
);
```

- [ ] **Step 3.5：tauri_event_adapter.rs — payload 加新字段**

找到 `TaskStatusChanged` 映射，改为：

```rust
RuntimeEventKind::TaskStatusChanged {
    task_id,
    status,
    subject,
    active_form,
    owner_agent_id,
} => Some(LegacyEvent {
    name: "task:status-changed".to_string(),
    payload: json!({
        "conversationId": conversation_id,
        "taskId": task_id.as_str(),
        "status": status,
        "runId": event.run_id.as_str(),
        "subject": subject,
        "activeForm": active_form,
        "owner": owner_agent_id.as_ref().map(|id| id.as_str()),
    }),
}),
```

- [ ] **Step 3.6：修复测试文件中的 TaskStatusChanged 模式匹配**

```bash
grep -rn "TaskStatusChanged" src-tauri/tests/ src-tauri/src/ | grep -v "events.rs\|task_runtime.rs\|tauri_event_adapter.rs" | grep -v "//"
```

对每处加 `subject: _, active_form: _, owner_agent_id: _`。

- [ ] **Step 3.7：编译通过**

```bash
cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | grep "error\[" | head -10
```

预期：0 error。

---

## Task 4：架构约束回归测试

**Files:**
- Create: `src-tauri/tests/review_backend_event_payload_test.rs`

- [ ] **Step 4.1：写测试——tool:executing payload 包含 input 字段**

```rust
// review_backend_event_payload_test.rs

use std::sync::Arc;
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::events::{RuntimeEvent, RuntimeEventKind};
use app_lib::runtime::ids::{RunId, SessionId, ToolCallId};
use app_lib::transport::tauri_event_adapter::TauriEventAdapter;
use app_lib::transport::testing::RecordingRuntimeHost;

fn make_bus_with_host() -> (RuntimeEventBus, RecordingRuntimeHost) {
    let host = RecordingRuntimeHost::new();
    let bus = RuntimeEventBus::new();
    bus.subscribe(Arc::new(TauriEventAdapter::new(host.clone())));
    (bus, host)
}

#[tokio::test]
async fn review_tool_executing_payload_includes_input() {
    let (bus, host) = make_bus_with_host();
    let session_id = SessionId::new("s1");
    let run_id = RunId::new("r1");

    bus.emit(RuntimeEvent::new(
        session_id.clone(),
        run_id.clone(),
        RuntimeEventKind::ToolCallExecuting {
            tool_call_id: ToolCallId::new("tc-1"),
            tool_name: "browse_navigate".to_string(),
            input: serde_json::json!({"url": "https://example.com"}),
        },
    ))
    .await
    .unwrap();

    let trace = host.trace();
    let event = trace
        .events
        .iter()
        .find(|e| e.name == "tool:executing")
        .expect("tool:executing must be emitted");

    assert_eq!(
        event.payload["toolName"].as_str(),
        Some("browse_navigate"),
    );
    assert_eq!(
        event.payload["input"]["url"].as_str(),
        Some("https://example.com"),
        "tool:executing payload must include input field"
    );
}
```

- [ ] **Step 4.2：写测试——tool:completed payload 是完整 Message 结构**

```rust
#[tokio::test]
async fn review_tool_completed_payload_is_full_message() {
    let (bus, host) = make_bus_with_host();
    let session_id = SessionId::new("s2");
    let run_id = RunId::new("r2");

    bus.emit(RuntimeEvent::new(
        session_id.clone(),
        run_id.clone(),
        RuntimeEventKind::ToolCallCompleted {
            tool_call_id: ToolCallId::new("tc-2"),
            tool_name: "browse_navigate".to_string(),
            is_error: false,
            content: "Page ready: https://example.com".to_string(),
            msg_id: "tool-abc-123".to_string(),
            duration_ms: Some(1200),
        },
    ))
    .await
    .unwrap();

    let trace = host.trace();
    let event = trace
        .events
        .iter()
        .find(|e| e.name == "tool:completed")
        .expect("tool:completed must be emitted");

    assert_eq!(event.payload["role"].as_str(), Some("tool"), "payload.role must be 'tool'");
    assert_eq!(event.payload["id"].as_str(), Some("tool-abc-123"), "payload.id must match msg_id");
    assert_eq!(
        event.payload["toolResult"]["toolCallId"].as_str(),
        Some("tc-2"),
        "payload.toolResult.toolCallId must be present"
    );
    assert_eq!(
        event.payload["toolResult"]["content"].as_str(),
        Some("Page ready: https://example.com"),
    );
    assert_eq!(
        event.payload["toolResult"]["isError"].as_bool(),
        Some(false),
    );
    assert_eq!(
        event.payload["toolResult"]["durationMs"].as_u64(),
        Some(1200),
    );
}
```

- [ ] **Step 4.3：写测试——task:status-changed payload 包含 subject 和 activeForm**

```rust
#[test]
fn review_task_status_changed_payload_includes_subject_and_active_form() {
    use app_lib::runtime::ids::TaskId;
    use app_lib::runtime::store::InMemoryTaskStore;
    use app_lib::runtime::task::{TaskRecord, TaskRuntime, TaskStatus};

    let host = RecordingRuntimeHost::new();
    let bus = RuntimeEventBus::new();
    bus.subscribe(Arc::new(TauriEventAdapter::new(host.clone())));

    let store = Arc::new(InMemoryTaskStore::new());
    let runtime = TaskRuntime::with_event_bus(store.clone(), bus);
    let task_id = TaskId::new("task-payload-1");

    runtime
        .create_task(TaskRecord {
            task_id: task_id.clone(),
            parent_run_id: RunId::new("run-payload-1"),
            owner_agent_id: None,
            subject: "探索项目上下文".to_string(),
            status: TaskStatus::Pending,
            active_form: Some("探索中…".to_string()),
        })
        .unwrap();

    runtime.set_status(&task_id, TaskStatus::Running).unwrap();

    let trace = host.trace();
    let event = trace
        .events
        .iter()
        .find(|e| e.name == "task:status-changed")
        .expect("task:status-changed must be emitted");

    assert_eq!(
        event.payload["subject"].as_str(),
        Some("探索项目上下文"),
        "payload must include subject"
    );
    assert_eq!(
        event.payload["activeForm"].as_str(),
        Some("探索中…"),
        "payload must include activeForm"
    );
    assert_eq!(event.payload["status"].as_str(), Some("running"));
}
```

- [ ] **Step 4.4：运行新测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml \
  review_tool_executing_payload_includes_input \
  review_tool_completed_payload_is_full_message \
  review_task_status_changed_payload_includes_subject_and_active_form \
  -- --nocapture 2>&1 | tail -15
```

预期：3 tests passed。

- [ ] **Step 4.5：运行 review_ 全量测试确认无退化**

```bash
cargo test --manifest-path src-tauri/Cargo.toml review_ --tests --no-fail-fast 2>&1 | tail -10
```

预期：全部通过。

- [ ] **Step 4.6：Commit**

```bash
git add src-tauri/src/runtime/task/task_models.rs \
        src-tauri/src/runtime/events.rs \
        src-tauri/src/runtime/task/task_runtime.rs \
        src-tauri/src/transport/tauri_event_adapter.rs \
        src-tauri/tests/review_backend_event_payload_test.rs \
        src-tauri/tests/
git commit -m "feat(events): enrich task:status-changed payload with subject and activeForm"
```

---

## 自检

- [x] Task 1 覆盖 `tool:executing` 加 `input`
- [x] Task 2 覆盖 `tool:completed` 改为完整 Message payload
- [x] Task 3 覆盖 `task:status-changed` 加 `subject`/`activeForm`/`owner`
- [x] Task 4 有完整测试验证三个事件的 payload 结构
- [x] TaskRecord 加 `active_form` 字段，构造处需全部补 `active_form: None`
- [x] ToolCallCompleted 的 `msg_id` 与磁盘写入的 `persist_tool_messages` 中 `msg_id` 目前各自独立生成——这是一个已知的 id 不一致问题，前端 upsert 时可能出现重复条目。后续需要在 `run_tool_call_with_bus_internal` 提前生成统一 msg_id 再分别传给持久化和事件发射，这属于后续优化，不阻塞本计划。
