# Runtime Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 对齐 `claude-code-best` 的下一批 runtime 核心语义：收敛 session cancel tree 单真源，并把 subagent transcript/result 从“最小 envelope”推进到“可恢复、可审计、可按 ref 取回”的持久化边界。

**Architecture:** 本计划分成两条连续主线。I1-I3 先收口 cancel control plane：`SessionRuntime` 成为 active session cancel root owner，`stop_streaming` 先打到 runtime cancel root，再桥接 gateway/python interrupt，`RuntimeRunRegistry` 明确只保留 provider stream cancel bridge 角色；I4-I6 再补 `subagent transcript parity`：让 `AgentRuntime` 同时持有 invocation store 与 transcript store，child run completion 写入真实 transcript payload，parent/background 路径可以通过 `transcript_ref` 反查 transcript，而不是只剩一个 summary 字符串。

**Tech Stack:** Rust, tokio, serde/serde_json, file-backed runtime stores, existing `CancellationToken`, existing `AgentRuntime` / `SubAgentResultEnvelope`

---

## 为什么是这批计划

### 对标来源
- cancel / abort tree：`/Users/a20250311/github/claude-code-best/src/QueryEngine.ts`
- parent/child abort 传播：`/Users/a20250311/github/claude-code-best/src/utils/abortController.ts`
- sidechain transcript 持久化：`/Users/a20250311/github/claude-code-best/src/tasks/LocalMainSessionTask.ts`
- transcript path / storage：`/Users/a20250311/github/claude-code-best/src/utils/sessionStorage.ts`
- tool result / transcript 保留语义：`/Users/a20250311/github/claude-code-best/src/Tool.ts`（`preserveToolUseResults?`）

### 当前 lotus-app 已完成但仍未 parity 的点
- `SessionRuntime` 现在仍保留 `cancel_token: Option<CancellationToken>` 这种“外部注入模板 token”形态，还没有成为真正的 session cancel owner。
- `stop_streaming` 目前只会走 `conversation_service::stop_streaming()` 的 gateway/python bridge，再单独清 pending permission；runtime turn/tool cancel 并没有一条统一入口。
- `RuntimeRunRegistry` 现在实际只提供 provider stream 的 `watch::Receiver<bool>`；这没问题，但需要文档与 review test 明确它不是第二套 runtime cancel owner。
- `SubAgentResultEnvelope.transcript_ref` 现在仍等价于裸 `child_run_id`，没有真实 transcript store 落点。
- `AgentRuntime` 当前只持有 invocation store，且 record 只有 `summary_or_output_ref`；没有稳定的 transcript store，也没有 `child_run_id -> transcript_ref -> transcript` 的恢复链路。

### 本计划的关键校准
- `SessionRuntime` 持有的是“每个 session 当前 active run 的 cancel root”，语义对齐 `AbortController` 的单次使用：一旦 root 被 cancel，下一次同 session 新 turn 必须自动轮换到 fresh root，不能复用已取消 root。
- `RuntimeRunRegistry` 继续保留 provider stream cancel bridge，不迁移成 runtime owner；真正的 owner 是 `SessionRuntime`。
- `AgentRuntime` 应该同时拥有 invocation store 与 transcript store；不要把 transcript store 再散落到 transport 或 plugin context 成为第三套真源。
- `transcript_ref` 统一规范成 `subagent://<child_run_id>`；真正 payload 落在 file-backed transcript store。

### 本计划明确不做的事
- 不重开 Plan-G / MCP 主体改造。
- 不重做 F9/F10 permission control plane。
- 不引入新的前端 transcript viewer 或 Tauri UI 面板。
- 不把 `sub_agent.rs` 整体迁移到纯 runtime tool 链；只补足对标本批所需的 cancel / transcript 真源。

---

## 文件地图与职责边界

### Cancel tree / session control plane
- Modify: `src-tauri/src/runtime/session_runtime.rs`
  - 移除外部注入的 `cancel_token` 模板字段，改成内部维护 `session_cancel_roots`。
  - root 被 cancel 后，下一次同 session 新 turn 自动轮换到 fresh root。
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
  - `stop_streaming` 先走 `runtime.cancel_session(..., Interrupt)`，再走 `conversation_service::stop_streaming(...)`。
- Modify: `src-tauri/src/runtime/run_registry.rs`
  - 只保留 busy/run-id/stream cancel bridge 语义，并补注释说明不是 runtime cancel owner。
