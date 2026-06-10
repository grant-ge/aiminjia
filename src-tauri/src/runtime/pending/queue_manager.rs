//! PendingQueueManager — see spec §5.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::Result;
use async_trait::async_trait;
use tokio::task::JoinHandle;

use crate::auth::AuthDeactivationHandler;
use crate::runtime::chat::chat_turn_driver::{ChatAttachmentRef, IM_MOBILE_CHANNEL_CONTEXT};
use crate::runtime::chat::ChatTurnRequest;
use crate::runtime::event_bus::{RuntimeEventBus, RuntimeEventSubscriber};
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};
use crate::runtime::ids::SessionId;
use crate::runtime::run_registry::RuntimeRunRegistry;

use super::types::{EnqueueOutcome, EnqueueRejection, PendingConfig, PendingItem, PendingSource};

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
    direct_in_flight: bool,
}

impl SessionPending {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            drain_timer: None,
            recently_drained: VecDeque::new(),
            direct_in_flight: false,
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
        if self
            .run_registry
            .is_session_suspended_for_human(session_id.as_str())
        {
            return Ok(EnqueueOutcome::HeldForHumanInteraction {
                interaction_id: self
                    .run_registry
                    .suspended_interaction_id(session_id.as_str()),
            });
        }

        // 2. Acquire pending lock FIRST, then check busy/direct-in-flight inside lock.
        //
        // Lock ordering invariant (spec §5.3): pending mutex is acquired BEFORE
        // any query into run_registry. A SentDirectly decision also marks this
        // session as direct-in-flight until the turn-end drain hook or explicit
        // failure release clears it. This closes the race where two concurrent
        // idle enqueue calls both return SentDirectly before the downstream
        // gateway has reserved the session.
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

