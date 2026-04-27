use std::sync::{Arc, Mutex};
use std::time::Duration;

use app_lib::runtime::cancellation::{CancellationReason, CancellationToken};
use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use app_lib::runtime::chat::turn_config::{
    LlmStepInput, LlmStepResult, TurnConfigOverrides, TurnError,
};
use app_lib::runtime::chat::{ChatTurnOutcome, RuntimeLlmExecutor};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::events::RuntimeEventKind;
use app_lib::runtime::ids::SessionId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::chat::chat_turn_driver::ChatAttachmentRef;
use app_lib::runtime::session_runtime::{ChatTurnRequest, SessionRuntime};
use app_lib::runtime::tools::{
    RuntimeTool, ToolDefinition, ToolError, ToolExecutionContext, ToolResult,
};
use async_trait::async_trait;
use serde_json::{json, Value};

struct SessionTestExecutor {
    responses: Mutex<Vec<LlmStepResult>>,
    captured_messages: Mutex<Vec<Vec<Value>>>,
    overrides: TurnConfigOverrides,
    wait_for_cancel: bool,
}

impl SessionTestExecutor {
    fn new(responses: Vec<LlmStepResult>) -> Self {
        Self {
            responses: Mutex::new(responses),
            captured_messages: Mutex::new(Vec::new()),
            overrides: TurnConfigOverrides::default(),
            wait_for_cancel: false,
        }
    }

    fn with_overrides(mut self, overrides: TurnConfigOverrides) -> Self {
        self.overrides = overrides;
        self
    }

    fn waiting_for_cancel() -> Self {
        Self {
            responses: Mutex::new(Vec::new()),
            captured_messages: Mutex::new(Vec::new()),
            overrides: TurnConfigOverrides::default(),
            wait_for_cancel: true,
        }
    }

    fn captured_messages(&self) -> Vec<Vec<Value>> {
        self.captured_messages.lock().unwrap().clone()
    }
}

#[async_trait]
impl RuntimeLlmExecutor for SessionTestExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        bus: &RuntimeEventBus,
        cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        self.captured_messages
            .lock()
            .unwrap()
            .push(input.messages.clone());
        if self.wait_for_cancel {
            for _ in 0..100 {
                if cancel.is_cancelled() {
                    return Ok(LlmStepResult::Cancelled);
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            return Ok(LlmStepResult::Cancelled);
        }
        let response = {
            let mut responses = self.responses.lock().unwrap();
            responses.remove(0)
        };
        if let LlmStepResult::ContentComplete { content, .. } = &response {
            if !content.is_empty() {
                let _ = bus
                    .emit(app_lib::runtime::events::RuntimeEvent::new(
                        app_lib::runtime::ids::SessionId::new(input.conversation_id),
                        app_lib::runtime::ids::RunId::new(input.run_id),
                        RuntimeEventKind::StreamDelta {
                            content: content.clone(),
                        },
                    ))
                    .await;
            }
        }
        Ok(response)
    }

    async fn load_turn_config_overrides(
        &self,
        _request: &app_lib::runtime::chat::ChatTurnRequest,
    ) -> Result<TurnConfigOverrides, TurnError> {
        Ok(self.overrides.clone())
    }

    async fn persist_user_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _attachments: &[ChatAttachmentRef],
        _client_message_id: Option<&str>,
        _selected_skill_id: Option<&str>,
        _selected_skill_label: Option<&str>,
    ) -> Result<String, TurnError> {
        Ok("user-msg".to_string())
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
}

struct DummyTool;

#[async_trait]
impl RuntimeTool for DummyTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("dummy_tool", "dummy tool")
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new("dummy_tool", "工具结果", None))
    }
}

fn runtime(executor: Arc<SessionTestExecutor>, with_dummy_tool: bool) -> SessionRuntime {
    let dispatcher = Arc::new(app_lib::runtime::tools::ToolDispatcher::new(Arc::new(
        app_lib::runtime::tools::permission::AllowAllPermissionPipeline,
    )));
    if with_dummy_tool {
        dispatcher.register(Arc::new(DummyTool));
    }
    SessionRuntime::with_llm_executor(
        QueryEngine::with_dispatcher(dispatcher),
        RuntimeEventBus::new(),
        executor,
    )
}