- Modify: `src-tauri/src/llm/gateway.rs`
  - 只补职责注释与 review 测试，不引入新的 runtime cancellation 语义。
- Test: `src-tauri/src/runtime/session_runtime.rs`（unit tests）
- Test: `src-tauri/tests/review_stop_streaming_runtime_cancel_wiring_test.rs`
- Test: `src-tauri/tests/review_runtime_cancel_owner_test.rs`

### Subagent transcript parity / result persistence
- Create: `src-tauri/src/runtime/agent/subagent_transcript_store.rs`
  - 定义 transcript record、trait、in-memory 实现、file-backed 实现。
- Modify: `src-tauri/src/runtime/agent/agent_runtime.rs`
  - 让 `AgentRuntime` 同时拥有 invocation store + transcript store。
  - 提供 `store_transcript`、`get_transcript_ref`、`load_transcript` 三个 runtime API。
- Modify: `src-tauri/src/runtime/agent/mod.rs`
  - export transcript store 模块。
- Modify: `src-tauri/src/lib.rs`
  - 构造 file-backed `AgentRuntime` 时同时传入 transcript store 目录。
- Modify: `src-tauri/src/runtime/agent/invocation.rs`
  - invocation 模型显式新增 `transcript_ref`。
- Modify: `src-tauri/src/runtime/store/agent_invocation_store.rs`
  - record 模型新增 `transcript_ref`，并提供更新 summary + transcript_ref 的接口。
- Modify: `src-tauri/src/runtime/agent/file_agent_invocation_store.rs`
  - 同步 file-backed store。
- Modify: `src-tauri/src/runtime/agent/subagent_result_envelope.rs`
  - 统一 `transcript_ref` 构造 helper，避免继续直接塞裸 `child_run_id`。
- Modify: `src-tauri/src/llm/sub_agent.rs`
  - child run 收敛结果前写 transcript store，再产出 envelope，并在 background completion 路径把 `transcript_ref` 写进 invocation store。
- Modify: `src-tauri/tests/background_run_message_bridge_test.rs`
  - 跟随 `complete_background_run(...)` 新签名更新现有测试。
- Test: `src-tauri/tests/subagent_transcript_store_test.rs`
- Test: `src-tauri/tests/subagent_transcript_persistence_test.rs`
- Test: `src-tauri/tests/subagent_parent_transcript_access_test.rs`
- Test: `src-tauri/tests/subagent_result_envelope_test.rs`

---

## Task I1：让 `SessionRuntime` 成为 active session cancel root owner，并在 cancel 后自动轮换 root

**Files:**
- Modify: `src-tauri/src/runtime/session_runtime.rs`
- Test: `src-tauri/src/runtime/session_runtime.rs`

- [ ] **Step I1-1: 写失败测试**

```rust
#[test]
fn session_runtime_reuses_cancel_root_until_it_is_cancelled() {
    let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new());
    let session = SessionId::new("sess-i1");

    let root_a = runtime.ensure_active_session_cancel_root(&session);
    let root_b = runtime.ensure_active_session_cancel_root(&session);

    runtime.cancel_session(&session, CancellationReason::Interrupt);

    assert!(root_a.is_cancelled());
    assert!(root_b.is_cancelled());
    assert_eq!(root_a.reason(), Some(CancellationReason::Interrupt));
    assert_eq!(root_b.reason(), Some(CancellationReason::Interrupt));
}

#[test]
fn session_runtime_rotates_cancel_root_after_interrupt() {
    let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new());
    let session = SessionId::new("sess-i1-rotate");

    let old_root = runtime.ensure_active_session_cancel_root(&session);
    runtime.cancel_session(&session, CancellationReason::Interrupt);
    let new_root = runtime.ensure_active_session_cancel_root(&session);

    assert!(old_root.is_cancelled());
    assert_eq!(old_root.reason(), Some(CancellationReason::Interrupt));
    assert!(!new_root.is_cancelled());
}

#[test]
fn clear_session_state_drops_cached_cancel_root_for_that_session() {
    let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new());
    let session = SessionId::new("sess-i1-clear");

    let root_before = runtime.ensure_active_session_cancel_root(&session);
    runtime.clear_session_state(&session);
    let root_after = runtime.ensure_active_session_cancel_root(&session);

    runtime.cancel_session(&session, CancellationReason::Interrupt);

    assert!(!root_before.is_cancelled());
    assert!(root_after.is_cancelled());
}
```

