//! PendingQueueManager — see spec §5.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::Result;
use async_trait::async_trait;
use tokio::task::JoinHandle;

use crate::auth::AuthDeactivationHandler;
use crate::runtime::chat::chat_turn_driver::ChatAttachmentRef;
use crate::runtime::chat::ChatTurnRequest;
use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::ids::SessionId;
use crate::runtime::run_registry::RuntimeRunRegistry;

use super::types::{EnqueueOutcome, EnqueueRejection, PendingConfig, PendingItem};

/// Per-host abstraction over conversation directory layout.
pub trait ConvDirResolver: Send + Sync {
    fn conversation_dir(&self, session_id: &SessionId) -> Option<PathBuf>;
    fn is_archived(&self, session_id: &SessionId) -> bool;
    fn conversations_root(&self) -> PathBuf;
}

/// Abstraction over "send a ChatTurnRequest" — production wires to
/// TauriChatCommandAdapter::send_chat_request (P3), tests use a fake.
#[async_trait]
pub trait ChatTurnDispatcher: Send + Sync {
    async fn dispatch(&self, request: ChatTurnRequest) -> Result<()>;
}

#[allow(dead_code)]
struct SessionPending {
    items: Vec<PendingItem>,
    drain_timer: Option<JoinHandle<()>>,
    recently_drained: VecDeque<(String, Instant)>,
}

impl SessionPending {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            drain_timer: None,
            recently_drained: VecDeque::new(),
        }
    }
}

#[allow(dead_code)]
pub struct PendingQueueManager {
    inner: Mutex<HashMap<SessionId, SessionPending>>,
    run_registry: Arc<RuntimeRunRegistry>,
    event_bus: Arc<RuntimeEventBus>,
    resolver: Arc<dyn ConvDirResolver>,
    config: PendingConfig,
    dispatcher: tokio::sync::RwLock<Option<Arc<dyn ChatTurnDispatcher>>>,
    self_arc: std::sync::OnceLock<std::sync::Weak<Self>>,
}

impl PendingQueueManager {
    pub fn new(
        run_registry: Arc<RuntimeRunRegistry>,
        event_bus: Arc<RuntimeEventBus>,
        resolver: Arc<dyn ConvDirResolver>,
        config: PendingConfig,
    ) -> Arc<Self> {
        let mgr = Arc::new(Self {
            inner: Mutex::new(HashMap::new()),
            run_registry,
            event_bus,
            resolver,
            config,
            dispatcher: tokio::sync::RwLock::new(None),
            self_arc: std::sync::OnceLock::new(),
        });
        let _ = mgr.self_arc.set(Arc::downgrade(&mgr));
        mgr
    }

    /// Snapshot the current pending items for a session (UI / test helper).
    pub async fn snapshot(&self, session_id: &SessionId) -> Vec<PendingItem> {
        let guard = self.inner.lock().expect("pending mutex poisoned");
        guard
            .get(session_id)
            .map(|sp| sp.items.clone())
            .unwrap_or_default()
    }

    pub fn clear_all(&self) {
        let mut guard = self.inner.lock().expect("pending mutex poisoned");
        for sp in guard.values_mut() {
            if let Some(handle) = sp.drain_timer.take() {
                handle.abort();
            }
        }
        guard.clear();
    }