            if !busy && sp.items.is_empty() && !sp.direct_in_flight {
                sp.direct_in_flight = true;
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

    /// Release an idle-path direct dispatch marker when the caller failed before
    /// the normal turn-end drain hook could run. If messages queued behind this
    /// marker, schedule an immediate drain so they do not remain stuck.
    pub async fn release_direct_dispatch(&self, session_id: &SessionId) {
        let should_drain = {
            let mut guard = self.inner.lock().expect("pending mutex poisoned");
            let Some(sp) = guard.get_mut(session_id) else {
                return;
            };
            sp.direct_in_flight = false;
            !sp.items.is_empty()
        };
        if should_drain {
            self.schedule_drain_immediate(session_id.clone()).await;
        }
    }

    /// Remove the earliest queued item so a newly registered human interaction
    /// can interpret it as the user's reply. This covers the timing gap where an
    /// IM message arrives while the run is still producing the ask, before the
    /// ask has been registered in the interaction coordinator.
    pub async fn take_next_for_human_interaction(
        &self,
        session_id: &SessionId,
    ) -> Option<PendingItem> {
        let (item, snapshot) = {
            let mut guard = self.inner.lock().expect("pending mutex poisoned");
            let sp = guard.get_mut(session_id)?;
            if sp.items.is_empty() {
                return None;
            }
            let item = sp.items.remove(0);
            (item, sp.items.clone())
        };

        if let Some(dir) = self.resolver.conversation_dir(session_id) {
            let path = dir.join("pending.json");
            let items_for_write = snapshot.clone();
            tokio::task::spawn_blocking(move || {
                if let Err(e) = super::store::write_pending(&path, &items_for_write) {
                    log::warn!(
                        "[pending] write_pending after human-interaction take failed: {:#}",
                        e
                    );
                }
            });
        }

        let event = crate::runtime::events::RuntimeEvent::new(
            session_id.clone(),
            crate::runtime::ids::RunId::new("pending"),
            crate::runtime::events::RuntimeEventKind::PendingRemoved {
                item_id: item.id.clone(),
            },
        );
        if let Err(e) = self.event_bus.emit(event).await {
            log::warn!(
                "[pending] emit PendingRemoved after human-interaction take failed: {:#}",
                e
            );
        }

        Some(item)
    }

    /// Dispatch a queued item as a fresh turn after the human interaction router
    /// decided that the user changed topic instead of answering the ask.
    pub async fn dispatch_taken_human_interaction_item_as_new_turn(
        &self,
        session_id: &SessionId,
        item: PendingItem,
    ) -> Result<()> {
        let dispatcher = self.dispatcher.read().await.clone();
        let Some(dispatcher) = dispatcher else {
            anyhow::bail!("[pending] no dispatcher set for human-interaction new turn");
        };
        dispatcher
            .dispatch(build_request_from_single(session_id, item))
            .await
    }

    pub async fn set_dispatcher(&self, dispatcher: Arc<dyn ChatTurnDispatcher>) {
        *self.dispatcher.write().await = Some(dispatcher);
    }

    /// Start (or reset) the debounce timer for a session. Called after StreamDone
    /// and after busy-path enqueue.
    pub async fn schedule_drain(&self, session_id: SessionId) {
        self.schedule_drain_after(session_id, self.config.debounce_window)
            .await;
    }

    /// Start (or reset) the drain timer without the normal debounce. Used after
    /// an explicit user stop so queued follow-up messages can run as soon as the
    /// cancelled turn has actually released the busy slot.
    pub async fn schedule_drain_immediate(&self, session_id: SessionId) {
        self.schedule_drain_after(session_id, std::time::Duration::ZERO)
            .await;
    }

    async fn schedule_drain_after(&self, session_id: SessionId, delay: std::time::Duration) {
        let weak = self.self_arc.get().cloned().unwrap_or_default();

        let mut guard = self.inner.lock().expect("pending mutex poisoned");
        let Some(sp) = guard.get_mut(&session_id) else {
            return;
        };
        sp.direct_in_flight = false;
        if sp.items.is_empty() {
            return;
        }
        if let Some(old) = sp.drain_timer.take() {
            old.abort();
        }
        let sid_clone = session_id.clone();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(delay).await;
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
            sp.direct_in_flight = false;
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

        let dispatcher = self.dispatcher.read().await.clone();
        let Some(dispatcher) = dispatcher else {
            log::warn!("[pending] no dispatcher set; drained items dropped");
            return;
        };
        for batch in split_items_by_output_binding(items) {
            let request = build_request_from_batch(&session_id, batch);
            if let Err(e) = dispatcher.dispatch(request).await {
                log::error!(
                    "[pending] dispatcher failed for session {}: {:#}",
                    session_id.as_str(),
                    e
                );
            }
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
impl RuntimeEventSubscriber for PendingQueueManager {
    async fn on_event(&self, event: &RuntimeEvent) -> Result<()> {
        if matches!(
            event.kind,
            RuntimeEventKind::PermissionAskRequired { .. }
                | RuntimeEventKind::UserInteractionRequired { .. }
        ) {
            self.release_direct_dispatch(&event.session_id).await;
        }
        Ok(())
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
    req.channel_context = channel_context_for_pending_source(item.source);
    req.turn_origin = item.origin;
    req.output_binding = item.output_binding;
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
    req.channel_context = channel_context_for_pending_source(last.source);
    req.skill_command = last.skill_command.clone();
    req.turn_origin = last.origin.clone();
    req.output_binding = last.output_binding.clone();
    req.pending_batch = Some(items);
    req
}

fn split_items_by_output_binding(items: Vec<PendingItem>) -> Vec<Vec<PendingItem>> {
    let mut batches: Vec<Vec<PendingItem>> = Vec::new();
    for item in items {
        if let Some(last_batch) = batches.last_mut() {
            if last_batch
                .last()
                .map(|last| last.output_binding == item.output_binding)
                .unwrap_or(false)
            {
                last_batch.push(item);
                continue;
            }
        }
        batches.push(vec![item]);
    }
    batches
}

#[cfg(test)]
pub fn build_request_from_batch_for_test(
    session_id: &SessionId,
    items: Vec<PendingItem>,
) -> ChatTurnRequest {
    build_request_from_batch(session_id, items)
}

fn channel_context_for_pending_source(source: PendingSource) -> Option<String> {
    match source {
        PendingSource::ImDingtalk
        | PendingSource::ImFeishu
        | PendingSource::ImWecom
        | PendingSource::ImTelegram
        | PendingSource::ImWechat
        | PendingSource::ImWhatsapp => Some(IM_MOBILE_CHANNEL_CONTEXT.to_string()),
        PendingSource::App => None,
    }
}

fn file_name_of(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .map(String::from)
        .unwrap_or_else(|| path.to_string())
}