- [ ] **Step I1-2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test session_runtime_rotates_cancel_root_after_interrupt --lib -- --nocapture`
Expected: FAIL，报 `ensure_active_session_cancel_root` / `cancel_session` 不存在，或 cancel 后下一次取 root 仍然是已取消状态。

- [ ] **Step I1-3: 最小实现 `SessionRuntime` 内部持有 cancel root map，并移除旧模板 token 字段**

```rust
// src-tauri/src/runtime/session_runtime.rs
#[derive(Clone)]
pub struct SessionRuntime {
    query_engine: QueryEngine,
    session_query_engines: Arc<Mutex<HashMap<String, QueryEngine>>>,
    session_cancel_roots: Arc<Mutex<HashMap<String, CancellationToken>>>,
    event_bus: RuntimeEventBus,
    llm_executor: Option<Arc<dyn RuntimeLlmExecutor>>,
    authorized_workspace_store: Option<Arc<dyn AuthorizedWorkspaceStore>>,
    pending_permission_store: Arc<PendingPermissionRequestStore>,
}

impl SessionRuntime {
    fn ensure_active_session_cancel_root(&self, session_id: &SessionId) -> CancellationToken {
        let mut roots = self
            .session_cancel_roots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = roots
            .entry(session_id.as_str().to_string())
            .or_insert_with(CancellationToken::new);
        if entry.is_cancelled() {
            *entry = CancellationToken::new();
        }
        entry.clone()
    }

    fn current_session_cancel_root(&self, session_id: &SessionId) -> Option<CancellationToken> {
        self.session_cancel_roots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(session_id.as_str())
            .cloned()
    }
}
```

- [ ] **Step I1-4: `run_chat_request()` 始终从 active session root 派生 child token**

```rust
// src-tauri/src/runtime/session_runtime.rs
let session_root = self.ensure_active_session_cancel_root(turn.session_id());
turn = turn.with_cancellation(session_root.child_token());
```

- [ ] **Step I1-5: `clear_session_state()` 同时清理 cancel root**

```rust
// src-tauri/src/runtime/session_runtime.rs
self.session_cancel_roots
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner())
    .remove(session_id.as_str());
```

- [ ] **Step I1-6: 运行测试确认通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test session_runtime_ --lib -- --nocapture`
Expected: PASS。

- [ ] **Step I1-7: Commit**

```bash
git add src-tauri/src/runtime/session_runtime.rs
git commit -m "refactor(runtime): make SessionRuntime own active session cancel roots — I1"
```

---

## Task I2：把 `stop_streaming` 接到 runtime session cancel root，再桥接 gateway/python interrupt

**Files:**
- Modify: `src-tauri/src/runtime/session_runtime.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Create: `src-tauri/tests/review_stop_streaming_runtime_cancel_wiring_test.rs`

- [ ] **Step I2-1: 写失败测试**

```rust
#[test]
fn review_stop_streaming_routes_interrupt_through_session_runtime_before_gateway_bridge() {
    let source = include_str!("../src/transport/tauri_commands/chat.rs");
    let start = source.find("pub async fn stop_streaming").unwrap();
    let end = source[start..]
        .find("pub async fn approve_permission_request")
        .map(|offset| start + offset)
        .unwrap();
    let body = &source[start..end];

    let cancel_idx = body.find("cancel_session(").unwrap();
    let bridge_idx = body.find("conversation_service::stop_streaming(").unwrap();

    assert!(cancel_idx < bridge_idx);
    assert!(body.contains("CancellationReason::Interrupt"));
    assert!(!body.contains("cancel_pending_permission_requests_for_session("));
}
```

- [ ] **Step I2-2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_stop_streaming_runtime_cancel_wiring -- --nocapture`
Expected: FAIL，说明 `stop_streaming` 还没先调用 `cancel_session(...)`，或仍在方法体里直接清 pending permission。

- [ ] **Step I2-3: 在 `SessionRuntime` 增加统一取消入口**

```rust
// src-tauri/src/runtime/session_runtime.rs
pub fn cancel_session(&self, session_id: &SessionId, reason: CancellationReason) {
    if let Some(root) = self.current_session_cancel_root(session_id) {
        root.cancel_with_reason(reason);
    }
    self.cancel_pending_permission_requests_for_session(
        session_id,
        "Permission request cancelled because the session was stopped.",
    );
}
```

