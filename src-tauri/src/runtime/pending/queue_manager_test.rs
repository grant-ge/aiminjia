use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::ids::SessionId;
use crate::runtime::pending::queue_manager::*;
use crate::runtime::pending::types::*;
use crate::runtime::run_registry::RuntimeRunRegistry;

/// Test resolver that maps SessionId → tmp-dir/{session_id}/
struct TempConvDirResolver(PathBuf);

impl ConvDirResolver for TempConvDirResolver {
    fn conversation_dir(&self, session_id: &SessionId) -> Option<PathBuf> {
        let dir = self.0.join(session_id.as_str());
        std::fs::create_dir_all(&dir).ok()?;
        Some(dir)
    }

    fn is_archived(&self, _session_id: &SessionId) -> bool {
        false
    }

    fn conversations_root(&self) -> PathBuf {
        self.0.clone()
    }
}

fn build_manager(tmp: &TempDir) -> (Arc<PendingQueueManager>, Arc<RuntimeRunRegistry>) {
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(TempConvDirResolver(tmp.path().to_path_buf()));
    let mgr = PendingQueueManager::new(registry.clone(), bus, resolver, PendingConfig::default());
    (mgr, registry)
}

fn sample_item(id: &str) -> PendingItem {
    PendingItem {
        id: id.into(),
        source: PendingSource::App,
        text: format!("text for {id}"),
        sender_nick: None,
        attachments: vec![],
        skill_command: None,
        received_at: "2026-05-11T03:21:00Z".into(),
    }
}

#[tokio::test]
async fn enqueue_idle_session_returns_sent_directly() {
    let tmp = TempDir::new().unwrap();
    let (mgr, _registry) = build_manager(&tmp);
    let session = SessionId::new("conv-1");
    let item = sample_item("pend-a");
    let outcome = mgr
        .enqueue_or_send(session.clone(), item.clone())
        .await
        .unwrap();
    match outcome {
        EnqueueOutcome::SentDirectly { request } => {
            assert_eq!(request.conversation_id.as_str(), "conv-1");
            assert_eq!(request.content, "text for pend-a");
        }
        other => panic!("expected SentDirectly, got {:?}", other),
    }
    assert!(mgr.snapshot(&session).await.is_empty());
}

#[tokio::test]
async fn enqueue_idle_but_queue_nonempty_still_queues() {
    let tmp = TempDir::new().unwrap();
    let (mgr, registry) = build_manager(&tmp);
    let session = SessionId::new("conv-mix");

    // Mark busy, enqueue an item
    use crate::runtime::ids::RunId;
    registry
        .reserve(session.as_str(), RunId::new("run-1"))
        .unwrap();
    let _ = mgr
        .enqueue_or_send(session.clone(), sample_item("first"))
        .await
        .unwrap();

    // Clear busy; queue still has "first"
    registry.clear(session.as_str());

    // Second enqueue should still be Queued (not SentDirectly), because
    // the queue is non-empty and must be drained first.
    let outcome = mgr
        .enqueue_or_send(session.clone(), sample_item("second"))
        .await
        .unwrap();
    match outcome {
        EnqueueOutcome::Queued { snapshot } => {
            assert_eq!(snapshot.len(), 2);
            assert_eq!(snapshot[0].id, "first");
            assert_eq!(snapshot[1].id, "second");
        }
        other => panic!("expected Queued, got {:?}", other),
    }
}

