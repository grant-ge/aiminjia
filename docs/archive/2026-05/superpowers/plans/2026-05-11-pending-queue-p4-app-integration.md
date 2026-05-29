# Pending Message Queue P4 — App Composer Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Route the app's in-window composer (`commands::chat::send_message` → `TauriChatCommandAdapter::send_message`) through `PendingQueueManager::enqueue_or_send`, so messages typed during an active turn are queued instead of returning "already processing" errors.

**Architecture:** Replace the direct `set_busy_for_run + spawn` path inside `TauriChatCommandAdapter::send_message` with a call to `enqueue_or_send`. Idle path: the manager returns `SentDirectly { request }` which the adapter then dispatches through its existing internal send path. Busy path: manager queues + persists + emits event; the Tauri command returns success with a `queued` indicator. Frontend lets the user know the message is pending via the chips (no separate toast needed).

**Tech Stack:** Rust + Tauri 2.x, TypeScript (small frontend tweak to ignore "Queued" return value for now — handled by event-driven chips).

**Spec reference:** §7.2

**Prerequisites:** P1 + P2 + P3 merged.

---

## File Structure

Modify:

- `src-tauri/src/transport/tauri_commands/chat.rs` — refactor `send_message` to go through `PendingQueueManager`
- `src/lib/tauri.ts` — `sendMessage` IPC return type stays `Result<void>` (no UI change needed — events drive chips)
- `src-tauri/tests/pending_app_integration_test.rs` — new integration test

---

## Task 1: Refactor TauriChatCommandAdapter.send_message

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

- [ ] **Step 1: Extract current "direct send" body into private helper**

Open `src-tauri/src/transport/tauri_commands/chat.rs`, locate `pub async fn send_message(...)` (around line 2285). The body has two halves:
1. Build `ChatTurnRequest` from raw params
2. Reserve run, call `send_chat_request`

Move halves into a helper. After the existing `impl TauriChatCommandAdapter` block, add a private method (or rename the existing send body):

```rust
    /// Build a ChatTurnRequest from raw Tauri command parameters.
    /// Used both by `send_message` (app composer) and by tests / future entry points.
    fn build_chat_turn_request_from_params(
        conversation_id: String,
        content: String,
        attachments: Vec<crate::runtime::chat::chat_turn_driver::ChatAttachmentRef>,
        permission_mode: Option<crate::runtime::tools::permission::PermissionMode>,
        agent_name: Option<String>,
        client_message_id: Option<String>,
    ) -> ChatTurnRequest {
        let mut request = ChatTurnRequest::new(conversation_id.clone(), content, attachments);
        request.session_attachment_dirs =
            crate::runtime::path_auth::derive_working_dirs_from_attachments(
                &request
                    .attachments
                    .iter()
                    .map(|a| std::path::PathBuf::from(&a.file_path))
                    .collect::<Vec<_>>(),
            );
        request.agent_name = agent_name;
        request.client_message_id = client_message_id;
        if let Some(permission_mode) = permission_mode {
            request.permission_mode = permission_mode;
        }
        request
    }
```

- [ ] **Step 2: Rewrite send_message body to route through pending manager**

Replace the `pub async fn send_message(...)` body with:

