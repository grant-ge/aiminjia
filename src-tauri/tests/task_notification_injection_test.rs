use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use app_lib::runtime::agent::task_notification::TaskNotificationQueue;
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::turn_config::{
    LlmStepInput, LlmStepResult, TurnError,
};
use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use app_lib::runtime::chat::{
    ChatTurnRequest, RuntimeChatTurnDriver, RuntimeLlmExecutor,
};
use app_lib::runtime::chat::chat_turn_driver::ChatAttachmentRef;
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::{RunId, SessionId};
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;

#[derive(Default)]
struct RecordingExecutor {
    seen_messages: Mutex<Vec<Vec<Value>>>,
}

impl RecordingExecutor {
    fn captured_messages(&self) -> Vec<Vec<Value>> {
        self.seen_messages.lock().unwrap().clone()
    }
}

struct IterationDrainExecutor {
    seen_messages: Mutex<Vec<Vec<Value>>>,
    queue: Arc<TaskNotificationQueue>,
    xml: String,
    session_id: SessionId,
}

impl IterationDrainExecutor {
    fn new(
        queue: Arc<TaskNotificationQueue>,
        xml: impl Into<String>,
        session_id: SessionId,
    ) -> Self {
        Self {
            seen_messages: Mutex::new(Vec::new()),
            queue,
            xml: xml.into(),
            session_id,
        }
    }

    fn captured_messages(&self) -> Vec<Vec<Value>> {
        self.seen_messages.lock().unwrap().clone()
    }
}

#[async_trait]
impl RuntimeLlmExecutor for RecordingExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        self.seen_messages.lock().unwrap().push(input.messages.clone());
        Ok(LlmStepResult::ContentComplete {
            content: "done".to_string(),
            tokens_in: 0,
            tokens_out: 0,
            stop_reason: Some("end_turn".to_string()),
        })
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _tool_calls: &[Value],
        _generated_file_ids: &[String],
        _file_metas: &[Value],
    ) -> Result<String, TurnError> {
        Ok("assistant-msg".to_string())
    }

    async fn persist_user_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _attachments: &[ChatAttachmentRef],
        _client_message_id: Option<&str>,
    ) -> Result<String, TurnError> {
        Ok("user-msg".to_string())
    }

    async fn load_workspace_path(&self) -> Result<PathBuf, TurnError> {
        Ok(std::env::temp_dir())
    }
}

#[async_trait]
impl RuntimeLlmExecutor for IterationDrainExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        let mut seen = self.seen_messages.lock().unwrap();
        seen.push(input.messages.clone());
        let call_index = seen.len();
        drop(seen);

        if call_index == 1 {
            self.queue.enqueue(
                "agent-between-iterations",
                self.xml.clone(),
                self.session_id.clone(),
                None,
            );
            return Ok(LlmStepResult::ToolCalls {
                assistant_content: "checking".to_string(),
                tool_calls: vec![RuntimeToolCallRequest {
                    tool_call_id: "tool-empty".to_string(),
                    tool_name: "noop".to_string(),
                    args: serde_json::json!({}),
                    purpose: None,
                }],
                tokens_in: 0,
                tokens_out: 0,
            });
        }

        Ok(LlmStepResult::ContentComplete {
            content: "done".to_string(),
            tokens_in: 0,
            tokens_out: 0,
            stop_reason: Some("end_turn".to_string()),
        })
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _tool_calls: &[Value],
        _generated_file_ids: &[String],
        _file_metas: &[Value],
    ) -> Result<String, TurnError> {
        Ok("assistant-msg".to_string())
    }

    async fn persist_user_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _attachments: &[ChatAttachmentRef],
        _client_message_id: Option<&str>,
    ) -> Result<String, TurnError> {
        Ok("user-msg".to_string())
    }

    async fn persist_iteration_assistant_message(
        &self,
        _conversation_id: &str,
        _tool_calls: &[Value],
    ) -> Result<Option<String>, TurnError> {
        Ok(Some("iteration-assistant-msg".to_string()))
    }

    async fn load_workspace_path(&self) -> Result<PathBuf, TurnError> {
        Ok(std::env::temp_dir())
    }
}