#[tokio::test]
async fn enqueue_busy_session_queues_and_persists() {
    let tmp = TempDir::new().unwrap();
    let (mgr, registry) = build_manager(&tmp);
    let session = SessionId::new("conv-busy");

    // Reserve a run to mark session busy.
    use crate::runtime::ids::RunId;
    registry
        .reserve(session.as_str(), RunId::new("run-1"))
        .unwrap();

    let item = sample_item("pend-1");
    let outcome = mgr
        .enqueue_or_send(session.clone(), item.clone())
        .await
        .unwrap();

    match outcome {
        EnqueueOutcome::Queued { snapshot } => {
            assert_eq!(snapshot.len(), 1);
            assert_eq!(snapshot[0].id, "pend-1");
        }
        other => panic!("expected Queued, got {:?}", other),
    }

    // Item still in memory
    let snap = mgr.snapshot(&session).await;
    assert_eq!(snap.len(), 1);

    // pending.json on disk has it (allow spawned write task time to land)
    let pending_path = tmp.path().join("conv-busy").join("pending.json");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(pending_path.exists(), "pending.json should exist");
    let content = std::fs::read_to_string(&pending_path).unwrap();
    assert!(content.contains("pend-1"));
}

#[tokio::test]
async fn enqueue_busy_full_queue_rejects() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(TempConvDirResolver(tmp.path().to_path_buf()));
    let mut config = PendingConfig::default();
    config.max_queue_per_session = 2;
    let mgr = PendingQueueManager::new(registry.clone(), bus, resolver, config);
    let session = SessionId::new("conv-full");
    use crate::runtime::ids::RunId;
    registry
        .reserve(session.as_str(), RunId::new("run-1"))
        .unwrap();

    mgr.enqueue_or_send(session.clone(), sample_item("a"))
        .await
        .unwrap();
    mgr.enqueue_or_send(session.clone(), sample_item("b"))
        .await
        .unwrap();
    let outcome = mgr
        .enqueue_or_send(session.clone(), sample_item("c"))
        .await
        .unwrap();

    match outcome {
        EnqueueOutcome::Rejected {
            reason: EnqueueRejection::QueueFull { limit: 2 },
        } => {}
        other => panic!("expected QueueFull(2), got {:?}", other),
    }
}

struct ArchivedResolver(PathBuf);
impl ConvDirResolver for ArchivedResolver {
    fn conversation_dir(&self, sid: &SessionId) -> Option<PathBuf> {
        let d = self.0.join(sid.as_str());
        std::fs::create_dir_all(&d).ok()?;
        Some(d)
    }
    fn is_archived(&self, _sid: &SessionId) -> bool {
        true
    }
    fn conversations_root(&self) -> PathBuf {
        self.0.clone()
    }
}

#[tokio::test]
async fn enqueue_archived_session_rejects() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(ArchivedResolver(tmp.path().to_path_buf()));
    let mgr = PendingQueueManager::new(registry, bus, resolver, PendingConfig::default());
    let session = SessionId::new("conv-archived");
    let outcome = mgr
        .enqueue_or_send(session, sample_item("x"))
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        EnqueueOutcome::Rejected {
            reason: EnqueueRejection::SessionArchived
        }
    ));
}

use std::sync::atomic::{AtomicUsize, Ordering};

struct CountingDispatcher {
    pub count: AtomicUsize,
    pub last_text: tokio::sync::Mutex<Option<String>>,
    pub last_skill_id: tokio::sync::Mutex<Option<String>>,
}

#[async_trait::async_trait]
impl ChatTurnDispatcher for CountingDispatcher {
    async fn dispatch(&self, request: crate::runtime::chat::ChatTurnRequest) -> anyhow::Result<()> {
        self.count.fetch_add(1, Ordering::SeqCst);
        *self.last_skill_id.lock().await =
            request.skill_command.as_ref().map(|skill| skill.id.clone());
        *self.last_text.lock().await = Some(request.content);
        Ok(())
    }
}