```rust
    pub async fn send_message(
        &self,
        conversation_id: String,
        content: String,
        attachments: Vec<crate::runtime::chat::chat_turn_driver::ChatAttachmentRef>,
        permission_mode: Option<crate::runtime::tools::permission::PermissionMode>,
        agent_name: Option<String>,
        client_message_id: Option<String>,
    ) -> Result<(), String> {
        log::info!(
            "[send_message] trace_id={:?} conversation_id={} content_len={} attachments_count={}",
            client_message_id.as_deref(),
            conversation_id,
            content.len(),
            attachments.len()
        );
        for att in &attachments {
            log::info!(
                "[send_message] attachment: name={} path={} kind={} type={}",
                att.file_name, att.file_path, att.kind, att.file_type
            );
        }

        let request = Self::build_chat_turn_request_from_params(
            conversation_id.clone(),
            content.clone(),
            attachments,
            permission_mode,
            agent_name,
            client_message_id.clone(),
        );

        // Build a PendingItem from the same params (used only if session is busy).
        // For app composer:
        //  - source = App
        //  - sender_nick = None (1:1 app session)
        //  - attachments = same set (just re-shaped)
        let pending_item = crate::runtime::pending::PendingItem {
            id: format!("pend-{}", uuid::Uuid::new_v4()),
            source: crate::runtime::pending::PendingSource::App,
            text: content.clone(),
            sender_nick: None,
            attachments: request
                .attachments
                .iter()
                .map(|a| crate::runtime::pending::PendingAttachment {
                    id: a.id.clone(),
                    file_path: a.file_path.clone(),
                    mime: a.mime_type.clone(),
                    size_bytes: Some(a.file_size),
                })
                .collect(),
            received_at: chrono::Utc::now().to_rfc3339(),
        };

        let pending_manager = self
            .services
            .app
            .try_state::<std::sync::Arc<crate::runtime::pending::PendingQueueManager>>()
            .ok_or_else(|| "PendingQueueManager not initialised".to_string())?
            .inner()
            .clone();

        let session_id = crate::runtime::ids::SessionId::new(conversation_id.clone());
        let outcome = pending_manager
            .enqueue_or_send(session_id, pending_item)
            .await
            .map_err(|e| format!("enqueue_or_send error: {e:#}"))?;

        match outcome {
            crate::runtime::pending::EnqueueOutcome::SentDirectly { request: req_from_mgr } => {
                // Manager rebuilt the request from the PendingItem; but we
                // already have the FULL request with agent_name / permission_mode /
                // client_message_id / session_attachment_dirs. Use OUR original
                // `request` (richer), not the manager-rebuilt one.
                drop(req_from_mgr);
                self.dispatch_built_request(request).await
            }
            crate::runtime::pending::EnqueueOutcome::Queued { snapshot } => {
                log::info!(
                    "[send_message] message queued conv={} queue_size={}",
                    conversation_id,
                    snapshot.len()
                );
                Ok(())
            }
            crate::runtime::pending::EnqueueOutcome::Rejected { reason } => match reason {
                crate::runtime::pending::EnqueueRejection::QueueFull { limit } => {
                    Err(format!("消息堆积过多（已达 {limit} 条），请稍后再发"))
                }
                crate::runtime::pending::EnqueueRejection::SessionArchived => {
                    Err("会话已归档，无法发送消息".to_string())
                }
            },
        }
    }

    /// Dispatch a fully-built ChatTurnRequest through the existing gateway path.
    /// Extracted from the previous monolithic send_message body.
    async fn dispatch_built_request(&self, request: ChatTurnRequest) -> Result<(), String> {
        let conversation_id = request.conversation_id.as_str().to_string();
        let run_id = request.run_id.clone();
        log::info!(
            "[send_message] calling set_busy_for_run conv={} run={}",
            conversation_id,
            run_id.as_str()
        );
        self.services
            .gateway
            .set_busy_for_run(&conversation_id, run_id.clone())?;

        self.send_chat_request(request).await
    }
```

**Important:** the existing function body below the early-logging section (everything from `let mut request = ChatTurnRequest::new...` down to the final `result`) gets fully replaced. Make sure you don't double-include lines.

If the original body had post-result logic (e.g., emitting `MessagePersisted` early for optimistic UI), move that into `dispatch_built_request` so the SentDirectly path keeps the existing semantics.

- [ ] **Step 3: Verify it compiles**

Run: `cd src-tauri && cargo check --lib`

