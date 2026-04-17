use std::sync::{Arc, Mutex};

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use app_lib::runtime::chat::turn_config::{LlmStepInput, LlmStepResult, TurnError};
use app_lib::runtime::chat::{ChatTurnRequest, RuntimeChatTurnDriver, RuntimeLlmExecutor};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::events::{RuntimeEvent, RuntimeEventKind};
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::{RunId, SessionId, ToolCallId};
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use app_lib::runtime::tools::permission::{PermissionDecision, PermissionReason};
use app_lib::runtime::tools::{
    PermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatcher, ToolError,
    ToolExecutionContext, ToolResult,
};
use async_trait::async_trait;
use serde_json::{json, Value};

fn make_test_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(mapping, RunId::new("run-a2-test"), "hi".to_string())
}

struct AlwaysAskPermissionPipeline;

impl PermissionPipeline for AlwaysAskPermissionPipeline {
    fn authorize(
        &self,
        definition: &ToolDefinition,
        _input: &Value,
        _ctx: &ToolExecutionContext,
    ) -> PermissionDecision {
        PermissionDecision::Ask {
            message: format!("permission confirmation required for '{}'", definition.id),
            suggestions: vec![
                "Allow once".to_string(),
                "Always allow".to_string(),
                "Deny".to_string(),
            ],
            reason: PermissionReason::UnknownScope,
        }
    }
}

struct EchoTool;

#[async_trait]
impl RuntimeTool for EchoTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("echo_tool", "simple echo tool for testing")
            .with_capability_scope(["custom:test"])
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new("echo_tool", "ok", None))
    }
}

struct ToolCallExecutor {
    responses: Mutex<Vec<LlmStepResult>>,
    received_messages: Mutex<Vec<Vec<Value>>>,
}

impl ToolCallExecutor {
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
impl RuntimeLlmExecutor for ToolCallExecutor {
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
        Ok(responses.remove(0))
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
}

#[test]
fn runtime_event_new_copies_tool_call_id_for_permission_ask_required() {
    let event = RuntimeEvent::new(
        SessionId::new("conv-a2"),
        RunId::new("run-a2"),
        RuntimeEventKind::PermissionAskRequired {
            tool_call_id: ToolCallId::new("tc-a2"),
            tool_name: "echo_tool".to_string(),
            message: "need permission".to_string(),
            suggestions: vec!["Allow once".to_string()],
        },
    );

    assert_eq!(event.tool_call_id.unwrap().as_str(), "tc-a2");
}

#[tokio::test]
async fn driver_emits_permission_ask_runtime_event_before_text_fallback() {
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AlwaysAskPermissionPipeline)));
    dispatcher.register(Arc::new(EchoTool));

    let executor = Arc::new(ToolCallExecutor::new(vec![
        LlmStepResult::ToolCalls {
            assistant_content: "".to_string(),
            tool_calls: vec![RuntimeToolCallRequest {
                tool_call_id: "tc-a2-ask".to_string(),
                tool_name: "echo_tool".to_string(),
                args: json!({}),
                purpose: None,
            }],
            tokens_in: 5,
            tokens_out: 7,
        },
        LlmStepResult::ContentComplete {
            content: "done".to_string(),
            tokens_in: 1,
            tokens_out: 1,
        },
    ]));

    let bus = RuntimeEventBus::new();
    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::with_dispatcher(dispatcher),
        bus.clone(),
        executor.clone(),
    );

    let mut turn = make_test_turn("conv-a2-ask");
    let request = ChatTurnRequest::new("conv-a2-ask", "hi", vec![]);
    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let events = bus.recorded();
    let permission_event = events
        .iter()
        .find(|event| matches!(event.kind, RuntimeEventKind::PermissionAskRequired { .. }))
        .expect("driver must emit PermissionAskRequired event");

    assert_eq!(
        permission_event
            .tool_call_id
            .as_ref()
            .expect("tool_call_id should be copied")
            .as_str(),
        "tc-a2-ask"
    );

    match &permission_event.kind {
        RuntimeEventKind::PermissionAskRequired {
            tool_name,
            message,
            suggestions,
            ..
        } => {
            assert_eq!(tool_name, "echo_tool");
            assert!(message.contains("permission confirmation required"));
            assert_eq!(
                suggestions,
                &vec![
                    "Allow once".to_string(),
                    "Always allow".to_string(),
                    "Deny".to_string(),
                ]
            );
        }
        other => panic!("unexpected event kind: {:?}", other),
    }

    let all_messages = executor.all_messages();
    assert!(all_messages.len() >= 2, "expected at least two llm steps");
    let second_step_messages = &all_messages[1];
    let tool_result_message = second_step_messages
        .iter()
        .find(|message| {
            message.get("role").and_then(|value| value.as_str()) == Some("tool")
                && message
                    .get("toolCallId")
                    .and_then(|value| value.as_str())
                    == Some("tc-a2-ask")
        })
        .expect("AskRequired should still produce text fallback tool result");
    assert!(tool_result_message["content"]
        .as_str()
        .unwrap_or_default()
        .contains("Please await user permission"));
}