#[tokio::test]
async fn drain_dispatches_after_debounce() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(TempConvDirResolver(tmp.path().to_path_buf()));
    let mut config = PendingConfig::default();
    config.debounce_window = std::time::Duration::from_millis(50);
    let dispatcher = Arc::new(CountingDispatcher {
        count: AtomicUsize::new(0),
        last_text: tokio::sync::Mutex::new(None),
        last_skill_id: tokio::sync::Mutex::new(None),
    });
    let mgr = PendingQueueManager::new(registry.clone(), bus, resolver, config);
    mgr.set_dispatcher(dispatcher.clone()).await;

    let session = SessionId::new("conv-drain");
    use crate::runtime::ids::RunId;
    registry
        .reserve(session.as_str(), RunId::new("run-1"))
        .unwrap();
    mgr.enqueue_or_send(session.clone(), sample_item("a"))
        .await
        .unwrap();
    mgr.enqueue_or_send(session.clone(), sample_item("b"))
        .await
        .unwrap();

    // Release busy + schedule_drain
    registry.clear(session.as_str());
    mgr.schedule_drain(session.clone()).await;

    // Wait > debounce
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    assert_eq!(
        dispatcher.count.load(Ordering::SeqCst),
        1,
        "dispatched once"
    );
    let text = dispatcher.last_text.lock().await.clone().unwrap();
    // Spec §6.1: drained batch lands as N independent user messages, the last
    // of which rides on the ChatTurnRequest (its content == last item's text).
    // Earlier items are persisted by the dispatcher impl before the LLM call.
    assert!(
        text.contains("text for b"),
        "request.content carries last item"
    );
    assert!(
        !text.contains("text for a"),
        "earlier items don't appear in content"
    );
    assert!(!text.contains("[以下是"), "no merge-header prefix");

    // Queue empty after drain
    assert!(mgr.snapshot(&session).await.is_empty());

    // pending.json empty
    let path = tmp.path().join("conv-drain").join("pending.json");
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("\"items\": []"));
}

#[tokio::test]
async fn drain_preserves_skill_command_on_dispatched_request() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(TempConvDirResolver(tmp.path().to_path_buf()));
    let mut config = PendingConfig::default();
    config.debounce_window = std::time::Duration::from_millis(50);
    let dispatcher = Arc::new(CountingDispatcher {
        count: AtomicUsize::new(0),
        last_text: tokio::sync::Mutex::new(None),
        last_skill_id: tokio::sync::Mutex::new(None),
    });
    let mgr = PendingQueueManager::new(registry.clone(), bus, resolver, config);
    mgr.set_dispatcher(dispatcher.clone()).await;

    let session = SessionId::new("conv-skill-drain");
    use crate::runtime::chat::chat_turn_driver::SkillCommandRef;
    use crate::runtime::ids::RunId;
    registry
        .reserve(session.as_str(), RunId::new("run-1"))
        .unwrap();
    let mut item = sample_item("skill");
    item.skill_command = Some(SkillCommandRef {
        id: "dingtalk-workspace".into(),
        label: Some("玩转钉钉".into()),
        command: Some("/dingtalk-workspace".into()),
    });
    mgr.enqueue_or_send(session.clone(), item).await.unwrap();

    registry.clear(session.as_str());
    mgr.schedule_drain(session.clone()).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    assert_eq!(dispatcher.count.load(Ordering::SeqCst), 1);
    assert_eq!(
        dispatcher.last_skill_id.lock().await.as_deref(),
        Some("dingtalk-workspace")
    );
}

#[tokio::test]
async fn drain_skipped_when_session_busy() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(TempConvDirResolver(tmp.path().to_path_buf()));
    let mut config = PendingConfig::default();
    config.debounce_window = std::time::Duration::from_millis(50);
    let dispatcher = Arc::new(CountingDispatcher {
        count: AtomicUsize::new(0),
        last_text: tokio::sync::Mutex::new(None),
        last_skill_id: tokio::sync::Mutex::new(None),
    });
    let mgr = PendingQueueManager::new(registry.clone(), bus, resolver, config);
    mgr.set_dispatcher(dispatcher.clone()).await;

    let session = SessionId::new("conv-busy-drain");
    use crate::runtime::ids::RunId;
    registry
        .reserve(session.as_str(), RunId::new("run-1"))
        .unwrap();
    mgr.enqueue_or_send(session.clone(), sample_item("a"))
        .await
        .unwrap();

    // Don't clear busy. schedule_drain anyway.
    mgr.schedule_drain(session.clone()).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    assert_eq!(dispatcher.count.load(Ordering::SeqCst), 0);
    assert_eq!(mgr.snapshot(&session).await.len(), 1, "still queued");
}

