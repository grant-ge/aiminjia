mod common;

use std::sync::{Arc, Mutex};

use app_lib::runtime::{
    ChatTurnRequest, QueryEngine, RuntimeEventBus, RuntimeEventKind, RuntimeTurnExecutor,
    SessionRuntime,
};
use async_trait::async_trait;

#[derive(Default)]
struct RecordingExecutor {
    requests: Mutex<Vec<ChatTurnRequest>>,
}

#[async_trait]
impl RuntimeTurnExecutor for RecordingExecutor {
    async fn run_chat_turn(&self, request: ChatTurnRequest) -> Result<(), String> {
        self.requests.lock().unwrap().push(request);
        Ok(())
    }
}

#[tokio::test]
async fn review_executor_backed_chat_path_should_still_record_runtime_events() {
    let executor = Arc::new(RecordingExecutor::default());
    let runtime =
        SessionRuntime::with_executor(QueryEngine::new(), RuntimeEventBus::new(), executor);

    runtime
        .run_chat_request(ChatTurnRequest::new(
            "conv-production-path",
            "hello runtime",
            vec!["file-1".to_string()],
        ))
        .await
        .unwrap();

    let recorded = runtime.recorded_events();
    assert!(
        !recorded.is_empty(),
        "production chat path should still traverse SessionRuntime event flow instead of bypassing runtime entirely"
    );
}

#[tokio::test]
async fn review_executor_backed_chat_path_should_run_query_engine_turn_flow() {
    let executor = Arc::new(RecordingExecutor::default());
    let runtime =
        SessionRuntime::with_executor(QueryEngine::new(), RuntimeEventBus::new(), executor);

    runtime
        .run_chat_request(ChatTurnRequest::new(
            "conv-production-query",
            "hello runtime",
            vec!["file-1".to_string()],
        ))
        .await
        .unwrap();

    let recorded = runtime.recorded_events();
    let has_query_engine_flow = recorded.iter().any(|event| {
        matches!(event.kind, RuntimeEventKind::StreamStarted)
    });
    assert!(
        has_query_engine_flow,
        "executor-backed chat path should still traverse QueryEngine preflight before delegating to legacy executor"
    );
}

/// RED-LIGHT: runtime must own terminal orchestration — not delegate it entirely to legacy executor.
///
/// Currently `run_chat_request()` with an executor does:
///   1. emit RunStarted
///   2. run_turn() → QueryEngine emits StreamStarted then returns early
///   3. call executor.run_chat_turn(request)  ← full-turn owner is the executor
///
/// This means `MessagePersisted` and `StreamDone` are never emitted by the
/// SessionRuntime event bus — only by whatever the legacy executor does outside
/// the runtime boundary.
///
/// The correct architecture requires the runtime to retain turn orchestration:
/// MessagePersisted and StreamDone MUST appear in runtime.recorded_events().
/// This test will FAIL on current code to document the gap.
#[tokio::test]
async fn executor_backed_chat_path_should_not_delegate_full_turn_to_legacy_executor() {
    let executor = Arc::new(RecordingExecutor::default());
    let runtime =
        SessionRuntime::with_executor(QueryEngine::new(), RuntimeEventBus::new(), executor);

    runtime
        .run_chat_request(ChatTurnRequest::new(
            "conv-terminal-ownership",
            "hello runtime",
            vec![],
        ))
        .await
        .unwrap();

    let recorded = runtime.recorded_events();
    let event_labels = common::event_labels(&recorded);

    let has_message_persisted = recorded
        .iter()
        .any(|e| matches!(e.kind, RuntimeEventKind::MessagePersisted { .. }));
    assert!(
        has_message_persisted,
        "runtime must own MessagePersisted — it must not be delegated to legacy executor. \
         Observed runtime events: {:?}",
        event_labels
    );

    let has_stream_done = recorded
        .iter()
        .any(|e| matches!(e.kind, RuntimeEventKind::StreamDone));
    assert!(
        has_stream_done,
        "runtime must own StreamDone — it must not be delegated to legacy executor. \
         Observed runtime events: {:?}",
        event_labels
    );
}