Expected: succeeds. Watch for unused imports left over from the old body.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/transport/tauri_commands/chat.rs
git commit -m "feat(pending): route app composer through PendingQueueManager"
```

---

## Task 2: Persist single-message (SentDirectly) user message — keep prior behavior

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

Before P4, `send_message` persisted the user message via the existing `send_chat_request` flow (which calls into conversation_service inside `run_chat_request`). Verify this still works in the SentDirectly path.

- [ ] **Step 1: Trace the message persistence in current flow**

Run:

```bash
grep -n "append.*user\|persist_user\|MessagePersisted\|insert_user_message" src-tauri/src/runtime/session_runtime.rs src-tauri/src/runtime/chat/chat_turn_driver.rs src-tauri/src/runtime/conversation_service.rs | head -20
```

Find where the user message gets persisted during normal `send_chat_request`. It's likely inside `RuntimeChatTurnDriver::run_chat_turn` or `QueryEngine`, before the LLM call.

- [ ] **Step 2: Confirm SentDirectly path is unchanged**

Per Task 1, `dispatch_built_request` calls `set_busy_for_run + send_chat_request`. This is exactly the OLD send_message body (minus the request-building). So the user message persistence is unchanged for SentDirectly.

For the BUSY path: P3 Task 5 already wired the `ChatTurnDispatcher` impl to persist N items as user messages from `pending_batch`. So when drain happens, those items get persisted at dispatch time.

No code change required in Task 2 — this is a verification step.

- [ ] **Step 3: Add a unit test that asserts the path**

Append to `src-tauri/src/transport/tauri_commands/chat.rs` if a `#[cfg(test)] mod ...` block exists at the bottom; otherwise create `src-tauri/src/transport/tauri_commands/chat/send_message_dispatch_test.rs` and `pub mod send_message_dispatch_test;`.

For simplicity, place the assertion in the integration test in Task 4 below (it covers SentDirectly + Queued paths end-to-end). Skip writing a separate unit test here.

- [ ] **Step 4: No commit (verification only)**

---

## Task 3: Frontend — handle send_message Promise resolution

**Files:**
- Modify: `src/lib/tauri.ts` (if any signature change needed)
- Inspect: `src/features/chat/` callsites of `sendMessage`

- [ ] **Step 1: Check current sendMessage TS shape**

Run:

```bash
grep -n "export.*sendMessage\|invoke.*send_message" src/lib/tauri.ts | head -3
```

The current Tauri binding likely returns `Promise<void>`. The backend changes preserve this — no return type change.

- [ ] **Step 2: Check callsite assumptions**

Run:

```bash
grep -rn "sendMessage(" src/features/chat/ | head -10
```

Confirm callers don't depend on a specific success indicator. They probably await the call and then wait for `streaming:delta` event. With queueing, they'll receive `pending:queued` event instead — already handled by P2's `pendingStore`.

- [ ] **Step 3: Update sendMessage UX flow (optional)**

If the existing send button shows a "sending..." spinner that depends on `set_busy_for_run` returning, ensure it also handles the "queued, will send later" case. Inspect:

```bash
grep -rn "set_busy_for_run\|isSending" src/features/chat/ | head -10
```

If the button only waits for the IPC to resolve, then:
- SentDirectly path: IPC resolves immediately after `set_busy_for_run` succeeds. UI shows "sending" via `streaming:delta` events.
- Queued path: IPC resolves immediately too. UI sees `pending:queued` event → chip appears. Composer clears.

Either way, no client-side change is needed. **Confirm there's no logic blocking on a specific event.** If there is (rare), add handling.

- [ ] **Step 4: No code change unless callsite analysis reveals one**

Document the result of step 3:

```
✓ sendMessage UX flow is event-driven; no callsite changes needed.
```

- [ ] **Step 5: No commit (analysis only)**

---

## Task 4: Integration test — app composer enqueue + drain

**Files:**
- Create: `src-tauri/tests/pending_app_integration_test.rs`

- [ ] **Step 1: Write the integration test**