fn kind_label(kind: &RuntimeEventKind) -> &'static str {
    match kind {
        RuntimeEventKind::RunStarted => "RunStarted",
        RuntimeEventKind::StreamStarted => "StreamStarted",
        RuntimeEventKind::StreamDelta { .. } => "StreamDelta",
        RuntimeEventKind::MessagePersisted { .. } => "MessagePersisted",
        RuntimeEventKind::StreamDone => "StreamDone",
        RuntimeEventKind::TurnCompleted { .. } => "TurnCompleted",
        RuntimeEventKind::AgentIdle { .. } => "AgentIdle",
        RuntimeEventKind::ToolCallExecuting { .. } => "ToolCallExecuting",
        RuntimeEventKind::ToolCallCompleted { .. } => "ToolCallCompleted",
        _ => "Other",
    }
}

fn index_of(labels: &[&str], label: &str) -> usize {
    labels
        .iter()
        .position(|item| *item == label)
        .expect("label should exist")
}

fn dummy_tool_call(id: &str) -> RuntimeToolCallRequest {
    RuntimeToolCallRequest {
        tool_call_id: id.to_string(),
        tool_name: "dummy_tool".to_string(),
        args: json!({}),
        purpose: None,
    }
}

#[tokio::test]
async fn normal_turn_emits_complete_lifecycle_events_in_order() {
    let executor = Arc::new(SessionTestExecutor::new(vec![
        LlmStepResult::ContentComplete {
            content: "你好！".to_string(),
            tokens_in: 3,
            tokens_out: 5,
            stop_reason: Some("end_turn".to_string()),
        },
    ]));
    let runtime = runtime(executor, false);

    runtime
        .run_chat_request(ChatTurnRequest::new("conv-session-normal", "hi", vec![]))
        .await
        .unwrap();

    let events = runtime.recorded_events();
    let labels: Vec<_> = events.iter().map(|event| kind_label(&event.kind)).collect();
    for required in [
        "RunStarted",
        "StreamStarted",
        "StreamDelta",
        "MessagePersisted",
        "StreamDone",
        "TurnCompleted",
        "AgentIdle",
    ] {
        assert!(labels.contains(&required), "missing {required}: {labels:?}");
    }
    assert_eq!(labels.first(), Some(&"RunStarted"));
    assert_eq!(labels.last(), Some(&"AgentIdle"));
    assert!(index_of(&labels, "StreamDone") < index_of(&labels, "TurnCompleted"));
    assert!(index_of(&labels, "TurnCompleted") < index_of(&labels, "AgentIdle"));
    assert!(!labels.contains(&"ToolCallExecuting"));
    assert!(!labels.contains(&"ToolCallCompleted"));
}

#[tokio::test]
async fn all_events_in_one_turn_share_the_same_run_id() {
    let executor = Arc::new(SessionTestExecutor::new(vec![
        LlmStepResult::ContentComplete {
            content: "OK".to_string(),
            tokens_in: 1,
            tokens_out: 1,
            stop_reason: Some("end_turn".to_string()),
        },
    ]));
    let runtime = runtime(executor, false);

    runtime
        .run_chat_request(ChatTurnRequest::new("conv-session-run-id", "hi", vec![]))
        .await
        .unwrap();

    let events = runtime.recorded_events();
    let first_run_id = events[0].run_id.as_str().to_string();
    assert!(events
        .iter()
        .all(|event| event.run_id.as_str() == first_run_id));
    assert_eq!(
        events.first().unwrap().run_id,
        events.last().unwrap().run_id
    );
}