- [ ] **Step I2-4: transport 层先 cancel runtime root，再走 legacy bridge**

```rust
// src-tauri/src/transport/tauri_commands/chat.rs
pub async fn stop_streaming(&self, conversation_id: String) -> Result<(), String> {
    let session_id = SessionId::new(conversation_id.clone());
    self.runtime.cancel_session(
        &session_id,
        crate::runtime::cancellation::CancellationReason::Interrupt,
    );

    conversation_service::stop_streaming(
        self.services.gateway.clone(),
        self.services.session_mgr.clone(),
        conversation_id,
    )
    .await
}
```

- [ ] **Step I2-5: 运行测试确认通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_stop_streaming_runtime_cancel_wiring -- --nocapture`
Expected: PASS。

- [ ] **Step I2-6: 跑级联回归**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test cancel_cascade -- --nocapture`
Expected: PASS。

- [ ] **Step I2-7: Commit**

```bash
git add src-tauri/src/runtime/session_runtime.rs src-tauri/src/transport/tauri_commands/chat.rs src-tauri/tests/review_stop_streaming_runtime_cancel_wiring_test.rs
git commit -m "refactor(runtime): route stop_streaming through session cancel root — I2"
```

---

## Task I3：收紧 `RuntimeRunRegistry` 的职责，避免形成第二套 cancel owner

**Files:**
- Modify: `src-tauri/src/runtime/run_registry.rs`
- Modify: `src-tauri/src/llm/gateway.rs`
- Create: `src-tauri/tests/review_runtime_cancel_owner_test.rs`

- [ ] **Step I3-1: 写失败测试**

```rust
#[test]
fn review_run_registry_stays_stream_cancel_bridge_only() {
    let source = include_str!("../src/runtime/run_registry.rs");
    assert!(source.contains("watch::Sender<bool>"));
    assert!(source.contains("run_id"));
    assert!(!source.contains("CancellationToken"));
}

#[test]
fn review_session_runtime_no_longer_accepts_injected_cancel_template() {
    let source = include_str!("../src/runtime/session_runtime.rs");
    assert!(!source.contains("cancel_token: Option<CancellationToken>"));
    assert!(!source.contains("with_cancellation_token("));
    assert!(source.contains("session_cancel_roots"));
}
```

- [ ] **Step I3-2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_runtime_cancel_owner -- --nocapture`
Expected: FAIL，直到源码里不再保留旧 `cancel_token` 模板 owner，且 review guard 文件已落地。

- [ ] **Step I3-3: 补职责注释，不给 `run_registry` 添加新的 runtime cancel 语义**

```rust
// src-tauri/src/runtime/run_registry.rs
/// RuntimeRunRegistry 只负责：
/// 1. session -> active run_id 映射
/// 2. provider stream 级 cancel watch channel
/// 3. busy session 查询
///
/// Session / turn / tool 的 runtime cancellation owner 在 SessionRuntime。
```

```rust
// src-tauri/src/llm/gateway.rs
// 保留 attach_stream()/cancel_conversation() 现有桥接语义，
// 不在 gateway/run_registry 中重新引入 runtime-owned cancellation。
```

- [ ] **Step I3-4: 运行测试确认通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_runtime_cancel_owner -- --nocapture`
Expected: PASS。

- [ ] **Step I3-5: 回归验证**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_ --tests --no-fail-fast`
Expected: PASS。

- [ ] **Step I3-6: Commit**

```bash
git add src-tauri/src/runtime/run_registry.rs src-tauri/src/llm/gateway.rs src-tauri/tests/review_runtime_cancel_owner_test.rs
git commit -m "docs(runtime): narrow RuntimeRunRegistry to stream cancel bridge — I3"
```

---

## Task I4：新增 file-backed subagent transcript store，并让 `AgentRuntime` 持有它

**Files:**
- Create: `src-tauri/src/runtime/agent/subagent_transcript_store.rs`
- Modify: `src-tauri/src/runtime/agent/agent_runtime.rs`
- Modify: `src-tauri/src/runtime/agent/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Create: `src-tauri/tests/subagent_transcript_store_test.rs`

- [ ] **Step I4-1: 写失败测试**

