use std::sync::{Arc, Mutex};

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use app_lib::runtime::chat::turn_config::{
    LlmStepInput, LlmStepResult, TurnError, MAX_OUTPUT_TOKENS_RECOVERY_LIMIT,
};
use app_lib::runtime::chat::{ChatTurnRequest, RuntimeChatTurnDriver, RuntimeLlmExecutor};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::events::RuntimeEventKind;
use app_lib::runtime::hooks::config::{HookConfig, HookEvent, HookRegistry};
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::{RunId, SessionId, ToolCallId};
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use app_lib::runtime::store::{
    PendingPermissionRequest, PendingPermissionRequestStore, PendingPermissionResolution,
};
use app_lib::runtime::tools::permission::{PermissionDestination, PermissionMode};
use async_trait::async_trait;
use serde_json::{json, Value};

fn make_test_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(mapping, RunId::new("test-run"), "hi".to_string())
}

#[test]
fn w2_max_output_tokens_recovery_limit_is_three() {
    assert_eq!(MAX_OUTPUT_TOKENS_RECOVERY_LIMIT, 3);
}

struct RecordingExecutor {
    responses: Mutex<Vec<LlmStepResult>>,
    received_messages: Mutex<Vec<Vec<Value>>>,
}

impl RecordingExecutor {
    fn new(responses: Vec<LlmStepResult>) -> Self {
        Self {
            responses: Mutex::new(responses),
            received_messages: Mutex::new(Vec::new()),
        }
    }

    fn all_messages(&self) -> Vec<Vec<Value>> {
        self.received_messages.lock().unwrap().clone()
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
        self.received_messages
            .lock()
            .unwrap()
            .push(input.messages.clone());
        Ok(self.responses.lock().unwrap().remove(0))
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _tool_calls: &[serde_json::Value],
        _generated_file_ids: &[String],
        _file_metas: &[Value],
        _thinking_blocks: &[Value],
    ) -> Result<String, TurnError> {
        Ok("assistant-msg".to_string())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![]) // 显式声明此 mock 不关心 tool_defs
    }
}

#[tokio::test]
async fn w2_max_tokens_injects_resume_message_and_completes() {
    let executor = Arc::new(RecordingExecutor::new(vec![
        LlmStepResult::ContentComplete {
            content: "part-1".to_string(),
            tokens_in: 5,
            tokens_out: 7,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0, thinking_blocks: Vec::new(),
            stop_reason: Some("max_tokens".to_string()),
        },
        LlmStepResult::ContentComplete {
            content: "part-2".to_string(),
            tokens_in: 3,
            tokens_out: 4,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0, thinking_blocks: Vec::new(),
            stop_reason: Some("end_turn".to_string()),
        },
    ]));

    let bus = RuntimeEventBus::new();
    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::default(),
        bus.clone(),
        executor.clone(),
    );
    let mut turn = make_test_turn("conv-w2-recovery");
    let request = ChatTurnRequest::new("conv-w2-recovery", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let calls = executor.all_messages();
    assert_eq!(
        calls.len(),
        2,
        "driver should run a second LLM turn after max_tokens"
    );
    let second_call = &calls[1];
    assert!(
        second_call.iter().any(|msg| {
            msg.get("role").and_then(|v| v.as_str()) == Some("assistant")
                && msg.get("content").and_then(|v| v.as_str()) == Some("part-1")
        }),
        "partial assistant content must be preserved in retry history"
    );
    assert!(
        second_call.iter().any(|msg| {
            msg.get("role").and_then(|v| v.as_str()) == Some("user")
                && msg
                    .get("content")
                    .and_then(|v| v.as_str())
                    .map(|text| text.contains("Output token limit hit. Resume directly"))
                    .unwrap_or(false)
        }),
        "retry history must contain the resume meta message"
    );

    let events = bus.recorded();
    let persisted = events
        .iter()
        .find(|event| {
            matches!(
                &event.kind,
                RuntimeEventKind::MessagePersisted { role, .. } if role == "assistant"
            )
        })
        .expect("assistant message persisted event");
    if let RuntimeEventKind::MessagePersisted { content, .. } = &persisted.kind {
        assert_eq!(content["text"], "part-1part-2");
    }
}