#[tokio::test]
async fn remove_item_removes_from_memory_disk_and_emits_event() {
    let tmp = TempDir::new().unwrap();
    let (mgr, registry) = build_manager(&tmp);
    let session = SessionId::new("conv-remove");
    use crate::runtime::ids::RunId;
    registry
        .reserve(session.as_str(), RunId::new("run-1"))
        .unwrap();

    mgr.enqueue_or_send(session.clone(), sample_item("keep"))
        .await
        .unwrap();
    mgr.enqueue_or_send(session.clone(), sample_item("drop"))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let removed = mgr.remove_item(&session, "drop").await.unwrap();
    assert!(removed);

    let snap = mgr.snapshot(&session).await;
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].id, "keep");

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let path = tmp.path().join("conv-remove").join("pending.json");
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(!content.contains("\"id\": \"drop\""));
    assert!(content.contains("\"id\": \"keep\""));
}

#[tokio::test]
async fn remove_item_missing_returns_false() {
    let tmp = TempDir::new().unwrap();
    let (mgr, _) = build_manager(&tmp);
    let session = SessionId::new("conv-missing");
    let removed = mgr.remove_item(&session, "nope").await.unwrap();
    assert!(!removed);
}

#[tokio::test]
async fn restore_from_disk_loads_pending_items() {
    let tmp = TempDir::new().unwrap();
    let conv_dir = tmp.path().join("conv-restore");
    std::fs::create_dir_all(&conv_dir).unwrap();
    let path = conv_dir.join("pending.json");
    crate::runtime::pending::store::write_pending(
        &path,
        &[sample_item("loaded-1"), sample_item("loaded-2")],
    )
    .unwrap();

    let (mgr, _) = build_manager(&tmp);
    mgr.restore_from_disk().await.unwrap();

    let session = SessionId::new("conv-restore");
    let snap = mgr.snapshot(&session).await;
    assert_eq!(snap.len(), 2);
}

#[tokio::test]
async fn drain_recently_drained_blocks_replay_after_restore() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(TempConvDirResolver(tmp.path().to_path_buf()));
    let mut config = PendingConfig::default();
    config.debounce_window = std::time::Duration::from_millis(30);
    let dispatcher = Arc::new(CountingDispatcher {
        count: AtomicUsize::new(0),
        last_text: tokio::sync::Mutex::new(None),
        last_skill_id: tokio::sync::Mutex::new(None),
    });
    let mgr = PendingQueueManager::new(registry.clone(), bus, resolver, config);
    mgr.set_dispatcher(dispatcher.clone()).await;
    let session = SessionId::new("conv-recent");
    use crate::runtime::ids::RunId;
    registry
        .reserve(session.as_str(), RunId::new("run-1"))
        .unwrap();
    mgr.enqueue_or_send(session.clone(), sample_item("once"))
        .await
        .unwrap();
    registry.clear(session.as_str());
    mgr.schedule_drain(session.clone()).await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(dispatcher.count.load(Ordering::SeqCst), 1);

    // Simulate disk still has the item (drain wrote empty but pretend crash)
    let conv_dir = tmp.path().join("conv-recent");
    crate::runtime::pending::store::write_pending(
        &conv_dir.join("pending.json"),
        &[sample_item("once")],
    )
    .unwrap();

    mgr.restore_from_disk().await.unwrap();
    let snap = mgr.snapshot(&session).await;
    assert!(snap.is_empty(), "recently_drained should suppress replay");
}