```rust
use app_lib::runtime::agent::subagent_transcript_store::{
    FileSubagentTranscriptStore, InMemorySubagentTranscriptStore,
    SubagentTranscriptEntryRecord, SubagentTranscriptStore,
};
use tempfile::TempDir;

#[test]
fn in_memory_transcript_store_roundtrips_entries_by_ref() {
    let store = InMemorySubagentTranscriptStore::new();
    let transcript_ref = "subagent://run-child-1";
    let entries = vec![SubagentTranscriptEntryRecord {
        role: "assistant".to_string(),
        content: "done".to_string(),
        tool_call_id: None,
        tool_name: None,
    }];

    store.put(transcript_ref, &entries).unwrap();
    let loaded = store.get(transcript_ref).unwrap().unwrap();
    assert_eq!(loaded, entries);
}

#[test]
fn file_backed_transcript_store_roundtrips_entries_by_ref() {
    let temp = TempDir::new().unwrap();
    let store = FileSubagentTranscriptStore::new(temp.path().to_path_buf()).unwrap();
    let transcript_ref = "subagent://run-child-2";

    store.put(
        transcript_ref,
        &[SubagentTranscriptEntryRecord {
            role: "tool".to_string(),
            content: "saved /tmp/a.json".to_string(),
            tool_call_id: Some("call-1".to_string()),
            tool_name: Some("extract_table_data".to_string()),
        }],
    )
    .unwrap();

    let loaded = store.get(transcript_ref).unwrap().unwrap();
    assert_eq!(loaded[0].tool_name.as_deref(), Some("extract_table_data"));
}
```

- [ ] **Step I4-2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test subagent_transcript_store_test -- --nocapture`
Expected: FAIL，报模块 / 类型不存在。

- [ ] **Step I4-3: 建立 transcript store 抽象与 file-backed 实现**

```rust
// src-tauri/src/runtime/agent/subagent_transcript_store.rs
pub trait SubagentTranscriptStore: Send + Sync {
    fn put(&self, transcript_ref: &str, entries: &[SubagentTranscriptEntryRecord]) -> Result<()>;
    fn get(&self, transcript_ref: &str) -> Result<Option<Vec<SubagentTranscriptEntryRecord>>>;
}

pub struct FileSubagentTranscriptStore {
    root_dir: PathBuf,
}
```

- [ ] **Step I4-4: 让 `AgentRuntime` 同时拥有 invocation store + transcript store**

```rust
// src-tauri/src/runtime/agent/agent_runtime.rs
pub struct AgentRuntime {
    invocation_store: Arc<dyn AgentInvocationStore>,
    transcript_store: Arc<dyn SubagentTranscriptStore>,
}

pub fn new(
    invocation_store: Arc<dyn AgentInvocationStore>,
    transcript_store: Arc<dyn SubagentTranscriptStore>,
) -> Self {
    Self {
        invocation_store,
        transcript_store,
    }
}
```

- [ ] **Step I4-5: 生产构造接入 file-backed transcript store**

```rust
// src-tauri/src/lib.rs
let agent_invocation_store_path = app_data_dir.join("agent_invocations.json");
let subagent_transcript_store_dir = app_data_dir.join("subagent_transcripts");
let agent_runtime = Arc::new(
    runtime::agent::AgentRuntime::from_storage(
        agent_invocation_store_path,
        subagent_transcript_store_dir,
    )
    .unwrap_or_else(|e| {
        log::warn!("Failed to create file-backed AgentRuntime: {e}, falling back to in-memory");
        runtime::agent::AgentRuntime::for_test()
    }),
);
```

- [ ] **Step I4-6: 运行测试确认通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test subagent_transcript_store_test -- --nocapture`
Expected: PASS。

- [ ] **Step I4-7: Commit**

```bash
git add src-tauri/src/runtime/agent/subagent_transcript_store.rs src-tauri/src/runtime/agent/agent_runtime.rs src-tauri/src/runtime/agent/mod.rs src-tauri/src/lib.rs src-tauri/tests/subagent_transcript_store_test.rs
git commit -m "feat(subagent): add file-backed transcript store and runtime ownership — I4"
```

---

## Task I5：让 child run completion 写入 transcript store，并持久化真实 `transcript_ref`

