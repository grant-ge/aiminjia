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
async fn production_session_runtime_routes_transport_requests_through_executor() {
    let executor = Arc::new(RecordingExecutor::default());
    let runtime =
        SessionRuntime::with_executor(QueryEngine::new(), RuntimeEventBus::new(), executor.clone());

    runtime
        .run_chat_request(ChatTurnRequest::new(
            "conv-transport",
            "hello runtime",
            vec!["file-1".to_string()],
        ))
        .await
        .unwrap();

    let requests = executor.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].conversation_id, "conv-transport");
    assert_eq!(requests[0].content, "hello runtime");
    assert_eq!(requests[0].file_ids, vec!["file-1".to_string()]);
}
