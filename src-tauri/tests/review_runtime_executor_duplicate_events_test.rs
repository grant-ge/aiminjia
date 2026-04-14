use std::sync::Arc;

use app_lib::runtime::{
    ChatTurnRequest, QueryEngine, RuntimeEventBus, RuntimeTurnExecutor, SessionRuntime,
};
use app_lib::transport::runtime_host::RuntimeHost;
use app_lib::transport::tauri_event_adapter::TauriEventAdapter;
use app_lib::transport::testing::RecordingRuntimeHost;
use async_trait::async_trait;
use serde_json::json;

/// A legacy-like executor that only emits streaming:delta (mid-turn content) to
/// the host directly.  After P1-B, MessagePersisted / StreamDone / AgentIdle are
/// emitted by `RuntimeChatTurnDriver` via the bus → `TauriEventAdapter`, so the
/// executor must NOT also emit them (that would produce duplicates).
struct LegacyLikeExecutor {
    host: Arc<dyn RuntimeHost>,
}

#[async_trait]
impl RuntimeTurnExecutor for LegacyLikeExecutor {
    async fn run_chat_turn(&self, request: ChatTurnRequest) -> Result<(), String> {
        // Executor may still emit mid-turn content (streaming:delta) directly.
        self.host
            .emit_legacy_event(
                "streaming:delta",
                json!({
                    "conversationId": request.conversation_id,
                    "delta": format!("legacy:{}", request.content),
                }),
            )
            .map_err(|err| err.to_string())?;
        // NOTE: message:updated and streaming:done are NO LONGER emitted here.
        // After P1-B they are emitted by RuntimeChatTurnDriver via the bus so
        // that TauriEventAdapter delivers exactly one copy of each to the host.
        Ok(())
    }
}

/// P1-B regression gate: the executor-backed production path must deliver exactly
/// one copy of each terminal event to the host.
///
/// Before P1-B, `RuntimeChatTurnDriver` called `record_only` (no subscriber
/// notification), so terminal events had to come from the executor directly.
///
/// After P1-B, `RuntimeChatTurnDriver` emits `MessagePersisted`, `StreamDone`,
/// and `AgentIdle` through the bus.  `TauriEventAdapter` maps them to
/// `message:updated`, `streaming:done`, and `agent:idle`.  The executor must NOT
/// also emit the same events (which would duplicate them); only mid-turn deltas
/// may be emitted directly by the executor.
///
/// Expected host event sequence:
///   streaming:delta   ← emitted directly by executor (mid-turn content)
///   message:updated   ← emitted by driver via bus → TauriEventAdapter
///   streaming:done    ← emitted by driver via bus → TauriEventAdapter
///   agent:idle        ← emitted by driver via bus → TauriEventAdapter
#[tokio::test]
async fn review_executor_backed_runtime_should_not_double_emit_legacy_chat_events() {
    let host = RecordingRuntimeHost::new();
    let bus = RuntimeEventBus::new();
    bus.subscribe(Arc::new(TauriEventAdapter::new(host.clone())));

    let runtime = SessionRuntime::with_executor(
        QueryEngine::new(),
        bus,
        Arc::new(LegacyLikeExecutor {
            host: host.clone(),
        }),
    );

    runtime
        .run_chat_request(ChatTurnRequest::new(
            "conv-dup",
            "hello runtime",
            Vec::new(),
        ))
        .await
        .unwrap();

    assert_eq!(
        host.trace().event_names(),
        vec![
            "streaming:delta",    // executor direct emit (mid-turn content)
            "message:updated",    // driver bus emit via TauriEventAdapter
            "streaming:done",     // driver bus emit via TauriEventAdapter
            "agent:idle",         // driver bus emit via TauriEventAdapter
        ],
        "executor-backed production path must deliver exactly one copy of each terminal \
         event: streaming:delta from executor, then message:updated + streaming:done + \
         agent:idle from RuntimeChatTurnDriver via bus. Got: {:?}",
        host.trace().event_names()
    );
}