**Files:**
- Modify: `src-tauri/src/llm/sub_agent.rs`
- Modify: `src-tauri/src/runtime/agent/subagent_result_envelope.rs`
- Modify: `src-tauri/src/runtime/agent/agent_runtime.rs`
- Modify: `src-tauri/src/runtime/agent/invocation.rs`
- Modify: `src-tauri/src/runtime/store/agent_invocation_store.rs`
- Modify: `src-tauri/src/runtime/agent/file_agent_invocation_store.rs`
- Modify: `src-tauri/tests/background_run_message_bridge_test.rs`
- Modify: `src-tauri/tests/subagent_result_envelope_test.rs`
- Create: `src-tauri/tests/subagent_transcript_persistence_test.rs`

- [ ] **Step I5-1: 写失败测试**

```rust
use app_lib::runtime::agent::subagent_transcript_store::SubagentTranscriptEntryRecord;
use app_lib::runtime::agent::{AgentRuntime, SpawnChildRunRequest};
use app_lib::runtime::agent::subagent_result_envelope::{
    build_subagent_transcript_ref, SubAgentResultEnvelope,
};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::ids::{RunId, SessionId};
use tempfile::TempDir;

#[test]
fn transcript_ref_builder_uses_subagent_scheme() {
    assert_eq!(
        build_subagent_transcript_ref("child-run-42"),
        "subagent://child-run-42"
    );
}
```

```rust
#[tokio::test]
async fn background_completion_persists_summary_and_transcript_ref_together() {
    let temp = TempDir::new().unwrap();
    let runtime = AgentRuntime::from_storage(
        temp.path().join("agent_invocations.json"),
        temp.path().join("subagent_transcripts"),
    )
    .unwrap();
    let bus = RuntimeEventBus::new();
    let session_id = SessionId::new("sess-i5");
    let parent_run_id = RunId::new("run-parent-i5");

    let mut request = SpawnChildRunRequest::for_test(parent_run_id.clone());
    request.background = true;
    let handle = runtime.spawn_child_run(request).await.unwrap();

    let transcript_ref = build_subagent_transcript_ref(handle.child_run_id().as_str());
    runtime
        .store_transcript(
            &transcript_ref,
            &[SubagentTranscriptEntryRecord {
                role: "assistant".to_string(),
                content: "done".to_string(),
                tool_call_id: None,
                tool_name: None,
            }],
        )
        .unwrap();

    let envelope = SubAgentResultEnvelope {
        schema_version: 1,
        output: "done".to_string(),
        iterations_used: 1,
        generated_files: Vec::new(),
        terminal_tool_results: Vec::new(),
        transcript_snapshot: Vec::new(),
        transcript_ref: Some(transcript_ref.clone()),
    };
    let summary = envelope.to_storage_summary();

    runtime
        .complete_background_run(
            handle.child_run_id(),
            Some(&summary),
            Some(&transcript_ref),
            session_id,
            parent_run_id,
            bus,
        )
        .await
        .unwrap();

    let stored_summary = runtime.get_summary(handle.child_run_id()).await.unwrap().unwrap();
    let decoded = SubAgentResultEnvelope::from_storage_summary(&stored_summary).unwrap();
    let stored_ref = runtime.get_transcript_ref(handle.child_run_id()).await.unwrap();

    assert_eq!(decoded.transcript_ref.as_deref(), Some(transcript_ref.as_str()));
    assert_eq!(stored_ref.as_deref(), Some(transcript_ref.as_str()));
}
```

- [ ] **Step I5-2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test subagent_transcript_persistence_test -- --nocapture`
Expected: FAIL。

- [ ] **Step I5-3: 统一 `transcript_ref` 格式**

```rust
// src-tauri/src/runtime/agent/subagent_result_envelope.rs
pub fn build_subagent_transcript_ref(child_run_id: &str) -> String {
    format!("subagent://{child_run_id}")
}
```

- [ ] **Step I5-4: `sub_agent.rs` 在构造 envelope 前把完整 child transcript 写入 store，再单独裁剪 envelope snapshot**

```rust
// src-tauri/src/llm/sub_agent.rs
let transcript_ref = build_subagent_transcript_ref(child_run_id.as_str());
let transcript_entries = messages
    .iter()
    .map(|message| SubagentTranscriptEntryRecord {
        role: message.role.clone(),
        content: message.content.clone(),
        tool_call_id: message.tool_call_id.clone(),
        tool_name: message.name.clone(),
    })
    .collect::<Vec<_>>();
agent_runtime.store_transcript(&transcript_ref, &transcript_entries)?;