#[tokio::test]
async fn tool_call_result_is_sent_to_next_llm_step_and_turn_completes() {
    let executor = Arc::new(SessionTestExecutor::new(vec![
        LlmStepResult::ToolCalls {
            assistant_content: "".to_string(),
            tool_calls: vec![dummy_tool_call("tc-dummy")],
            tokens_in: 2,
            tokens_out: 3,
        },
        LlmStepResult::ContentComplete {
            content: "分析完成".to_string(),
            tokens_in: 4,
            tokens_out: 5,
            stop_reason: Some("end_turn".to_string()),
        },
    ]));
    let runtime = runtime(executor.clone(), true);

    runtime
        .run_chat_request(ChatTurnRequest::new(
            "conv-session-tool",
            "use tool",
            vec![],
        ))
        .await
        .unwrap();

    let events = runtime.recorded_events();
    let labels: Vec<_> = events.iter().map(|event| kind_label(&event.kind)).collect();
    assert_eq!(
        labels
            .iter()
            .filter(|label| **label == "ToolCallExecuting")
            .count(),
        1
    );
    assert_eq!(
        labels
            .iter()
            .filter(|label| **label == "ToolCallCompleted")
            .count(),
        1
    );
    assert!(index_of(&labels, "ToolCallExecuting") < index_of(&labels, "ToolCallCompleted"));
    assert!(events.iter().any(|event| matches!(
        event.kind,
        RuntimeEventKind::TurnCompleted {
            outcome: ChatTurnOutcome::Success,
            ..
        }
    )));
    assert!(labels.contains(&"StreamDone"));
    assert_eq!(labels.last(), Some(&"AgentIdle"));

    let messages = executor.captured_messages();
    let second_step = &messages[1];
    let combined = second_step
        .iter()
        .filter_map(|message| message.get("content").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(combined.contains("工具结果"));
}

#[tokio::test]
async fn cancellation_exits_turn_without_hanging_and_emits_cancelled_completion() {
    let executor = Arc::new(SessionTestExecutor::waiting_for_cancel());
    let runtime = Arc::new(runtime(executor, false));
    let session_id = SessionId::new("conv-session-cancel");
    let runtime_for_cancel = runtime.clone();
    let session_for_cancel = session_id.clone();

    let handle = tokio::spawn({
        let runtime = runtime.clone();
        async move {
            runtime
                .run_chat_request(ChatTurnRequest::new(
                    "conv-session-cancel",
                    "cancel",
                    vec![],
                ))
                .await
        }
    });
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        runtime_for_cancel.cancel_session(&session_for_cancel, CancellationReason::UserCancel);
    });

    tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("run_chat_request should not hang")
        .unwrap()
        .unwrap();

    let events = runtime.recorded_events();
    assert!(events.iter().any(|event| matches!(
        event.kind,
        RuntimeEventKind::TurnCompleted {
            outcome: ChatTurnOutcome::Cancelled,
            ..
        }
    )));
    assert!(matches!(
        events.last().unwrap().kind,
        RuntimeEventKind::AgentIdle { .. }
    ));
}

#[tokio::test]
async fn max_iterations_reached_ends_turn_after_configured_limit() {
    let executor = Arc::new(
        SessionTestExecutor::new(vec![
            LlmStepResult::ToolCalls {
                assistant_content: "".to_string(),
                tool_calls: vec![dummy_tool_call("tc-max-1")],
                tokens_in: 1,
                tokens_out: 1,
            },
            LlmStepResult::ToolCalls {
                assistant_content: "".to_string(),
                tool_calls: vec![dummy_tool_call("tc-max-2")],
                tokens_in: 1,
                tokens_out: 1,
            },
        ])
        .with_overrides(TurnConfigOverrides {
            max_iterations: Some(2),
            ..TurnConfigOverrides::default()
        }),
    );
    let runtime = runtime(executor, true);

    runtime
        .run_chat_request(ChatTurnRequest::new("conv-session-max", "loop", vec![]))
        .await
        .unwrap();

    let events = runtime.recorded_events();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.kind, RuntimeEventKind::ToolCallExecuting { .. }))
            .count(),
        2
    );
    assert!(events.iter().any(|event| matches!(
        event.kind,
        RuntimeEventKind::TurnCompleted {
            outcome: ChatTurnOutcome::MaxIterationsReached { iterations: 2 },
            ..
        }
    )));
    assert!(matches!(
        events.last().unwrap().kind,
        RuntimeEventKind::AgentIdle { .. }
    ));
}