    /// Enqueue the item if the session is busy; otherwise return a ChatTurnRequest
    /// for the caller to dispatch immediately.
    pub async fn enqueue_or_send(
        &self,
        session_id: SessionId,
        item: PendingItem,
    ) -> Result<EnqueueOutcome> {
        // 1. Archive check (lock-free — read-only resolver call)
        if self.resolver.is_archived(&session_id) {
            return Ok(EnqueueOutcome::Rejected {
                reason: EnqueueRejection::SessionArchived,
            });
        }

        // 2. Acquire pending lock FIRST, then check busy inside lock.
        //
        // Lock ordering invariant (spec §5.3): pending mutex is acquired BEFORE
        // any query into run_registry. This serializes the "send or queue" decision
        // and prevents races where two concurrent enqueue calls both think the
        // session is idle and both return SentDirectly, leading to one caller's
        // run_registry.reserve() failing and the message being silently dropped.
        //
        // Important: the std::sync::MutexGuard is NOT Send, so it must NEVER cross
        // an `.await`. We use a scoped block + enum to extract the decision before
        // any await happens.
        enum Decision {
            SendDirectly,
            Queue { snapshot: Vec<PendingItem> },
            QueueFull,
        }
        let decision = {
            let mut guard = self.inner.lock().expect("pending mutex poisoned");
            let sp = guard
                .entry(session_id.clone())
                .or_insert_with(SessionPending::new);

            let busy = self.run_registry.is_session_busy(session_id.as_str());

            if !busy && sp.items.is_empty() {
                Decision::SendDirectly
            } else if sp.items.len() >= self.config.max_queue_per_session {
                Decision::QueueFull
            } else {
                sp.items.push(item.clone());
                Decision::Queue {
                    snapshot: sp.items.clone(),
                }
            }
            // guard drops here
        };

        match decision {
            Decision::SendDirectly => {
                let request = build_request_from_single(&session_id, item);
                return Ok(EnqueueOutcome::SentDirectly { request });
            }
            Decision::QueueFull => {
                return Ok(EnqueueOutcome::Rejected {
                    reason: EnqueueRejection::QueueFull {
                        limit: self.config.max_queue_per_session,
                    },
                });
            }
            Decision::Queue { snapshot } => {
                // Persist asynchronously — never block the enqueue path on disk IO.
                if let Some(dir) = self.resolver.conversation_dir(&session_id) {
                    let path = dir.join("pending.json");
                    let items_for_write = snapshot.clone();
                    tokio::task::spawn_blocking(move || {
                        if let Err(e) = super::store::write_pending(&path, &items_for_write) {
                            log::warn!("[pending] write_pending failed: {:#}", e);
                        }
                    });
                }

                // Emit event so UI can render a chip.
                let event = crate::runtime::events::RuntimeEvent::new(
                    session_id.clone(),
                    crate::runtime::ids::RunId::new("pending"),
                    crate::runtime::events::RuntimeEventKind::PendingQueued { item },
                );
                if let Err(e) = self.event_bus.emit(event).await {
                    log::warn!("[pending] emit PendingQueued failed: {:#}", e);
                }

                Ok(EnqueueOutcome::Queued { snapshot })
            }
        }
    }

    pub async fn set_dispatcher(&self, dispatcher: Arc<dyn ChatTurnDispatcher>) {
        *self.dispatcher.write().await = Some(dispatcher);
    }

    /// Start (or reset) the debounce timer for a session. Called after StreamDone
    /// and after busy-path enqueue.
    pub async fn schedule_drain(&self, session_id: SessionId) {
        let debounce = self.config.debounce_window;
        let weak = self.self_arc.get().cloned().unwrap_or_default();

        let mut guard = self.inner.lock().expect("pending mutex poisoned");
        let Some(sp) = guard.get_mut(&session_id) else {
            return;
        };
        if sp.items.is_empty() {
            return;
        }
        if let Some(old) = sp.drain_timer.take() {
            old.abort();
        }
        let sid_clone = session_id.clone();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(debounce).await;
            if let Some(mgr) = weak.upgrade() {
                mgr.drain_and_dispatch(sid_clone).await;
            }
        });
        sp.drain_timer = Some(handle);
    }

    async fn drain_and_dispatch(&self, session_id: SessionId) {
        // 1. Take items (with re-check is_busy under the same lock).
        let items_opt: Option<Vec<PendingItem>> = {
            let mut guard = self.inner.lock().expect("pending mutex poisoned");
            let Some(sp) = guard.get_mut(&session_id) else {
                return;
            };
            if self.run_registry.is_session_busy(session_id.as_str()) {
                log::info!(
                    "[pending] drain skipped — session {} still busy",
                    session_id.as_str()
                );
                return;
            }
            if sp.items.is_empty() {
                return;
            }
            let taken = std::mem::take(&mut sp.items);
            sp.drain_timer = None;
            let now = Instant::now();
            for it in &taken {
                sp.recently_drained.push_back((it.id.clone(), now));
            }
            Self::trim_recently_drained(sp, self.config.recently_drained_ttl);
            Some(taken)
        };

        let Some(items) = items_opt else { return };
        let drained_ids: Vec<String> = items.iter().map(|i| i.id.clone()).collect();

        // 2. Persist empty file
        if let Some(dir) = self.resolver.conversation_dir(&session_id) {
            let path = dir.join("pending.json");
            let _ = tokio::task::spawn_blocking(move || super::store::write_pending(&path, &[]))
                .await
                .map(|res| {
                    if let Err(e) = res {
                        log::warn!("[pending] clearing pending.json failed: {:#}", e);
                    }
                });
        }

        // 3. Emit drained event
        let event = crate::runtime::events::RuntimeEvent::new(
            session_id.clone(),
            crate::runtime::ids::RunId::new("pending"),
            crate::runtime::events::RuntimeEventKind::PendingDrained {
                drained_ids: drained_ids.clone(),
            },
        );
        if let Err(e) = self.event_bus.emit(event).await {
            log::warn!("[pending] emit PendingDrained failed: {:#}", e);
        }

        // 4. Build merged request and dispatch
        let request = build_request_from_batch(&session_id, items);
        let dispatcher = self.dispatcher.read().await.clone();
        let Some(dispatcher) = dispatcher else {
            log::warn!("[pending] no dispatcher set; drained items dropped");
            return;
        };
        if let Err(e) = dispatcher.dispatch(request).await {
            log::error!(
                "[pending] dispatcher failed for session {}: {:#}",
                session_id.as_str(),
                e
            );
        }
    }

    /// Remove one item from the queue (UI × button).
    pub async fn remove_item(&self, session_id: &SessionId, item_id: &str) -> Result<bool> {
        let (removed, snapshot_opt) = {
            let mut guard = self.inner.lock().expect("pending mutex poisoned");
            let Some(sp) = guard.get_mut(session_id) else {
                return Ok(false);
            };
            let before = sp.items.len();
            sp.items.retain(|i| i.id != item_id);
            let removed = sp.items.len() < before;
            (removed, removed.then(|| sp.items.clone()))
        };
        if !removed {
            return Ok(false);
        }

        // Persist new state
        if let (Some(snap), Some(dir)) = (snapshot_opt, self.resolver.conversation_dir(session_id))
        {
            let path = dir.join("pending.json");
            tokio::task::spawn_blocking(move || {
                if let Err(e) = super::store::write_pending(&path, &snap) {
                    log::warn!("[pending] write_pending on remove failed: {:#}", e);
                }
            });
        }

        let event = crate::runtime::events::RuntimeEvent::new(
            session_id.clone(),
            crate::runtime::ids::RunId::new("pending"),
            crate::runtime::events::RuntimeEventKind::PendingRemoved {
                item_id: item_id.to_string(),
            },
        );
        if let Err(e) = self.event_bus.emit(event).await {
            log::warn!("[pending] emit PendingRemoved failed: {:#}", e);
        }
        Ok(true)
    }

    /// Load pending.json from all conversations into memory. Items previously
    /// drained (within TTL) are filtered.
    pub async fn restore_from_disk(&self) -> Result<()> {
        let root = self.resolver.conversations_root();
        let scanned =
            tokio::task::spawn_blocking(move || super::store::scan_conversation_pending(&root))
                .await
                .map_err(|e| anyhow::anyhow!("join: {e}"))??;

        let mut guard = self.inner.lock().expect("pending mutex poisoned");
        for (conv_id, items) in scanned {
            let session_id = SessionId::new(conv_id);
            if self.resolver.is_archived(&session_id) {
                continue;
            }
            let sp = guard.entry(session_id).or_insert_with(SessionPending::new);
            for item in items {
                // Skip if recently drained
                let drained = sp.recently_drained.iter().any(|(id, _)| id == &item.id);
                if !drained {
                    sp.items.push(item);
                }
            }
        }
        Ok(())
    }

    fn trim_recently_drained(sp: &mut SessionPending, ttl: std::time::Duration) {
        let cutoff = Instant::now().checked_sub(ttl);
        if let Some(cutoff) = cutoff {
            while sp
                .recently_drained
                .front()
                .map(|(_, t)| *t < cutoff)
                .unwrap_or(false)
            {
                sp.recently_drained.pop_front();
            }
        }
    }
}