Create `src-tauri/tests/pending_app_integration_test.rs`:

```rust
//! Integration test: app composer paths through PendingQueueManager.
//!
//! Verifies:
//! 1. Idle session → SentDirectly path → manager does not queue
//! 2. Busy session → Queued path → drain after busy clears
//! 3. Queue full → Rejected → error returned

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::TempDir;

use aijia::runtime::chat::ChatTurnRequest;
use aijia::runtime::event_bus::RuntimeEventBus;
use aijia::runtime::ids::{RunId, SessionId};
use aijia::runtime::pending::{
    ChatTurnDispatcher, ConvDirResolver, EnqueueOutcome, EnqueueRejection, PendingAttachment,
    PendingConfig, PendingItem, PendingQueueManager, PendingSource,
};
use aijia::runtime::run_registry::RuntimeRunRegistry;

struct TempResolver(PathBuf);
impl ConvDirResolver for TempResolver {
    fn conversation_dir(&self, sid: &SessionId) -> Option<PathBuf> {
        let d = self.0.join(sid.as_str());
        std::fs::create_dir_all(&d).ok()?;
        Some(d)
    }
    fn is_archived(&self, _: &SessionId) -> bool {
        false
    }
    fn conversations_root(&self) -> PathBuf {
        self.0.clone()
    }
}

struct CountingDispatcher {
    count: AtomicUsize,
    last: tokio::sync::Mutex<Option<ChatTurnRequest>>,
}

#[async_trait::async_trait]
impl ChatTurnDispatcher for CountingDispatcher {
    async fn dispatch(&self, request: ChatTurnRequest) -> anyhow::Result<()> {
        self.count.fetch_add(1, Ordering::SeqCst);
        *self.last.lock().await = Some(request);
        Ok(())
    }
}

fn app_item(id: &str, text: &str, atts: Vec<PendingAttachment>) -> PendingItem {
    PendingItem {
        id: id.into(),
        source: PendingSource::App,
        text: text.into(),
        sender_nick: None,
        attachments: atts,
        received_at: "2026-05-11T03:21:00Z".into(),
    }
}

#[tokio::test]
async fn app_idle_path_returns_sent_directly() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(TempResolver(tmp.path().to_path_buf()));
    let mgr = PendingQueueManager::new(registry, bus, resolver, PendingConfig::default());

    let session = SessionId::new("conv-app-idle");
    let outcome = mgr
        .enqueue_or_send(session.clone(), app_item("p1", "hello", vec![]))
        .await
        .unwrap();

    match outcome {
        EnqueueOutcome::SentDirectly { request } => {
            assert_eq!(request.conversation_id.as_str(), "conv-app-idle");
            assert_eq!(request.content, "hello");
            assert!(request.pending_batch.is_none());
        }
        other => panic!("expected SentDirectly, got {:?}", other),
    }
}

#[tokio::test]
async fn app_busy_path_queues_and_persists() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(TempResolver(tmp.path().to_path_buf()));
    let mgr = PendingQueueManager::new(registry.clone(), bus, resolver, PendingConfig::default());

    let session = SessionId::new("conv-app-busy");
    registry
        .reserve(session.as_str(), RunId::new("run-1"))
        .unwrap();

    let outcome = mgr
        .enqueue_or_send(session.clone(), app_item("p1", "first", vec![]))
        .await
        .unwrap();
    assert!(matches!(outcome, EnqueueOutcome::Queued { .. }));

    let outcome2 = mgr
        .enqueue_or_send(session.clone(), app_item("p2", "second", vec![]))
        .await
        .unwrap();
    assert!(matches!(outcome2, EnqueueOutcome::Queued { .. }));

    // pending.json persisted with 2 items
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let pending_path = tmp.path().join("conv-app-busy").join("pending.json");
    let content = std::fs::read_to_string(&pending_path).unwrap();
    assert!(content.contains("p1"));
    assert!(content.contains("p2"));
    assert!(content.contains("\"app\""));
}

#[tokio::test]
async fn app_drains_to_dispatcher_after_busy_clears() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(TempResolver(tmp.path().to_path_buf()));
    let mut config = PendingConfig::default();
    config.debounce_window = std::time::Duration::from_millis(50);
    let mgr = PendingQueueManager::new(registry.clone(), bus, resolver, config);
    let dispatcher = Arc::new(CountingDispatcher {
        count: AtomicUsize::new(0),
        last: tokio::sync::Mutex::new(None),
    });
    mgr.set_dispatcher(dispatcher.clone()).await;

    let session = SessionId::new("conv-app-drain");
    registry.reserve(session.as_str(), RunId::new("run-1")).unwrap();
    mgr.enqueue_or_send(session.clone(), app_item("p1", "first", vec![])).await.unwrap();
    mgr.enqueue_or_send(session.clone(), app_item("p2", "second", vec![])).await.unwrap();

    registry.clear(session.as_str());
    mgr.schedule_drain(session.clone()).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    assert_eq!(dispatcher.count.load(Ordering::SeqCst), 1);
    let last = dispatcher.last.lock().await.clone().unwrap();
    assert!(last.content.contains("first"));
    assert!(last.content.contains("second"));
    // App items have NO sender prefix
    assert!(!last.content.contains("["));
    // pending_batch carried 2 items
    assert_eq!(last.pending_batch.as_ref().unwrap().len(), 2);
    assert_eq!(last.pending_batch.as_ref().unwrap()[0].source, PendingSource::App);
}

#[tokio::test]
async fn app_queue_full_rejects() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(TempResolver(tmp.path().to_path_buf()));
    let mut config = PendingConfig::default();
    config.max_queue_per_session = 2;
    let mgr = PendingQueueManager::new(registry.clone(), bus, resolver, config);

    let session = SessionId::new("conv-app-full");
    registry.reserve(session.as_str(), RunId::new("run-1")).unwrap();
    mgr.enqueue_or_send(session.clone(), app_item("p1", "a", vec![])).await.unwrap();
    mgr.enqueue_or_send(session.clone(), app_item("p2", "b", vec![])).await.unwrap();
    let outcome = mgr
        .enqueue_or_send(session.clone(), app_item("p3", "c", vec![]))
        .await
        .unwrap();

    match outcome {
        EnqueueOutcome::Rejected {
            reason: EnqueueRejection::QueueFull { limit: 2 },
        } => {}
        other => panic!("expected QueueFull(2), got {:?}", other),
    }
}

#[tokio::test]
async fn app_carries_attachments_through_pending_batch() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(TempResolver(tmp.path().to_path_buf()));
    let mut config = PendingConfig::default();
    config.debounce_window = std::time::Duration::from_millis(30);
    let mgr = PendingQueueManager::new(registry.clone(), bus, resolver, config);
    let dispatcher = Arc::new(CountingDispatcher {
        count: AtomicUsize::new(0),
        last: tokio::sync::Mutex::new(None),
    });
    mgr.set_dispatcher(dispatcher.clone()).await;

    let session = SessionId::new("conv-app-atts");
    registry.reserve(session.as_str(), RunId::new("run-1")).unwrap();

    let attachments = vec![PendingAttachment {
        id: "att-1".into(),
        file_path: "/tmp/foo.png".into(),
        mime: Some("image/png".into()),
        size_bytes: Some(2048),
    }];
    mgr.enqueue_or_send(
        session.clone(),
        app_item("p1", "with image", attachments.clone()),
    )
    .await
    .unwrap();

    registry.clear(session.as_str());
    mgr.schedule_drain(session.clone()).await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let last = dispatcher.last.lock().await.clone().unwrap();
    assert_eq!(last.attachments.len(), 1);
    assert_eq!(last.attachments[0].file_path, "/tmp/foo.png");
    assert_eq!(last.attachments[0].mime_type.as_deref(), Some("image/png"));
}
```