let transcript_snapshot = transcript_entries
    .iter()
    .rev()
    .take(16)
    .map(|entry| SubAgentTranscriptEntry {
        role: entry.role.clone(),
        content: entry.content.clone(),
        tool_call_id: entry.tool_call_id.clone(),
        tool_name: entry.tool_name.clone(),
    })
    .collect::<Vec<_>>();
```

- [ ] **Step I5-5: invocation / record 模型显式新增 `transcript_ref`，并提供原子更新接口**

```rust
// src-tauri/src/runtime/store/agent_invocation_store.rs
pub trait AgentInvocationStore: Send + Sync {
    fn update_invocation_result_metadata(
        &self,
        agent_id: &AgentId,
        summary: Option<String>,
        transcript_ref: Option<String>,
    ) -> Result<()>;
}
```

```rust
// src-tauri/src/runtime/agent/agent_runtime.rs
pub async fn get_transcript_ref(&self, child_run_id: &RunId) -> Result<Option<String>> {
    for record in self.invocation_store.list_invocations()? {
        if &record.child_run_id == child_run_id {
            return Ok(record.transcript_ref.clone());
        }
    }
    Ok(None)
}

pub async fn complete_background_run(
    &self,
    child_run_id: &RunId,
    summary: Option<&str>,
    transcript_ref: Option<&str>,
    session_id: SessionId,
    parent_run_id: RunId,
    bus: RuntimeEventBus,
) -> Result<()> {
    let mut target_agent_id = None;
    for record in self.invocation_store.list_invocations()? {
        if &record.child_run_id == child_run_id {
            target_agent_id = Some(record.agent_id.clone());
            self.invocation_store
                .update_invocation_status(&record.agent_id, AgentStatus::Completed)?;
            self.invocation_store.update_invocation_result_metadata(
                &record.agent_id,
                summary.map(str::to_owned),
                transcript_ref.map(str::to_owned),
            )?;
        }
    }
    if let Some(agent_id) = target_agent_id {
        let event = RuntimeEvent::new(session_id, parent_run_id, bridge_agent_summary(agent_id));
        bus.emit(event).await?;
    }
    Ok(())
}
```

- [ ] **Step I5-6: file-backed store 跟随模型扩展**

```rust
// src-tauri/src/runtime/agent/file_agent_invocation_store.rs
// 直接跟随 AgentInvocationRecord 的 serde 结构；
// 新增 transcript_ref 为 Option<String>，旧 JSON 记录缺字段时按 None 处理。
```

- [ ] **Step I5-7: 运行测试确认通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test subagent_result_envelope_test -- --nocapture && cargo test subagent_transcript_persistence_test -- --nocapture && cargo test background_run_message_bridge_test -- --nocapture`
Expected: PASS。

- [ ] **Step I5-8: Commit**

```bash
git add src-tauri/src/llm/sub_agent.rs src-tauri/src/runtime/agent/subagent_result_envelope.rs src-tauri/src/runtime/agent/agent_runtime.rs src-tauri/src/runtime/agent/invocation.rs src-tauri/src/runtime/store/agent_invocation_store.rs src-tauri/src/runtime/agent/file_agent_invocation_store.rs src-tauri/tests/background_run_message_bridge_test.rs src-tauri/tests/subagent_result_envelope_test.rs src-tauri/tests/subagent_transcript_persistence_test.rs
git commit -m "feat(subagent): persist transcript refs alongside background completion metadata — I5"
```

---

## Task I6：让 parent/background retrieval 能通过已持久化的 `transcript_ref` 真正装载 transcript payload

**Files:**
- Modify: `src-tauri/src/runtime/agent/agent_runtime.rs`
- Modify: `src-tauri/src/runtime/agent/subagent_transcript_store.rs`
- Create: `src-tauri/tests/subagent_parent_transcript_access_test.rs`

- [ ] **Step I6-1: 写失败测试**

