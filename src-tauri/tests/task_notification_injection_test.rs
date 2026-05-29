use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use app_lib::runtime::agent::task_notification::TaskNotificationQueue;
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::chat_turn_driver::ChatAttachmentRef;
use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use app_lib::runtime::chat::turn_config::{LlmStepInput, LlmStepResult, TurnError};
use app_lib::runtime::chat::{ChatTurnRequest, RuntimeChatTurnDriver, RuntimeLlmExecutor};
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

struct CancelingExecutor {
    seen_messages: Mutex<Vec<Vec<Value>>>,
}

impl CancelingExecutor {
    fn new() -> Self {
        Self {
            seen_messages: Mutex::new(Vec::new()),
        }
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
        self.seen_messages
            .lock()
            .unwrap()
            .push(input.messages.clone());
        Ok(LlmStepResult::ContentComplete {
            content: "done".to_string(),
            tokens_in: 0,
            tokens_out: 0,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
            thinking_blocks: Vec::new(),
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
        _skill_command: Option<&app_lib::runtime::chat::chat_turn_driver::SkillCommandRef>,
        _client_message_id: Option<&str>,
    ) -> Result<String, TurnError> {
        Ok("user-msg".to_string())
    }

    async fn load_workspace_path(&self) -> Result<PathBuf, TurnError> {
        Ok(std::env::temp_dir())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![]) // 显式声明此 mock 不关心 tool_defs
    }
}

#[async_trait]
impl RuntimeLlmExecutor for CancelingExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        self.seen_messages
            .lock()
            .unwrap()
            .push(input.messages.clone());
        Ok(LlmStepResult::Cancelled)
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
        _skill_command: Option<&app_lib::runtime::chat::chat_turn_driver::SkillCommandRef>,
        _client_message_id: Option<&str>,
    ) -> Result<String, TurnError> {
        Ok("user-msg".to_string())
    }

    async fn load_workspace_path(&self) -> Result<PathBuf, TurnError> {
        Ok(std::env::temp_dir())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![]) // 显式声明此 mock 不关心 tool_defs
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
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: 0,
                thinking_blocks: Vec::new(),
            });
        }

        Ok(LlmStepResult::ContentComplete {
            content: "done".to_string(),
            tokens_in: 0,
            tokens_out: 0,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
            thinking_blocks: Vec::new(),
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
        _skill_command: Option<&app_lib::runtime::chat::chat_turn_driver::SkillCommandRef>,
        _client_message_id: Option<&str>,
    ) -> Result<String, TurnError> {
        Ok("user-msg".to_string())
    }

    async fn persist_iteration_assistant_message(
        &self,
        _conversation_id: &str,
        _assistant_content: &str,
        _tool_calls: &[Value],
        _thinking_blocks: &[Value],
    ) -> Result<Option<String>, TurnError> {
        Ok(Some("iteration-assistant-msg".to_string()))
    }

    async fn load_workspace_path(&self) -> Result<PathBuf, TurnError> {
        Ok(std::env::temp_dir())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![]) // 显式声明此 mock 不关心 tool_defs
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

async fn run_turn_with_queue(queue: Arc<TaskNotificationQueue>) -> Arc<RecordingExecutor> {
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
    let xml2 =
        "<task-notification><task-id>agent-2</task-id><status>failed</status></task-notification>";
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
    assert_eq!(
        task_notification_user_contents(&captured[1]),
        vec![xml.to_string()]
    );
}

/// M1.5 / F4: initial-turn injection must place <task-notification> AFTER the
/// current user message so async sub-agent completions are the most recent
/// user input. If injected before user_message, the LLM tends to respond to
/// user_message and ignore the notifications.
#[tokio::test]
async fn initial_injection_places_task_notification_after_user_message() {
    let queue = Arc::new(TaskNotificationQueue::new());
    let xml = "<task-notification><task-id>agent-prior</task-id><status>completed</status></task-notification>";
    queue.enqueue("agent-prior", xml, test_session_id(), None);

    let executor = run_turn_with_queue(queue).await;
    let captured = executor.captured_messages();
    assert_eq!(captured.len(), 1);

    let messages = &captured[0];
    // Find indices of the parent's user_message ("parent turn") and the notification.
    let parent_user_idx = messages.iter().position(|m| {
        m.get("role").and_then(Value::as_str) == Some("user")
            && m.get("content").and_then(Value::as_str) == Some("parent turn")
    });
    let notif_idx = messages.iter().position(|m| {
        m.get("role").and_then(Value::as_str) == Some("user")
            && m.get("content")
                .and_then(Value::as_str)
                .map(|c| c.contains("<task-notification>"))
                .unwrap_or(false)
    });
    let parent_user_idx = parent_user_idx.expect("parent user_message must be present");
    let notif_idx = notif_idx.expect("task-notification user_message must be present");
    assert!(
        notif_idx > parent_user_idx,
        "task-notification (idx {notif_idx}) must come AFTER parent user_message (idx {parent_user_idx})"
    );
}

#[tokio::test]
async fn notifications_for_other_sessions_remain_queued() {
    let queue = Arc::new(TaskNotificationQueue::new());
    let xml_a = "<task-notification><task-id>agent-a</task-id><status>completed</status></task-notification>";
    let xml_b = "<task-notification><task-id>agent-b</task-id><status>completed</status></task-notification>";
    let session_a = SessionId::new("task-notification-session-a");
    let session_b = SessionId::new("task-notification-session-b");
    queue.enqueue("agent-a", xml_a, session_a.clone(), None);
    queue.enqueue("agent-b", xml_b, session_b.clone(), None);

    let executor = Arc::new(RecordingExecutor::default());
    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::new(),
        RuntimeEventBus::new(),
        executor.clone(),
    )
    .with_task_notification_queue(queue.clone());

    let run_a = RunId::new("run-a");
    let mut turn_a = TurnState::new(
        IdentityMapping::direct(session_a.clone()),
        run_a.clone(),
        "parent turn".to_string(),
    );
    let mut request_a = ChatTurnRequest::new(session_a.clone(), "parent turn", vec![]);
    request_a.run_id = run_a;

    driver
        .run_chat_turn(&mut turn_a, &request_a)
        .await
        .expect("chat turn for session A should complete");

    let captured = executor.captured_messages();
    assert_eq!(captured.len(), 1);
    let notifications_a = task_notification_user_contents(&captured[0]);
    assert_eq!(notifications_a, vec![xml_a.to_string()]);

    let remaining = queue.drain_for_session(&session_b);
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].xml, xml_b);
}