#[tokio::test]
async fn w2_max_tokens_recovery_stops_after_limit_and_keeps_partial_content() {
    let executor = Arc::new(RecordingExecutor::new(
        (0..=MAX_OUTPUT_TOKENS_RECOVERY_LIMIT)
            .map(|idx| LlmStepResult::ContentComplete {
                content: format!("part-{idx}"),
                tokens_in: 1,
                tokens_out: 1,
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: 0, thinking_blocks: Vec::new(),
                stop_reason: Some("max_tokens".to_string()),
            })
            .collect(),
    ));

    let bus = RuntimeEventBus::new();
    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::default(),
        bus.clone(),
        executor.clone(),
    );
    let mut turn = make_test_turn("conv-w2-limit");
    let request = ChatTurnRequest::new("conv-w2-limit", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let calls = executor.all_messages();
    assert_eq!(
        calls.len(),
        MAX_OUTPUT_TOKENS_RECOVERY_LIMIT + 1,
        "driver must stop retrying once the limit is exhausted"
    );

    let events = bus.recorded();
    let persisted = events
        .iter()
        .find(|event| {
            matches!(
                &event.kind,
                RuntimeEventKind::MessagePersisted { role, .. } if role == "assistant"
            )
        })
        .expect("assistant message persisted event");
    if let RuntimeEventKind::MessagePersisted { content, .. } = &persisted.kind {
        let text = content["text"].as_str().unwrap_or("");
        assert!(text.contains("part-0part-1part-2part-3"));
        assert!(
            text.contains("输出 token 上限"),
            "partial content should include a truncation notice once recovery is exhausted"
        );
    }
}

#[tokio::test]
async fn w3_stop_hook_blocking_errors_drive_new_llm_turn_once() {
    let executor = Arc::new(RecordingExecutor::new(vec![
        LlmStepResult::ContentComplete {
            content: "draft".to_string(),
            tokens_in: 1,
            tokens_out: 1,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0, thinking_blocks: Vec::new(),
            stop_reason: Some("end_turn".to_string()),
        },
        LlmStepResult::ContentComplete {
            content: "final".to_string(),
            tokens_in: 1,
            tokens_out: 1,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0, thinking_blocks: Vec::new(),
            stop_reason: Some("end_turn".to_string()),
        },
    ]));

    let bus = RuntimeEventBus::new();
    let driver =
        RuntimeChatTurnDriver::with_llm_executor(QueryEngine::default(), bus, executor.clone());

    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::Stop,
        command: "python3 - <<'PY'\nimport json\nprint(json.dumps({\"behavior\":\"allow\",\"blockingErrors\":[\"Fix: missing summary\"]}))\nPY".to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    });

    let mut turn = make_test_turn("conv-w3-blocking");
    let mut request = ChatTurnRequest::new("conv-w3-blocking", "hello", vec![]);
    request.hook_registry = Some(Arc::new(registry));

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let calls = executor.all_messages();
    assert_eq!(
        calls.len(),
        2,
        "blocking stop-hook errors should trigger exactly one more LLM turn"
    );
    let second_call = &calls[1];
    assert!(
        second_call.iter().any(|msg| {
            msg.get("role").and_then(|v| v.as_str()) == Some("user")
                && msg.get("isMeta").and_then(|v| v.as_bool()) == Some(true)
                && msg
                    .get("content")
                    .and_then(|v| v.as_str())
                    .map(|text| text.contains("Fix: missing summary"))
                    .unwrap_or(false)
        }),
        "blocking errors must be appended as meta user messages before retry"
    );
}

#[tokio::test]
async fn w4_orphaned_permission_is_cancelled_and_event_emitted() {
    let executor = Arc::new(RecordingExecutor::new(vec![
        LlmStepResult::ContentComplete {
            content: "done".to_string(),
            tokens_in: 1,
            tokens_out: 1,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0, thinking_blocks: Vec::new(),
            stop_reason: Some("end_turn".to_string()),
        },
    ]));
    let bus = RuntimeEventBus::new();
    let pending_store = Arc::new(PendingPermissionRequestStore::new());
    let driver = RuntimeChatTurnDriver::with_llm_executor_and_permission_control_plane(
        QueryEngine::default(),
        bus.clone(),
        executor,
        pending_store.clone(),
    );

    let session_id = SessionId::new("conv-w4-orphan");
    let request = PendingPermissionRequest {
        tool_call_id: ToolCallId::new("tc-orphan"),
        session_id: session_id.clone(),
        run_id: RunId::new("run-orphan"),
        tool_name: "echo_tool".to_string(),
        capability_scopes: vec!["custom:test".to_string()],
        message: "need approval".to_string(),
        suggestions: vec!["Allow".to_string()],
        mode: PermissionMode::Default,
        remember_options: vec![PermissionDestination::Session],
        default_destination: Some(PermissionDestination::Session),
        original_request: RuntimeToolCallRequest {
            tool_call_id: "tc-orphan".to_string(),
            tool_name: "echo_tool".to_string(),
            args: json!({"value": 1}),
            purpose: None,
        },
        path_auth_scope: None,
    };
    let resolution_rx = pending_store
        .insert(request)
        .expect("insert orphan pending request");

    let mut turn = make_test_turn("conv-w4-orphan");
    let request = ChatTurnRequest::new("conv-w4-orphan", "hello", vec![]);
    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    match resolution_rx
        .await
        .expect("orphan request should be resolved")
    {
        PendingPermissionResolution::Cancel { message } => {
            assert!(message.contains("orphaned"));
        }
        other => panic!("expected orphaned request to be cancelled, got {other:?}"),
    }

    let events = bus.recorded();
    assert!(events.iter().any(|event| matches!(
        event.kind,
        RuntimeEventKind::OrphanedPermissionDetected { count: 1 }
    )));
}