#[async_trait]
impl AuthDeactivationHandler for PendingQueueManager {
    async fn on_deactivated(&self) {
        self.clear_all();
    }
}

fn build_request_from_single(session_id: &SessionId, item: PendingItem) -> ChatTurnRequest {
    let attachments: Vec<ChatAttachmentRef> = item
        .attachments
        .iter()
        .map(|a| ChatAttachmentRef {
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
        })
        .collect();
    let mut req = ChatTurnRequest::new(session_id.clone(), item.text, attachments);
    req.skill_command = item.skill_command;
    req
}

/// Build a ChatTurnRequest from a drained batch.
///
/// Strategy (spec §6.1 + §6.2):落 N 条独立 user message + LLM 看到 N 条独立 user.
/// We only construct ONE ChatTurnRequest (the last item triggers the LLM turn).
/// The dispatcher impl is responsible for persisting the first N-1 items as
/// standalone user messages BEFORE calling `send_chat_request` on this request.
/// `request.pending_batch` carries the full Vec for the dispatcher to read.
fn build_request_from_batch(session_id: &SessionId, items: Vec<PendingItem>) -> ChatTurnRequest {
    debug_assert!(
        !items.is_empty(),
        "drain should never invoke with empty items"
    );
    let last = items.last().expect("non-empty");
    let last_text = match &last.sender_nick {
        Some(nick) if !nick.is_empty() => format!("[{}]: {}", nick, last.text),
        _ => last.text.clone(),
    };
    let last_attachments: Vec<ChatAttachmentRef> = last
        .attachments
        .iter()
        .map(|a| ChatAttachmentRef {
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
        })
        .collect();
    let mut req = ChatTurnRequest::new(session_id.clone(), last_text, last_attachments);
    req.skill_command = last.skill_command.clone();
    req.pending_batch = Some(items);
    req
}

fn file_name_of(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .map(String::from)
        .unwrap_or_else(|| path.to_string())
}
