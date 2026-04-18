use std::sync::{Arc, Mutex};

use app_lib::runtime::cancellation::{CancellationReason, CancellationToken};
use app_lib::runtime::chat::chat_turn_driver::inject_synthetic_tool_results_for_missing_calls;
use app_lib::runtime::chat::turn_config::{LlmStepInput, LlmStepResult, TurnError};
use app_lib::runtime::chat::{ChatTurnRequest, RuntimeChatTurnDriver, RuntimeLlmExecutor};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::events::RuntimeEventKind;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use async_trait::async_trait;
use serde_json::{json, Value as JsonValue};

fn make_test_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(mapping, RunId::new("test-run"), "hi".to_string())
}

struct RecordingExecutor {
    responses: Mutex<Vec<LlmStepResult>>,
    received_messages: Mutex<Vec<Vec<JsonValue>>>,
    history: Vec<JsonValue>,
}

impl RecordingExecutor {
    fn new(responses: Vec<LlmStepResult>, history: Vec<JsonValue>) -> Self {
        Self {
            responses: Mutex::new(responses),
            received_messages: Mutex::new(Vec::new()),
            history,
        }
    }

    fn all_messages(&self) -> Vec<Vec<JsonValue>> {
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
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            Ok(LlmStepResult::ContentComplete {
                content: "done".to_string(),
                tokens_in: 0,
                tokens_out: 0,
            })
        } else {
            Ok(responses.remove(0))
        }
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("mock-msg-id".to_string())
    }

    async fn load_history(&self, _conversation_id: &str) -> Result<Vec<JsonValue>, TurnError> {
        Ok(self.history.clone())
    }
}

#[tokio::test]
async fn driver_carries_assistant_tool_calls_into_next_llm_input() {
    let tool_call_id = "tc-a1-preserve".to_string();
    let executor = Arc::new(RecordingExecutor::new(
        vec![
            LlmStepResult::ToolCalls {
                assistant_content: "".to_string(),
                tool_calls: vec![RuntimeToolCallRequest {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: "unknown_tool".to_string(),
                    args: json!({}),
                    purpose: None,
                }],
                tokens_in: 12,
                tokens_out: 7,
            },
            LlmStepResult::ContentComplete {
                content: "ok".to_string(),
                tokens_in: 3,
                tokens_out: 2,
            },
        ],
        vec![],
    ));

    let bus = RuntimeEventBus::new();
    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::default(),
        bus,
        executor.clone(),
    );

    let mut turn = make_test_turn("conv-a1-preserve");
    let request = ChatTurnRequest::new("conv-a1-preserve", "hi", vec![]);
    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let all_messages = executor.all_messages();
    assert!(all_messages.len() >= 2, "expected at least two llm steps");
    let second_step_messages = &all_messages[1];

    let assistant_with_tool_call = second_step_messages.iter().find(|msg| {
        msg.get("role").and_then(|v| v.as_str()) == Some("assistant") && msg.get("toolCalls").is_some()
    });

    assert!(assistant_with_tool_call.is_some(), "second llm call should include assistant message with toolCalls");
    let msg = assistant_with_tool_call.unwrap();
    let actual_id = msg["toolCalls"][0]["id"].as_str().unwrap_or_default();
    assert_eq!(actual_id, tool_call_id);
}

#[test]
fn injects_synthetic_tool_result_for_unmatched_assistant_tool_call() {
    let mut messages = vec![
        json!({
            "role": "assistant",
            "content": "",
            "toolCalls": [
                {"id": "tc-a1-missing", "name": "unknown_tool", "arguments": {}}
            ]
        }),
    ];

    let injected = inject_synthetic_tool_results_for_missing_calls(
        &mut messages,
        Some(CancellationReason::UserCancel),
    );
    assert_eq!(injected, 1, "must inject one synthetic tool result");

    let synthetic = messages
        .iter()
        .find(|msg| {
            msg.get("role").and_then(|v| v.as_str()) == Some("tool")
                && msg.get("toolCallId").and_then(|v| v.as_str()) == Some("tc-a1-missing")
        })
        .expect("synthetic tool result should exist");

    assert_eq!(
        synthetic.get("content").and_then(|v| v.as_str()).unwrap_or_default(),
        "Tool execution was interrupted by user cancellation.",
    );
}

#[test]
fn injects_reason_specific_synthetic_tool_result_for_interrupt() {
    let mut messages = vec![json!({
        "role": "assistant",
        "content": "",
        "toolCalls": [
            {"id": "tc-a1-interrupt", "name": "unknown_tool", "arguments": {}}
        ]
    })];

    let injected = inject_synthetic_tool_results_for_missing_calls(
        &mut messages,
        Some(CancellationReason::Interrupt),
    );
    assert_eq!(injected, 1, "must inject one synthetic tool result");

    let synthetic = messages
        .iter()
        .find(|msg| {
            msg.get("role").and_then(|v| v.as_str()) == Some("tool")
                && msg.get("toolCallId").and_then(|v| v.as_str()) == Some("tc-a1-interrupt")
        })
        .expect("synthetic tool result should exist");

    assert_eq!(
        synthetic.get("content").and_then(|v| v.as_str()).unwrap_or_default(),
        "Tool execution was interrupted before completion.",
    );
}

#[test]
fn injects_reason_specific_synthetic_tool_result_for_sibling_error() {
    let mut messages = vec![json!({
        "role": "assistant",
        "content": "",
        "toolCalls": [
            {"id": "tc-a1-sibling", "name": "unknown_tool", "arguments": {}}
        ]
    })];

    let injected = inject_synthetic_tool_results_for_missing_calls(
        &mut messages,
        Some(CancellationReason::SiblingError),
    );
    assert_eq!(injected, 1, "must inject one synthetic tool result");

    let synthetic = messages
        .iter()
        .find(|msg| {
            msg.get("role").and_then(|v| v.as_str()) == Some("tool")
                && msg.get("toolCallId").and_then(|v| v.as_str()) == Some("tc-a1-sibling")
        })
        .expect("synthetic tool result should exist");

    assert_eq!(
        synthetic.get("content").and_then(|v| v.as_str()).unwrap_or_default(),
        "Tool execution was cancelled because another tool call failed.",
    );
}

#[tokio::test]
async fn cancelled_turn_still_emits_stream_done() {
    let executor = Arc::new(RecordingExecutor::new(
        vec![LlmStepResult::Cancelled],
        vec![json!({
            "role": "assistant",
            "content": "",
            "toolCalls": [
                {"id": "tc-a1-cancel", "name": "unknown_tool", "arguments": {}}
            ]
        })],
    ));

    let bus = RuntimeEventBus::new();
    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::default(),
        bus.clone(),
        executor,
    );

    let mut turn = make_test_turn("conv-a1-cancel");
    let request = ChatTurnRequest::new("conv-a1-cancel", "cancel", vec![]);
    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let events = bus.recorded();
    assert!(
        events
            .iter()
            .any(|e| matches!(e.kind, RuntimeEventKind::StreamDone)),
        "cancel path must still emit StreamDone"
    );
}
