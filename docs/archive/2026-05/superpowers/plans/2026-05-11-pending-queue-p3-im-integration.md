# Pending Message Queue P3 — IM Worker Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Route DingTalk IM messages through `PendingQueueManager::enqueue_or_send` so that messages arriving while LLM is busy are queued + persisted + UI-visible, then merged into a single ChatTurnRequest after the current turn ends + 1.2s debounce.

**Architecture:** Replace the worker's current `spawn(send_chat_request)` with `enqueue_or_send`. Wire the manager's `dispatcher` to call `TauriChatCommandAdapter::send_chat_request`. Hook `schedule_drain` into `SessionRuntime` so it fires automatically after StreamDone. Persist drained items as N independent user messages to `messages.jsonl`.

**Tech Stack:** Rust, Tauri 2.x, existing `connector::channel`, existing `SessionRuntime` event hooks.

**Spec reference:** §7.1, §7.3, §10 (edge cases)

**Prerequisites:** P1 + P2 merged.

---

## File Structure

Modify:

- `src-tauri/src/connector/channel/manager.rs` — replace `spawn(send_chat_request)` with `PendingQueueManager::enqueue_or_send`
- `src-tauri/src/transport/tauri_commands/chat.rs` — implement `ChatTurnDispatcher` for `TauriChatCommandAdapter`; persist drained N items as user messages
- `src-tauri/src/runtime/session_runtime.rs` — call `pending_manager.schedule_drain(session)` after StreamDone
- `src-tauri/src/lib.rs` — set the dispatcher on `PendingQueueManager` after both manager and adapter exist

Create:

