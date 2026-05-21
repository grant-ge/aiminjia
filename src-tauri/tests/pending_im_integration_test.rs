//! Integration test: enqueueing while busy, then drain after busy clears.
//!
//! Uses fake ChatTurnDispatcher to verify the merged ChatTurnRequest reaches
//! the dispatcher exactly once with the expected merged content.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::TempDir;

use app_lib::runtime::chat::ChatTurnRequest;
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::ids::{RunId, SessionId};
use app_lib::runtime::pending::{
    ChatTurnDispatcher, ConvDirResolver, EnqueueOutcome, PendingConfig, PendingItem,
    PendingQueueManager, PendingSource,
};
use app_lib::runtime::run_registry::RuntimeRunRegistry;

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
        skill_command: None,
        received_at: "2026-05-11T03:21:00Z".into(),
    }
}

#[tokio::test]
async fn three_im_messages_merge_into_one_dispatch() {
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
    // Spec §6.1: request.content carries the LAST drained item's text only.
    // Earlier items are persisted by the dispatcher impl before send_chat_request.
    let body = &last.content;
    assert!(
        body.contains("就是 Q1"),
        "last item's text rides on request"
    );
    assert!(
        body.contains("[张三]:"),
        "sender prefix preserved on last item"
    );
    assert!(!body.contains("帮我看下"), "earlier items NOT in content");
    assert!(
        !body.contains("顺便看下这个"),
        "earlier items NOT in content"
    );
    assert!(!body.contains("[以下是"), "no merge-header prefix");
    // pending_batch carried through with all 3 items for the dispatcher impl.
    assert!(last.pending_batch.is_some());
    assert_eq!(last.pending_batch.as_ref().unwrap().len(), 3);
}

#[tokio::test]
async fn idle_session_returns_sent_directly_without_queue() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver: Arc<dyn ConvDirResolver> = Arc::new(TempResolver(tmp.path().to_path_buf()));
    let mgr = PendingQueueManager::new(registry, bus, resolver, PendingConfig::default());
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