```rust
use app_lib::runtime::agent::subagent_result_envelope::build_subagent_transcript_ref;
use app_lib::runtime::agent::subagent_transcript_store::SubagentTranscriptEntryRecord;
use app_lib::runtime::agent::{AgentRuntime, SpawnChildRunRequest};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::ids::{RunId, SessionId};
use tempfile::TempDir;

#[tokio::test]
async fn parent_can_load_transcript_entries_via_child_run_id() {
    let temp = TempDir::new().unwrap();
    let runtime = AgentRuntime::from_storage(
        temp.path().join("agent_invocations.json"),
        temp.path().join("subagent_transcripts"),
    )
    .unwrap();

    let parent_run_id = RunId::new("run-parent-i6");
    let mut request = SpawnChildRunRequest::for_test(parent_run_id.clone());
    request.background = true;
    let handle = runtime.spawn_child_run(request).await.unwrap();

    let transcript_ref = build_subagent_transcript_ref(handle.child_run_id().as_str());
    runtime
        .store_transcript(
            &transcript_ref,
            &[SubagentTranscriptEntryRecord {
                role: "assistant".to_string(),
                content: "done".to_string(),
                tool_call_id: None,
                tool_name: None,
            }],
        )
        .unwrap();

    let summary = format!(
        "subagent-envelope:v1:{{\"schemaVersion\":1,\"output\":\"done\",\"iterationsUsed\":1,\"transcriptRef\":\"{}\"}}",
        transcript_ref
    );

    runtime
        .complete_background_run(
            handle.child_run_id(),
            Some(&summary),
            Some(&transcript_ref),
            SessionId::new("sess-i6"),
            parent_run_id,
            RuntimeEventBus::new(),
        )
        .await
        .unwrap();

    let loaded_ref = runtime.get_transcript_ref(handle.child_run_id()).await.unwrap();
    let loaded = runtime.load_transcript(handle.child_run_id()).await.unwrap().unwrap();

    assert_eq!(loaded_ref.as_deref(), Some(transcript_ref.as_str()));
    assert_eq!(loaded[0].content, "done");
}
```

- [ ] **Step I6-2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test subagent_parent_transcript_access_test -- --nocapture`
Expected: FAIL，报 `load_transcript` 不存在，或 runtime 还不能按 `child_run_id` 取回 transcript payload。

- [ ] **Step I6-3: `AgentRuntime` 增加 transcript retrieval API**

```rust
// src-tauri/src/runtime/agent/agent_runtime.rs
pub async fn load_transcript(
    &self,
    child_run_id: &RunId,
) -> Result<Option<Vec<SubagentTranscriptEntryRecord>>> {
    let Some(transcript_ref) = self.get_transcript_ref(child_run_id).await? else {
        return Ok(None);
    };
    self.transcript_store.get(&transcript_ref)
}
```

- [ ] **Step I6-4: 运行测试确认通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test subagent_parent_transcript_access_test -- --nocapture`
Expected: PASS。

- [ ] **Step I6-5: 全量回归**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_ --tests --no-fail-fast && cargo test --tests --no-fail-fast 2>&1 | grep -E "FAILED|^error"`
Expected: `review_` 通过，grep 无输出。

- [ ] **Step I6-6: Commit**

```bash
git add src-tauri/src/runtime/agent/agent_runtime.rs src-tauri/src/runtime/agent/subagent_transcript_store.rs src-tauri/tests/subagent_parent_transcript_access_test.rs
git commit -m "refactor(subagent): make transcript refs loadable from parent runtime flows — I6"
```

---

## 完成标准（Definition of Done）

- `SessionRuntime` 成为 active session cancel root owner；不再依赖外部注入的 `cancel_token` 模板字段。
- 同一 session 的 cancel root 在 active run 内复用，但一旦被 `Interrupt`/`clear_session_state` 打断，下一次同 session 新 turn 会自动轮换 fresh root。
- `stop_streaming` 先触发 runtime `cancel_session(..., Interrupt)`，再走 gateway/python interrupt bridge。
- `RuntimeRunRegistry` 被明确收紧为 stream cancel bridge + run-id/busy registry，不再被误解为 runtime cancel owner。
- `AgentRuntime` 同时拥有 invocation store 与 transcript store；`transcript_ref` 统一采用 `subagent://<child_run_id>`。
- child/background run completion 会写真实 transcript payload，并把 `transcript_ref` 显式持久化到 invocation record。
- parent/background retrieval 可以通过 `child_run_id -> transcript_ref -> transcript` 取回 transcript，而不是只剩 summary 字符串。
- 所有新增测试与 `review_` 架构回归通过。

---

## 推荐执行顺序

1. I1
2. I2
3. I3
4. I4
5. I5
6. I6

原因：前 3 个 task 先把 cancel owner 真源收紧，并补上 `AbortController` 单次使用对应的 root 轮换语义；后 3 个 task 再补 sidechain transcript parity，这样不会在 transcript 方案还没稳定时继续背负两套 cancel 语义或半成品恢复链路。