- `src-tauri/src/connector/channel/pending_adapter.rs` — small helper to convert `DownloadedFile` → `PendingItem` (so manager.rs doesn't grow further)
- `src-tauri/tests/pending_im_integration_test.rs` — integration test with fake `RuntimeRunRegistry` and recorded ChatTurnDispatcher

---

## Task 1: ChatTurnDispatcher impl for TauriChatCommandAdapter

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

- [ ] **Step 1: Add the impl block**

Open `src-tauri/src/transport/tauri_commands/chat.rs`. Find the existing `impl TauriChatCommandAdapter` block (around line 2019). After the existing `impl crate::runtime::schedule_runner::ScheduleRunDispatcher for TauriChatCommandAdapter` block, add:

```rust
#[async_trait::async_trait]
impl crate::runtime::pending::ChatTurnDispatcher for TauriChatCommandAdapter {
    async fn dispatch(&self, request: ChatTurnRequest) -> anyhow::Result<()> {
        self.send_chat_request(request)
            .await
            .map_err(|e| anyhow::anyhow!("dispatch via TauriChatCommandAdapter failed: {e}"))
    }
}
```

Export `ChatTurnDispatcher` from `runtime/pending/mod.rs` if not yet:

```bash
grep -n "ChatTurnDispatcher" src-tauri/src/runtime/pending/mod.rs
```

If absent, add:

```rust
pub use queue_manager::ChatTurnDispatcher;
```

- [ ] **Step 2: Wire dispatcher in lib.rs**

In `src-tauri/src/lib.rs`, locate where `app.manage(pending_manager)` is called (added in P2 Task 3). Both `pending_manager` and the chat adapter must exist; find the chat adapter creation site (`TauriChatCommandAdapter::new` call) and ensure it's created BEFORE we set the dispatcher.

Add immediately after the chat adapter is constructed and before app.manage of the pending manager (or just before `app.manage(pending_manager.clone())` — adjust ordering):

```rust
            // Wire pending manager's dispatcher to the chat adapter
            let chat_adapter_arc = std::sync::Arc::new(chat_adapter.clone());
            // (chat_adapter likely already an Arc — adjust to match the var type)
            let dispatcher_for_pending: std::sync::Arc<dyn crate::runtime::pending::ChatTurnDispatcher> =
                chat_adapter_arc.clone();
            tauri::async_runtime::block_on(pending_manager.set_dispatcher(dispatcher_for_pending));
```

**Variable name note:** the existing chat_adapter binding may be `chat_adapter: Arc<TauriChatCommandAdapter>` already. Inspect `src-tauri/src/lib.rs` around the setup:

```bash
grep -n "TauriChatCommandAdapter::new\|let chat_adapter\|chat_adapter\.clone" src-tauri/src/lib.rs | head -10
```

If `chat_adapter` is `TauriChatCommandAdapter` (not Arc), wrap it. If already Arc, just use it.

- [ ] **Step 3: Verify it compiles**

Run: `cd src-tauri && cargo check --lib`

Expected: succeeds.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/transport/tauri_commands/chat.rs src-tauri/src/runtime/pending/mod.rs src-tauri/src/lib.rs
git commit -m "feat(pending): wire TauriChatCommandAdapter as drain dispatcher"
```

---

## Task 2: Drain hook in SessionRuntime after StreamDone

**Files:**
- Modify: `src-tauri/src/runtime/session_runtime.rs`

- [ ] **Step 1: Inspect StreamDone emission site**

Run: `grep -n "StreamDone\|emit_stream_done\|TurnCompleted" src-tauri/src/runtime/session_runtime.rs | head -10`

Identify where the turn completion flow finishes. Likely inside `run_chat_request` or via the `RuntimeChatTurnDriver::run_chat_turn` return path.

- [ ] **Step 2: Add an optional PendingQueueManager dependency**

In `src-tauri/src/runtime/session_runtime.rs`, find the `SessionRuntime` struct. Add an optional manager:

```rust
pub struct SessionRuntime {
    // existing fields
    pending_manager: Option<std::sync::Arc<crate::runtime::pending::PendingQueueManager>>,
}
```

Add to `impl SessionRuntime`:

```rust
pub fn with_pending_manager(
    mut self,
    mgr: std::sync::Arc<crate::runtime::pending::PendingQueueManager>,
) -> Self {
    self.pending_manager = Some(mgr);
    self
}
```

In `SessionRuntime::new` (and any other constructor), initialize `pending_manager: None`.

- [ ] **Step 3: Schedule drain after run_chat_request**

Find `run_chat_request` (around line 180 per spec exploration). At the end of the function, AFTER the turn completes (success or error path, but only after `RunCompleted`/`TurnCompleted` semantics), call:

```rust
        // After turn finishes (success or otherwise), give the queue a chance
        // to flush any items buffered during this turn.
        if let Some(mgr) = self.pending_manager.as_ref() {
            mgr.schedule_drain(request.conversation_id.clone()).await;
        }
```

Place this immediately before the function returns, but inside the same async scope where `request.conversation_id` is still in scope. If the function has multiple return points, place a `let _result = ...; ...; result` reshape so the drain hook runs unconditionally.

A safer placement is via a `defer-like` block:

```rust
        let outcome = /* existing turn execution */;
        if let Some(mgr) = self.pending_manager.as_ref() {
            mgr.schedule_drain(session_id.clone()).await;
        }
        outcome
```

Adjust to the actual function shape. If you can't find a clean single return, refactor to `let outcome = ... ; drain ; return outcome`.

- [ ] **Step 4: Wire the manager into SessionRuntime in lib.rs**

In `src-tauri/src/lib.rs`, where `SessionRuntime` is constructed for `TauriChatCommandAdapter`, chain `.with_pending_manager(pending_manager.clone())`:

```bash
grep -n "SessionRuntime::new\|SessionRuntime\.\.\." src-tauri/src/lib.rs | head -5
```

Adapt — likely some `let session_runtime = SessionRuntime::new(...)...;` builder chain.

- [ ] **Step 5: Verify**

Run: `cd src-tauri && cargo check --lib && cargo test --lib pending`

Expected: P1/P2 unit tests still pass; new code compiles.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/session_runtime.rs src-tauri/src/lib.rs
git commit -m "feat(pending): SessionRuntime schedules drain after run_chat_request"
```

---

## Task 3: DingTalk → PendingItem helper

**Files:**
- Create: `src-tauri/src/connector/channel/pending_adapter.rs`
- Modify: `src-tauri/src/connector/channel/mod.rs` (add `pub mod pending_adapter;`)

- [ ] **Step 1: Write the helper**

Create `src-tauri/src/connector/channel/pending_adapter.rs`:

```rust
//! Adapter: turn a DingTalk message + downloaded attachments into a `PendingItem`.

use crate::runtime::chat::ChatAttachmentRef;
use crate::runtime::pending::{PendingAttachment, PendingItem, PendingSource};

use super::types::ConversationType;

/// Build a `PendingItem` from a downloaded DingTalk message.
///
/// - `sender_nick` is preserved as-is (used as the `[sender]:` prefix for group chats only)
/// - Attachments are converted; `mime_type` and `file_size` are passed through
pub fn build_pending_item_from_dingtalk(
    msg_id: &str,
    conv_type: &ConversationType,
    sender_nick: &str,
    text: &str,
    attachments: Vec<ChatAttachmentRef>,
    download_failures: &[String],
) -> PendingItem {
    let nick = match conv_type {
        ConversationType::Group => Some(sender_nick.to_string()),
        ConversationType::Private => None,
    };
    let body = if download_failures.is_empty() {
        text.to_string()
    } else {
        format!(
            "{}\n[注意：以下附件下载失败，未能加载：{}]",
            text,
            download_failures.join(", ")
        )
    };
    let pending_attachments: Vec<PendingAttachment> = attachments
        .into_iter()
        .map(|a| PendingAttachment {
            id: a.id,
            file_path: a.file_path,
            mime: a.mime_type,
            size_bytes: Some(a.file_size),
        })
        .collect();
    PendingItem {
        id: format!("pend-{}", uuid::Uuid::new_v4()),
        source: PendingSource::ImDingtalk,
        text: body,
        sender_nick: nick,
        attachments: pending_attachments,
        received_at: chrono::Utc::now().to_rfc3339(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn att(id: &str) -> ChatAttachmentRef {
        ChatAttachmentRef {
            id: id.into(),
            file_name: format!("{id}.png"),
            file_path: format!("/tmp/{id}.png"),
            kind: "image".into(),
            file_size: 100,
            file_type: "png".into(),
            mime_type: Some("image/png".into()),
        }
    }

    #[test]
    fn group_chat_carries_sender_nick() {
        let item = build_pending_item_from_dingtalk(
            "m-1",
            &ConversationType::Group,
            "张三",
            "hello",
            vec![att("a")],
            &[],
        );
        assert_eq!(item.source, PendingSource::ImDingtalk);
        assert_eq!(item.sender_nick.as_deref(), Some("张三"));
        assert_eq!(item.text, "hello");
        assert_eq!(item.attachments.len(), 1);
        assert_eq!(item.attachments[0].id, "a");
        assert_eq!(item.attachments[0].mime.as_deref(), Some("image/png"));
    }

    #[test]
    fn private_chat_omits_sender_nick() {
        let item = build_pending_item_from_dingtalk(
            "m-2",
            &ConversationType::Private,
            "李四",
            "hi",
            vec![],
            &[],
        );
        assert!(item.sender_nick.is_none());
    }

    #[test]
    fn download_failures_appended_to_text() {
        let item = build_pending_item_from_dingtalk(
            "m-3",
            &ConversationType::Private,
            "x",
            "hello",
            vec![],
            &["a.docx".into()],
        );
        assert!(item.text.contains("hello"));
        assert!(item.text.contains("a.docx"));
        assert!(item.text.contains("下载失败"));
    }

    #[test]
    fn item_id_has_pend_prefix() {
        let item = build_pending_item_from_dingtalk(
            "m-4",
            &ConversationType::Private,
            "",
            "",
            vec![],
            &[],
        );
        assert!(item.id.starts_with("pend-"));
    }
}
```

- [ ] **Step 2: Export from mod**

Modify `src-tauri/src/connector/channel/mod.rs` — add `pub mod pending_adapter;` alphabetically.

- [ ] **Step 3: Run tests**

Run: `cd src-tauri && cargo test --lib pending_adapter`

Expected: 4 tests PASS.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/connector/channel/pending_adapter.rs src-tauri/src/connector/channel/mod.rs
git commit -m "feat(channel): DingTalk message → PendingItem adapter"
```

---

## Task 4: Replace IM worker dispatch with enqueue_or_send

**Files:**
- Modify: `src-tauri/src/connector/channel/manager.rs`

- [ ] **Step 1: Pass PendingQueueManager into ChannelManager**

Find `pub struct ChannelManager` and its `new` or builder. Add a field:

```rust
pub struct ChannelManager {
    // existing
    pending_manager: std::sync::Arc<crate::runtime::pending::PendingQueueManager>,
}
```

Update the constructor to accept and store it. Find the call site (likely in `lib.rs` where `ChannelManager::new(...)` is called) and pass `pending_manager.clone()` in.

```bash
grep -n "ChannelManager::new\|impl ChannelManager" src-tauri/src/connector/channel/manager.rs | head -5
grep -n "ChannelManager::new\|ChannelManager {" src-tauri/src/lib.rs | head -5
```

- [ ] **Step 2: Replace the worker dispatch lines**

In `connector/channel/manager.rs`, locate lines ~720–768 (the `let request = build_channel_chat_request(...)` followed by `tokio::spawn(adapter_for_turn.send_chat_request(...))`). Replace with:

```rust
                // Build PendingItem instead of ChatTurnRequest directly
                let pending_item = super::pending_adapter::build_pending_item_from_dingtalk(
                    &msg.msg_id,
                    &conv_type,
                    &sender_nick,
                    &text,
                    chat_attachments.clone(),
                    &download_failures,
                );

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }

                // Register reply manager BEFORE enqueue so cards exist for any
                // future turn this item might be part of. (We don't yet know
                // which run_id will dispatch — that's chosen by the queue
                // manager at drain time. For now, register with a placeholder
                // run_id; the reply manager will be re-registered by the
                // queue's eventual ChatTurnRequest.)
                //
                // Spec §7.1: reply_manager.register is still pre-enqueue.
                let card_target = match &conv_type {
                    ConversationType::Group => CardTarget::Group {
                        open_conversation_id: conv_key.clone(),
                    },
                    ConversationType::Private => CardTarget::Private {
                        user_id: msg.sender_id.clone(),
                    },
                };

                // Use the pending_item.id as a stable handle until drain
                // assigns the real run_id. Reply manager keyed by session+pending.
                let placeholder_run_id = pending_item.id.clone();
                let register_reply = reply_manager_ref.register(
                    session_id.clone(),
                    placeholder_run_id,
                    reply_app_key.clone(),
                    reply_app_secret.clone(),
                    reply_robot_code.clone(),
                    card_target.clone(),
                );
                tokio::select! {
                    biased;
                    _ = message_cancel.cancelled() => break,
                    _ = register_reply => {}
                }

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }

                // Enqueue (idle path returns SentDirectly; busy path returns Queued).
                let session_for_enqueue = SessionId::new(session_id.clone());
                let pending_manager_clone = pending_manager_ref.clone();
                let adapter_for_send = Arc::clone(&adapter);
                let session_for_log = session_id.clone();
                let webhook_for_reject = msg.session_webhook.clone();
                tokio::spawn(async move {
                    match pending_manager_clone
                        .enqueue_or_send(session_for_enqueue, pending_item)
                        .await
                    {
                        Ok(crate::runtime::pending::EnqueueOutcome::SentDirectly { request }) => {
                            if let Err(e) = adapter_for_send.send_chat_request(request).await {
                                log::error!(
                                    "[channel] send_chat_request failed session={}: {}",
                                    session_for_log, e
                                );
                            }
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::Queued { snapshot }) => {
                            log::info!(
                                "[channel] message queued session={} queue_size={}",
                                session_for_log,
                                snapshot.len()
                            );
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::Rejected { reason }) => {
                            log::warn!(
                                "[channel] enqueue rejected session={} reason={:?}",
                                session_for_log, reason
                            );
                            if let crate::runtime::pending::EnqueueRejection::QueueFull { limit } =
                                reason
                            {
                                if let Some(webhook) = webhook_for_reject {
                                    let text = format!(
                                        "消息堆积过多（已达 {limit} 条），请稍后再发。"
                                    );
                                    tokio::spawn(
                                        super::dingtalk_stream::send_session_webhook_text(
                                            webhook, text,
                                        ),
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            log::error!(
                                "[channel] enqueue_or_send error session={}: {:#}",
                                session_for_log, e
                            );
                        }
                    }
                });
```

Above the spawn, you need `pending_manager_ref` in scope. Just below where other refs are cloned (`adapter`, `reply_manager_ref` etc., near line 484–490), add:

```rust
        let pending_manager_ref = self.pending_manager.clone();
```

And before the `let message_handle = tokio::spawn(async move {` block, ensure `let pending_manager_ref = pending_manager_ref.clone();` is moved into the closure.

Also add the use:

```rust
use crate::runtime::ids::SessionId;
```

at top of file if not already imported.

- [ ] **Step 3: Verify it compiles**

Run: `cd src-tauri && cargo check --lib`

Expected: succeeds. Resolve any type errors by reading the surrounding worker closure and confirming all moved variables exist.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/connector/channel/manager.rs src-tauri/src/lib.rs
git commit -m "feat(channel): route DingTalk worker through PendingQueueManager::enqueue_or_send"
```

---

## Task 5: Persist N drained items as user messages

The drain dispatcher currently builds ONE merged `ChatTurnRequest` and sends it. To match spec §6.1 (落 N 条独立 user message), we need to persist each pending item as its own user message in `messages.jsonl` BEFORE the LLM call.

There are two clean options:
- **A.** Persist in `PendingQueueManager::drain_and_dispatch` before calling the dispatcher (requires manager to know about ConversationStore — coupling).
- **B.** Persist inside `TauriChatCommandAdapter::dispatch` (manager stays decoupled; adapter has access to conversation_service).

We pick **B** to keep the manager pure.

**Files:**
- Modify: `src-tauri/src/runtime/pending/queue_manager.rs` (carry per-item info into ChatTurnRequest)
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs` (dispatch impl persists N items)

- [ ] **Step 1: Carry per-item info through ChatTurnRequest extra field**

We need to pass the original `Vec<PendingItem>` down to the dispatcher, but `ChatTurnRequest` is shared across many code paths. Add an OPTIONAL field on `ChatTurnRequest`:

In `src-tauri/src/runtime/chat/chat_turn_driver.rs` (struct `ChatTurnRequest`), add at the end of the struct:

```rust
    /// When set, this turn originated from a drained PendingQueue batch.
    /// The dispatcher should persist each item as an independent user message
    /// before invoking the LLM. Default: None (= single-item turn, existing path).
    #[allow(dead_code)]
    pub pending_batch: Option<Vec<crate::runtime::pending::PendingItem>>,
```

Update `ChatTurnRequest::new` to initialize `pending_batch: None,`.

- [ ] **Step 2: Manager populates pending_batch in drain path**

In `src-tauri/src/runtime/pending/queue_manager.rs`, modify `build_request_from_batch` (introduced in P1 Task 6) to set:

```rust
fn build_request_from_batch(session_id: &SessionId, items: Vec<PendingItem>) -> ChatTurnRequest {
    let n = items.len();
    let mut content = String::new();
    if n > 1 {
        content.push_str(&format!("[以下是 {} 条新消息]\n", n));
    }
    let mut all_atts: Vec<ChatAttachmentRef> = Vec::new();
    for (idx, it) in items.iter().enumerate() {
        let prefix = match &it.sender_nick {
            Some(nick) if !nick.is_empty() => format!("[{}]: ", nick),
            _ => String::new(),
        };
        content.push_str(&prefix);
        content.push_str(&it.text);
        if idx + 1 < n {
            content.push('\n');
        }
        for a in &it.attachments {
            all_atts.push(ChatAttachmentRef {
                id: a.id.clone(),
                file_name: file_name_of(&a.file_path),
                file_path: a.file_path.clone(),
                kind: "file".to_string(),
                file_size: a.size_bytes.unwrap_or(0),
                file_type: a
                    .mime
                    .clone()
                    .unwrap_or_else(|| "application/octet-stream".to_string()),
                mime_type: a.mime.clone(),
            });
        }
    }
    let mut req = ChatTurnRequest::new(session_id.clone(), content, all_atts);
    req.pending_batch = Some(items);
    req
}
```

- [ ] **Step 3: Adapter persists N items before dispatching to LLM**

In `src-tauri/src/transport/tauri_commands/chat.rs`, modify the `ChatTurnDispatcher` impl created in Task 1:

```rust
#[async_trait::async_trait]
impl crate::runtime::pending::ChatTurnDispatcher for TauriChatCommandAdapter {
    async fn dispatch(&self, mut request: ChatTurnRequest) -> anyhow::Result<()> {
        // If this turn originated from a drained PendingQueue batch, persist
        // each item as an independent user message before dispatching to LLM.
        if let Some(items) = request.pending_batch.take() {
            for item in &items {
                let prefix = item
                    .sender_nick
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .map(|s| format!("[{}]: ", s))
                    .unwrap_or_default();
                let content = format!("{}{}", prefix, item.text);
                let attachments: Vec<crate::runtime::chat::ChatAttachmentRef> = item
                    .attachments
                    .iter()
                    .map(|a| crate::runtime::chat::ChatAttachmentRef {
                        id: a.id.clone(),
                        file_name: std::path::Path::new(&a.file_path)
                            .file_name()
                            .and_then(|s| s.to_str())
                            .map(String::from)
                            .unwrap_or_else(|| a.file_path.clone()),
                        file_path: a.file_path.clone(),
                        kind: "file".into(),
                        file_size: a.size_bytes.unwrap_or(0),
                        file_type: a
                            .mime
                            .clone()
                            .unwrap_or_else(|| "application/octet-stream".into()),
                        mime_type: a.mime.clone(),
                    })
                    .collect();
                if let Err(e) = self.services.conversation_service.persist_user_message(
                    request.conversation_id.as_str(),
                    &content,
                    &attachments,
                    None, // client_message_id
                ) {
                    log::warn!(
                        "[pending] persist_user_message failed for item {}: {:#}",
                        item.id, e
                    );
                }
            }
        }

        self.send_chat_request(request)
            .await
            .map_err(|e| anyhow::anyhow!("dispatch via TauriChatCommandAdapter failed: {e}"))
    }
}
```

**API verification:** `self.services.conversation_service.persist_user_message(...)` — verify the actual name and signature:

```bash
grep -n "pub fn persist_user_message\|pub async fn persist_user_message\|persist_user_message" src-tauri/src/runtime/conversation_service.rs | head -5
```

If named differently (e.g., `append_user_message` or via `repository.append_message`), use the correct API. The function must (a) append a `user` role message to `messages.N.jsonl`, (b) emit `MessagePersisted` so UI sees the bubble. If no single-line helper exists, fall back to:

```rust
self.runtime.conversation_store_arc().append_message(...).await?;
```

or the equivalent. The exact integration point depends on the existing code; the engineer must match.

- [ ] **Step 4: Verify it compiles**

Run: `cd src-tauri && cargo check --lib && cargo test --lib pending`

Expected: P1/P2 tests still pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/chat/chat_turn_driver.rs src-tauri/src/runtime/pending/queue_manager.rs src-tauri/src/transport/tauri_commands/chat.rs
git commit -m "feat(pending): persist N drained items as independent user messages"
```

---

## Task 6: Integration test — IM enqueue + drain end-to-end

**Files:**
- Create: `src-tauri/tests/pending_im_integration_test.rs`

- [ ] **Step 1: Write the integration test**

Create `src-tauri/tests/pending_im_integration_test.rs`:

```rust
//! Integration test: enqueueing while busy, then drain after busy clears.
//!
//! Uses fake ChatTurnDispatcher to verify the merged ChatTurnRequest reaches
//! the dispatcher exactly once with the expected merged content.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::TempDir;

use aijia::runtime::chat::ChatTurnRequest;
use aijia::runtime::event_bus::RuntimeEventBus;
use aijia::runtime::ids::{RunId, SessionId};
use aijia::runtime::pending::{
    ChatTurnDispatcher, ConvDirResolver, EnqueueOutcome, PendingConfig, PendingItem,
    PendingQueueManager, PendingSource,
};
use aijia::runtime::run_registry::RuntimeRunRegistry;

struct TempResolver(PathBuf);
impl ConvDirResolver for TempResolver {
    fn conversation_dir(&self, sid: &SessionId) -> Option<PathBuf> {
        let d = self.0.join(sid.as_str());
        std::fs::create_dir_all(&d).ok()?;
        Some(d)
    }
    fn is_archived(&self, _sid: &SessionId) -> bool {
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

fn item(id: &str, sender: Option<&str>, text: &str) -> PendingItem {
    PendingItem {
        id: id.into(),
        source: PendingSource::ImDingtalk,
        text: text.into(),
        sender_nick: sender.map(String::from),
        attachments: vec![],
        received_at: "2026-05-11T03:21:00Z".into(),
    }
}

#[tokio::test]
async fn three_im_messages_merge_into_one_dispatch() {
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

    let session = SessionId::new("conv-im-merge");

    // Mark session busy
    registry
        .reserve(session.as_str(), RunId::new("run-1"))
        .unwrap();

    // Three IM messages arrive while busy
    let o1 = mgr
        .enqueue_or_send(session.clone(), item("p1", Some("张三"), "帮我看下"))
        .await
        .unwrap();
    let o2 = mgr
        .enqueue_or_send(session.clone(), item("p2", Some("李四"), "顺便看下这个"))
        .await
        .unwrap();
    let o3 = mgr
        .enqueue_or_send(session.clone(), item("p3", Some("张三"), "就是 Q1"))
        .await
        .unwrap();

    assert!(matches!(o1, EnqueueOutcome::Queued { .. }));
    assert!(matches!(o2, EnqueueOutcome::Queued { .. }));
    assert!(matches!(o3, EnqueueOutcome::Queued { .. }));

    // Free the session
    registry.clear(session.as_str());
    mgr.schedule_drain(session.clone()).await;

    // Wait > debounce
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    assert_eq!(dispatcher.count.load(Ordering::SeqCst), 1);
    let last = dispatcher.last.lock().await.clone().unwrap();
    assert_eq!(last.conversation_id.as_str(), "conv-im-merge");
    // Merged content order = received order
    let body = &last.content;
    let pos_a = body.find("帮我看下").unwrap();
    let pos_b = body.find("顺便看下这个").unwrap();
    let pos_c = body.find("就是 Q1").unwrap();
    assert!(pos_a < pos_b && pos_b < pos_c);
    // Sender prefixes preserved
    assert!(body.contains("[张三]: 帮我看下"));
    assert!(body.contains("[李四]: 顺便看下这个"));
    // pending_batch carried through
    assert!(last.pending_batch.is_some());
    assert_eq!(last.pending_batch.as_ref().unwrap().len(), 3);
}

#[tokio::test]
async fn idle_session_sends_immediately_without_queue() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(TempResolver(tmp.path().to_path_buf()));
    let mgr = PendingQueueManager::new(registry.clone(), bus, resolver, PendingConfig::default());
    let dispatcher = Arc::new(CountingDispatcher {
        count: AtomicUsize::new(0),
        last: tokio::sync::Mutex::new(None),
    });
    mgr.set_dispatcher(dispatcher.clone()).await;

    let session = SessionId::new("conv-idle");
    let outcome = mgr
        .enqueue_or_send(session.clone(), item("p1", Some("张三"), "single"))
        .await
        .unwrap();

    // Idle path: caller (this test) gets SentDirectly and must "dispatch"
    // it manually — manager does not auto-send on idle to keep callsite control.
    match outcome {
        EnqueueOutcome::SentDirectly { request } => {
            // Single-item content has NO "[以下是 N 条]" prefix
            assert!(!request.content.contains("[以下是"));
            // No pending_batch in idle path (test that contract)
            assert!(request.pending_batch.is_none());
        }
        other => panic!("expected SentDirectly, got {:?}", other),
    }
    // Dispatcher NOT called because idle path returns the request to caller
    assert_eq!(dispatcher.count.load(Ordering::SeqCst), 0);
}
```

- [ ] **Step 2: Run integration test**

Run: `cd src-tauri && cargo test --test pending_im_integration_test -- --nocapture`

Expected: 2 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tests/pending_im_integration_test.rs
git commit -m "test(pending): IM integration — 3-message merge + idle direct send"
```

---

## Task 7: Architecture review test

**Files:**
- Create: `src-tauri/tests/review_pending_im_decoupling.rs`

- [ ] **Step 1: Write the architecture test**

Create `src-tauri/tests/review_pending_im_decoupling.rs`:

```rust
//! Architecture review:
//! 1. `runtime/pending/` must not depend on Tauri
//! 2. `connector/channel/` must not depend on `runtime/pending/queue_manager` internals
//!    (only public API: enqueue_or_send / EnqueueOutcome)

use std::path::Path;

fn read_file(path: &str) -> String {
    std::fs::read_to_string(Path::new(path)).expect(path)
}

#[test]
fn runtime_pending_does_not_use_tauri() {
    for entry in walkdir::WalkDir::new("src/runtime/pending") {
        let entry = entry.unwrap();
        if entry.file_type().is_file()
            && entry.path().extension().map_or(false, |e| e == "rs")
        {
            let path = entry.path().to_string_lossy();
            let content = read_file(&path);
            assert!(
                !content.contains("use tauri::") && !content.contains("tauri::AppHandle"),
                "{} must not depend on Tauri (spec §11.2)",
                path
            );
        }
    }
}

#[test]
fn channel_manager_uses_only_public_pending_api() {
    let path = "src/connector/channel/manager.rs";
    let content = read_file(path);
    // Allow only re-exports from runtime::pending crate root, not deep internals
    assert!(
        !content.contains("runtime::pending::queue_manager::"),
        "channel/manager.rs must not reach into queue_manager internals"
    );
    assert!(
        !content.contains("runtime::pending::store::"),
        "channel/manager.rs must not reach into store internals"
    );
}
```

`walkdir` is a dev-dependency that may or may not be present. Verify:

```bash
grep -n "walkdir" src-tauri/Cargo.toml
```

If absent, simplify to enumerate known files manually:

```rust
const PENDING_FILES: &[&str] = &[
    "src/runtime/pending/mod.rs",
    "src/runtime/pending/types.rs",
    "src/runtime/pending/store.rs",
    "src/runtime/pending/queue_manager.rs",
    "src/runtime/pending/aijia_resolver.rs",
];

#[test]
fn runtime_pending_does_not_use_tauri() {
    for f in PENDING_FILES {
        let c = std::fs::read_to_string(f).expect(f);
        assert!(!c.contains("use tauri::"), "{} uses tauri", f);
    }
}
```

The `aijia_resolver` does depend on `crate::storage::AiJiaHome` and `UserScope` (from auth/state) — those are runtime-level types, not Tauri. Verify:

```bash
grep -n "use tauri" src-tauri/src/runtime/pending/aijia_resolver.rs
```

Should be 0 hits.

- [ ] **Step 2: Run review test**

Run: `cd src-tauri && cargo test --test review_pending_im_decoupling`

Expected: 2 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tests/review_pending_im_decoupling.rs
git commit -m "test(review): pending + channel architecture constraints"
```

---

## Task 8: Manual smoke — IM end-to-end

**Files:** none (manual)

- [ ] **Step 1: Start app + DingTalk channel**

Run: `pnpm tauri:dev`. Connect DingTalk channel via app settings.

- [ ] **Step 2: Trigger busy state**

Send a long-running query in a DingTalk-bound conversation (group or private).

- [ ] **Step 3: While LLM is responding, send 3 more DingTalk messages**

Expected:
- Chips appear above composer (app side) — possibly need to open the session in app to see
- Once first turn completes + 1.2s, chips disappear and 3 user bubbles appear in history, followed by 1 LLM response

- [ ] **Step 4: Verify queue cap**

Send > 50 messages back-to-back during busy. After 50, expect a webhook reply "消息堆积过多" in DingTalk.

- [ ] **Step 5: Verify × removal**

While chips visible, click ×. Chip disappears (after backend round-trip). After drain, removed item is NOT in the LLM response.

---

## Self-Review

Spec coverage:
1. **§7.1 IM worker → enqueue_or_send** → Task 4 ✓
2. **§7.3 ask_coordinator priority (unchanged)** → Task 4 keeps ask_coordinator call before enqueue ✓
3. **§6.1 N independent user messages** → Task 5 ✓
4. **§5.4 schedule_drain on StreamDone** → Task 2 ✓
5. **§10 queue full → webhook reply** → Task 4 ✓
6. **§11.2 architecture (no Tauri in runtime/pending)** → Task 7 ✓

Not covered in P3 (deferred):
- App composer integration → P4
- Multimodal cross-message budget → P5
- non-Anthropic pre-merge → P5