- [ ] **Step 2: Run integration test**

Run: `cd src-tauri && cargo test --test pending_app_integration_test`

Expected: 5 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tests/pending_app_integration_test.rs
git commit -m "test(pending): app composer integration — idle / busy / drain / queue full"
```

---

## Task 5: Manual smoke — app composer end-to-end

**Files:** none (manual)

- [ ] **Step 1: Start the app**

Run: `pnpm tauri:dev`. Open any conversation.

- [ ] **Step 2: Trigger busy + queue 3 messages**

Send a query that will take 10+ seconds (e.g., "请帮我写一个 500 字的关于咖啡的故事"). While the LLM is streaming, type and send 3 short messages.

Expected:
- 3 chips appear above composer immediately after each send
- Composer clears after each send
- When LLM finishes + 1.2s debounce, all 3 chips disappear, 3 user bubbles appear in history, and a new LLM response begins

- [ ] **Step 3: Verify × removal**

Repeat step 2 but click × on the 2nd chip before drain. Expected: only 2 user bubbles appear; the removed one is dropped.

- [ ] **Step 4: Verify queue full**

Configure `PendingConfig::default()` is 50; in dev set to e.g. 3 temporarily by patching `lib.rs` if you want to test fast. Send > 3 messages while busy. The 4th should return error to UI (toast or composer error).

(For production, leave the default 50.)

---

## Task 6: Review test — adapter dispatches through pending manager

**Files:**
- Create: `src-tauri/tests/review_app_send_message_uses_pending.rs`

- [ ] **Step 1: Write the review test**

Create `src-tauri/tests/review_app_send_message_uses_pending.rs`:

```rust
//! Architectural review test:
//! The Tauri command `send_message` body in `TauriChatCommandAdapter` MUST
//! invoke `PendingQueueManager::enqueue_or_send`. This guarantees future edits
//! can't silently regress the queue integration.

