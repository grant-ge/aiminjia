use std::sync::{Arc, Mutex};

use app_lib::runtime::{
    ChatTurnRequest, QueryEngine, RuntimeEventBus, RuntimeTurnExecutor, SessionRuntime,
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