fn make_turn_and_request() -> (TurnState, ChatTurnRequest) {
    let conversation_id = "task-notification-injection";
    let run_id = RunId::new("run-task-notification-injection");
    let identity = IdentityMapping::from_legacy_conversation_id(conversation_id.to_string());
    let turn = TurnState::new(identity, run_id.clone(), "parent turn".to_string());
    let mut request = ChatTurnRequest::new(conversation_id, "parent turn", vec![]);
    request.run_id = run_id;
    (turn, request)
}

fn test_session_id() -> SessionId {
    SessionId::new("task-notification-injection")
}

async fn run_turn_with_queue(
    queue: Arc<TaskNotificationQueue>,
) -> Arc<RecordingExecutor> {
    let executor = Arc::new(RecordingExecutor::default());
    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::new(),
        RuntimeEventBus::new(),
        executor.clone(),
    )
    .with_task_notification_queue(queue);
    let (mut turn, request) = make_turn_and_request();
    driver
        .run_chat_turn(&mut turn, &request)
        .await
        .expect("chat turn should complete");
    executor
}

fn task_notification_user_contents(messages: &[Value]) -> Vec<String> {
    messages
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        .filter_map(|message| message.get("content").and_then(Value::as_str))
        .filter(|content| content.contains("<task-notification>"))
        .map(ToOwned::to_owned)
        .collect()
}

#[tokio::test]
async fn queued_task_notification_is_injected_as_user_message() {
    let queue = Arc::new(TaskNotificationQueue::new());
    let xml = "<task-notification><task-id>agent-x</task-id><status>completed</status></task-notification>";
    queue.enqueue("agent-x", xml, test_session_id(), None);

    let executor = run_turn_with_queue(queue).await;
    let captured = executor.captured_messages();
    assert_eq!(captured.len(), 1);

    let notifications = task_notification_user_contents(&captured[0]);
    assert_eq!(notifications, vec![xml.to_string()]);
}

#[tokio::test]
async fn empty_queue_does_not_add_synthetic_task_notification_message() {
    let queue = Arc::new(TaskNotificationQueue::new());

    let executor = run_turn_with_queue(queue).await;
    let captured = executor.captured_messages();
    assert_eq!(captured.len(), 1);

    let notifications = task_notification_user_contents(&captured[0]);
    assert!(notifications.is_empty());
}

#[tokio::test]
async fn multiple_task_notifications_are_injected_in_enqueue_order() {
    let queue = Arc::new(TaskNotificationQueue::new());
    let xml1 = "<task-notification><task-id>agent-1</task-id><status>completed</status></task-notification>";
    let xml2 = "<task-notification><task-id>agent-2</task-id><status>failed</status></task-notification>";
    queue.enqueue("agent-1", xml1, test_session_id(), None);
    queue.enqueue("agent-2", xml2, test_session_id(), None);

    let executor = run_turn_with_queue(queue).await;
    let captured = executor.captured_messages();
    assert_eq!(captured.len(), 1);

    let notifications = task_notification_user_contents(&captured[0]);
    assert_eq!(notifications, vec![xml1.to_string(), xml2.to_string()]);
}

#[tokio::test]
async fn iteration_time_drain_injects_notifications() {
    let queue = Arc::new(TaskNotificationQueue::new());
    let xml = "<task-notification><task-id>agent-between-iterations</task-id><status>completed</status></task-notification>";
    let executor = Arc::new(IterationDrainExecutor::new(
        queue.clone(),
        xml,
        test_session_id(),
    ));
    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::new(),
        RuntimeEventBus::new(),
        executor.clone(),
    )
    .with_task_notification_queue(queue);
    let (mut turn, request) = make_turn_and_request();

    driver
        .run_chat_turn(&mut turn, &request)
        .await
        .expect("chat turn should complete");

    let captured = executor.captured_messages();
    assert_eq!(captured.len(), 2);
    assert!(task_notification_user_contents(&captured[0]).is_empty());
    assert_eq!(task_notification_user_contents(&captured[1]), vec![xml.to_string()]);
}
