//! Integration test: app composer paths through PendingQueueManager.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::TempDir;

use app_lib::runtime::chat::ChatTurnRequest;
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::ids::{RunId, SessionId};
use app_lib::runtime::pending::{
    ChatTurnDispatcher, ConvDirResolver, EnqueueOutcome, EnqueueRejection, PendingAttachment,
    PendingConfig, PendingItem, PendingQueueManager, PendingSource,
};
use app_lib::runtime::run_registry::RuntimeRunRegistry;

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
        skill_command: None,
        reasoning_mode: None,
        received_at: "2026-05-11T03:21:00Z".into(),
        origin: Default::default(),
        output_binding: Default::default(),
    }
}

#[tokio::test]
async fn app_idle_returns_sent_directly() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver: Arc<dyn ConvDirResolver> = Arc::new(TempResolver(tmp.path().to_path_buf()));
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
    let resolver: Arc<dyn ConvDirResolver> = Arc::new(TempResolver(tmp.path().to_path_buf()));
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
    let resolver: Arc<dyn ConvDirResolver> = Arc::new(TempResolver(tmp.path().to_path_buf()));
    let mut config = PendingConfig::default();
    config.debounce_window = std::time::Duration::from_millis(50);
    let mgr = PendingQueueManager::new(registry.clone(), bus, resolver, config);
    let dispatcher = Arc::new(CountingDispatcher {
        count: AtomicUsize::new(0),
        last: tokio::sync::Mutex::new(None),
    });
    mgr.set_dispatcher(dispatcher.clone()).await;

    let session = SessionId::new("conv-app-drain");
    registry
        .reserve(session.as_str(), RunId::new("run-1"))
        .unwrap();
    mgr.enqueue_or_send(session.clone(), app_item("p1", "first", vec![]))
        .await
        .unwrap();
    mgr.enqueue_or_send(session.clone(), app_item("p2", "second", vec![]))
        .await
        .unwrap();

    registry.clear(session.as_str());
    mgr.schedule_drain(session.clone()).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    assert_eq!(dispatcher.count.load(Ordering::SeqCst), 1);
    let last = dispatcher.last.lock().await.clone().unwrap();
    // Spec §6.1: only the LAST drained item rides on request.content. Earlier
    // items are persisted by the dispatcher impl as independent user messages.
    assert!(
        last.content.contains("second"),
        "last item on request.content"
    );
    assert!(!last.content.contains("first"), "first item NOT in content");
    assert!(!last.content.contains("[以下是"), "no merge-header prefix");
    // App items have no sender prefix.
    assert!(!last.content.contains("["));
    // pending_batch carries all 2 items for the dispatcher impl.
    assert_eq!(last.pending_batch.as_ref().unwrap().len(), 2);
    assert_eq!(
        last.pending_batch.as_ref().unwrap()[0].source,
        PendingSource::App
    );
}

#[tokio::test]
async fn app_queue_full_rejects() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver: Arc<dyn ConvDirResolver> = Arc::new(TempResolver(tmp.path().to_path_buf()));
    let mut config = PendingConfig::default();
    config.max_queue_per_session = 2;
    let mgr = PendingQueueManager::new(registry.clone(), bus, resolver, config);

    let session = SessionId::new("conv-app-full");
    registry
        .reserve(session.as_str(), RunId::new("run-1"))
        .unwrap();
    mgr.enqueue_or_send(session.clone(), app_item("p1", "a", vec![]))
        .await
        .unwrap();
    mgr.enqueue_or_send(session.clone(), app_item("p2", "b", vec![]))
        .await
        .unwrap();
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