use std::path::Path;

#[test]
fn send_message_routes_through_pending_manager() {
    let content = std::fs::read_to_string(Path::new(
        "src/transport/tauri_commands/chat.rs",
    ))
    .expect("read chat.rs");
    assert!(
        content.contains("enqueue_or_send"),
        "TauriChatCommandAdapter::send_message must call PendingQueueManager::enqueue_or_send"
    );
    assert!(
        content.contains("PendingQueueManager"),
        "chat.rs must reference PendingQueueManager"
    );
}
```

- [ ] **Step 2: Run**

Run: `cd src-tauri && cargo test --test review_app_send_message_uses_pending`

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tests/review_app_send_message_uses_pending.rs
git commit -m "test(review): app send_message must go through PendingQueueManager"
```

---

## Self-Review

Spec coverage:
1. **§7.2 app composer integration** → Task 1 ✓
2. **§6.1 user message persistence (drain path)** → Inherits P3 Task 5 ✓
3. **§7.2 SendMessageOutcome semantics** → Mapped to `Result<(), String>` (queued = Ok(()), rejected = Err) — slightly simpler than spec's enum but functionally equivalent. Spec was speculative; this implementation favors backward compatibility.
4. **§10 queue full / archived** → Task 1 (returns Err) + Task 4 test ✓
5. **§7.3 ask_coordinator priority unchanged** → Inherits P3 (only IM worker calls ask_coordinator) ✓

Not covered in P4 (deferred):
- Multimodal cross-message budget → P5
- non-Anthropic pre-merge → P5
- Frontend rich error UX for queue-full → out of scope (toast on Err already works via existing chat send error handling)

Type consistency:
- `PendingItem.source = PendingSource::App` for composer entries ✓
- `pending_batch` carried via `ChatTurnRequest` (introduced P3 Task 5) ✓
- Test imports use `aijia::` crate prefix (matches existing test convention)