#[tokio::test]
async fn cancelled_turn_re_enqueues_injected_task_notification() {
    let queue = Arc::new(TaskNotificationQueue::new());
    let xml = "<task-notification><task-id>agent-cancel</task-id><status>completed</status></task-notification>";
    let session = test_session_id();
    queue.enqueue("agent-cancel", xml, session.clone(), None);

    let canceling = Arc::new(CancelingExecutor::new());
    let cancel_driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::new(),
        RuntimeEventBus::new(),
        canceling.clone(),
    )
    .with_task_notification_queue(queue.clone());

    let run_id_cancel = RunId::new("run-cancel");
    let mut cancel_turn = TurnState::new(
        IdentityMapping::direct(session.clone()),
        run_id_cancel.clone(),
        "parent turn".to_string(),
    );
    let mut cancel_request = ChatTurnRequest::new(session.clone(), "parent turn", vec![]);
    cancel_request.run_id = run_id_cancel;

    cancel_driver
        .run_chat_turn(&mut cancel_turn, &cancel_request)
        .await
        .expect("cancelled chat turn should still complete");

    assert_eq!(
        queue.pending_count(),
        1,
        "cancelled turn must re-enqueue the drained notification"
    );

    let finisher = Arc::new(RecordingExecutor::default());
    let finish_driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::new(),
        RuntimeEventBus::new(),
        finisher.clone(),
    )
    .with_task_notification_queue(queue.clone());

    let run_id_finish = RunId::new("run-finish");
    let mut finish_turn = TurnState::new(
        IdentityMapping::direct(session.clone()),
        run_id_finish.clone(),
        "parent turn".to_string(),
    );
    let mut finish_request = ChatTurnRequest::new(session.clone(), "parent turn", vec![]);
    finish_request.run_id = run_id_finish;

    finish_driver
        .run_chat_turn(&mut finish_turn, &finish_request)
        .await
        .expect("follow-up chat turn should complete");

    let captured = finisher.captured_messages();
    assert_eq!(captured.len(), 1);
    assert_eq!(
        task_notification_user_contents(&captured[0]),
        vec![xml.to_string()],
        "follow-up turn must receive the re-enqueued notification"
    );
    assert_eq!(
        queue.pending_count(),
        0,
        "follow-up turn must drain the re-enqueued notification"
    );
}
