use std::sync::Arc;

use app_lib::runtime::{
    ChatTurnRequest, QueryEngine, RuntimeEventBus, RuntimeTurnExecutor, SessionRuntime,
};
use app_lib::transport::runtime_host::RuntimeHost;
use app_lib::transport::tauri_event_adapter::TauriEventAdapter;
use app_lib::transport::testing::RecordingRuntimeHost;
use async_trait::async_trait;
use serde_json::json;

struct LegacyLikeExecutor {
    host: Arc<dyn RuntimeHost>,
}

#[async_trait]
impl RuntimeTurnExecutor for LegacyLikeExecutor {
    async fn run_chat_turn(&self, request: ChatTurnRequest) -> Result<(), String> {
        self.host
            .emit_legacy_event(
                "streaming:delta",
                json!({
                    "conversationId": request.conversation_id,
                    "delta": format!("legacy:{}", request.content),
                }),
            )
            .map_err(|err| err.to_string())?;
        self.host
            .emit_legacy_event(
                "message:updated",
                json!({
                    "id": "legacy-msg-1",
                    "conversationId": request.conversation_id,
                    "role": "assistant",
                    "content": {"text": format!("legacy:{}", request.content)},
                }),
            )
            .map_err(|err| err.to_string())?;
        self.host
            .emit_legacy_event(
                "streaming:done",
                json!({
                    "conversationId": request.conversation_id,
                }),
            )
            .map_err(|err| err.to_string())?;
        Ok(())
    }
}

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
        vec!["streaming:delta", "message:updated", "streaming:done"],
        "executor-backed production path should emit one legacy chat sequence; runtime prelude + legacy executor currently duplicate the same events"
    );
}
