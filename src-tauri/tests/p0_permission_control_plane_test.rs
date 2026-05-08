use std::sync::{Arc, Mutex};
use std::time::Duration;

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use app_lib::runtime::chat::turn_config::{LlmStepInput, LlmStepResult, TurnError};
use app_lib::runtime::chat::{ChatTurnRequest, RuntimeChatTurnDriver, RuntimeLlmExecutor};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::events::RuntimeEventKind;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::{RunId, ToolCallId};
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use app_lib::runtime::store::{PendingPermissionRequestStore, PendingPermissionResolution};
use app_lib::runtime::tools::permission::{
    default_permission_ask, PermissionDecision, PermissionDestination, PermissionMode,
    PermissionReason,
};
use app_lib::runtime::tools::{
    PermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatcher, ToolError,
    ToolExecutionContext, ToolResult,
};
use async_trait::async_trait;
use serde_json::{json, Value};

fn make_test_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(mapping, RunId::new("run-permission-test"), "hi".to_string())
}

async fn wait_for_pending_request(
    store: &PendingPermissionRequestStore,
    tool_call_id: &str,
) -> app_lib::runtime::store::PendingPermissionRequest {
    let id = ToolCallId::new(tool_call_id);
    for _ in 0..100 {
        if let Some(request) = store.get(&id) {
            return request;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for pending permission request: {tool_call_id}");
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
            remember_options: default_permission_ask().0,
            default_destination: default_permission_ask().1,
            reason: PermissionReason::UnknownScope,
            path_auth_scope: None,
        }
    }
}

struct EchoInputTool {
    received_inputs: Arc<Mutex<Vec<Value>>>,
}

#[async_trait]
impl RuntimeTool for EchoInputTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("echo_tool", "echo tool for permission control plane")
            .with_capability_scope(["custom:test"])
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        self.received_inputs.lock().unwrap().push(input.clone());
        Ok(ToolResult::new(
            "echo_tool",
            format!("executed with {}", input),
            None,
        ))
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
        _tool_calls: &[serde_json::Value],
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("mock-msg-id".to_string())
    }
}

#[tokio::test]
async fn ask_request_is_recorded_without_completed_error_event() {
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AlwaysAskPermissionPipeline)));
    let received_inputs = Arc::new(Mutex::new(Vec::new()));
    dispatcher.register(Arc::new(EchoInputTool {
        received_inputs: received_inputs.clone(),
    }));

    let executor = Arc::new(ToolCallExecutor::new(vec![
        LlmStepResult::ToolCalls {
            assistant_content: "".to_string(),
            tool_calls: vec![RuntimeToolCallRequest {
                tool_call_id: "tc-ask-recorded".to_string(),
                tool_name: "echo_tool".to_string(),
                args: json!({ "value": "original" }),
                purpose: None,
            }],
            tokens_in: 5,
            tokens_out: 7,
        },
        LlmStepResult::ContentComplete {
            content: "done".to_string(),
            tokens_in: 1,
            tokens_out: 1,
            stop_reason: Some("end_turn".to_string()),
        },
    ]));

    let bus = RuntimeEventBus::new();
    let pending_store = Arc::new(PendingPermissionRequestStore::new());
    let driver = RuntimeChatTurnDriver::with_llm_executor_and_permission_control_plane(
        QueryEngine::with_dispatcher(dispatcher),
        bus.clone(),
        executor.clone(),
        pending_store.clone(),
    );

    let mut turn = make_test_turn("conv-permission-recorded");
    let request = ChatTurnRequest::new("conv-permission-recorded", "hi", vec![]);

    let handle = tokio::spawn(async move { driver.run_chat_turn(&mut turn, &request).await });

    let pending = wait_for_pending_request(&pending_store, "tc-ask-recorded").await;
    assert_eq!(pending.tool_name, "echo_tool");
    assert!(pending.message.contains("permission confirmation required"));
    assert_eq!(
        pending.suggestions,
        vec![
            "Allow once".to_string(),
            "Always allow".to_string(),
            "Deny".to_string(),
        ]
    );
    assert_eq!(pending.mode, PermissionMode::Default);
    assert_eq!(
        pending.remember_options,
        vec![
            PermissionDestination::Session,
            PermissionDestination::Workspace,
            PermissionDestination::User,
        ]
    );
    assert_eq!(
        pending.default_destination,
        Some(PermissionDestination::Session)
    );

    let events = bus.recorded();
    assert!(
        events
            .iter()
            .any(|event| matches!(event.kind, RuntimeEventKind::PermissionAskRequired { .. })),
        "ask should emit PermissionAskRequired"
    );
    assert!(
        !events.iter().any(|event| {
            matches!(
                &event.kind,
                RuntimeEventKind::ToolCallCompleted { tool_call_id, .. }
                    if tool_call_id.as_str() == "tc-ask-recorded"
            )
        }),
        "ask must not emit ToolCallCompleted before user resolution"
    );
    assert_eq!(
        executor.all_messages().len(),
        1,
        "driver should pause after permission ask instead of sending fallback tool_result to the LLM"
    );

    pending_store
        .resolve(
            &ToolCallId::new("tc-ask-recorded"),
            PendingPermissionResolution::Deny {
                message: "Denied by user".to_string(),
                remember: false,
                destination: None,
            },
        )
        .expect("deny pending request");

    handle.await.unwrap().unwrap();
    assert!(
        received_inputs.lock().unwrap().is_empty(),
        "denied permission must not execute the tool"
    );
}

#[tokio::test]
async fn approve_replays_original_tool_call_with_updated_input() {
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AlwaysAskPermissionPipeline)));
    let received_inputs = Arc::new(Mutex::new(Vec::new()));
    dispatcher.register(Arc::new(EchoInputTool {
        received_inputs: received_inputs.clone(),
    }));

    let executor = Arc::new(ToolCallExecutor::new(vec![
        LlmStepResult::ToolCalls {
            assistant_content: "".to_string(),
            tool_calls: vec![RuntimeToolCallRequest {
                tool_call_id: "tc-approve-updated-input".to_string(),
                tool_name: "echo_tool".to_string(),
                args: json!({ "value": "original" }),
                purpose: None,
            }],
            tokens_in: 3,
            tokens_out: 5,
        },
        LlmStepResult::ContentComplete {
            content: "done".to_string(),
            tokens_in: 1,
            tokens_out: 1,
            stop_reason: Some("end_turn".to_string()),
        },
    ]));

    let bus = RuntimeEventBus::new();
    let pending_store = Arc::new(PendingPermissionRequestStore::new());
    let driver = RuntimeChatTurnDriver::with_llm_executor_and_permission_control_plane(
        QueryEngine::with_dispatcher(dispatcher),
        bus,
        executor.clone(),
        pending_store.clone(),
    );

    let mut turn = make_test_turn("conv-approve-updated-input");
    let request = ChatTurnRequest::new("conv-approve-updated-input", "hi", vec![]);

    let handle = tokio::spawn(async move { driver.run_chat_turn(&mut turn, &request).await });

    wait_for_pending_request(&pending_store, "tc-approve-updated-input").await;
    pending_store
        .resolve(
            &ToolCallId::new("tc-approve-updated-input"),
            PendingPermissionResolution::Allow {
                updated_input: Some(json!({ "value": "patched" })),
                remember: true,
                destination: Some(PermissionDestination::Workspace),
            },
        )
        .expect("approve pending request");

    handle.await.unwrap().unwrap();

    assert_eq!(
        received_inputs.lock().unwrap().as_slice(),
        &[json!({ "value": "patched" })],
        "approved request must replay the original tool call with updated_input"
    );
    assert!(
        pending_store
            .get(&ToolCallId::new("tc-approve-updated-input"))
            .is_none(),
        "resolved request must be removed from pending store"
    );

    let all_messages = executor.all_messages();
    let second_step_messages = &all_messages[1];
    let tool_result_message = second_step_messages
        .iter()
        .find(|message| {
            message.get("role").and_then(|value| value.as_str()) == Some("tool")
                && message.get("toolCallId").and_then(|value| value.as_str())
                    == Some("tc-approve-updated-input")
        })
        .expect("approved ask must replay as a normal tool result");
    let content = tool_result_message["content"].as_str().unwrap_or_default();
    assert!(content.contains("patched"));
    assert!(!content.contains("Please await user permission"));
}

#[tokio::test]
async fn cancel_clears_pending_request_and_resumes_with_cancelled_outcome() {
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AlwaysAskPermissionPipeline)));
    let received_inputs = Arc::new(Mutex::new(Vec::new()));
    dispatcher.register(Arc::new(EchoInputTool {
        received_inputs: received_inputs.clone(),
    }));

    let executor = Arc::new(ToolCallExecutor::new(vec![
        LlmStepResult::ToolCalls {
            assistant_content: "".to_string(),
            tool_calls: vec![RuntimeToolCallRequest {
                tool_call_id: "tc-cancel-request".to_string(),
                tool_name: "echo_tool".to_string(),
                args: json!({ "value": "original" }),
                purpose: None,
            }],
            tokens_in: 3,
            tokens_out: 5,
        },
        LlmStepResult::ContentComplete {
            content: "done".to_string(),
            tokens_in: 1,
            tokens_out: 1,
            stop_reason: Some("end_turn".to_string()),
        },
    ]));

    let pending_store = Arc::new(PendingPermissionRequestStore::new());
    let driver = RuntimeChatTurnDriver::with_llm_executor_and_permission_control_plane(
        QueryEngine::with_dispatcher(dispatcher),
        RuntimeEventBus::new(),
        executor.clone(),
        pending_store.clone(),
    );

    let mut turn = make_test_turn("conv-cancel-request");
    let request = ChatTurnRequest::new("conv-cancel-request", "hi", vec![]);

    let handle = tokio::spawn(async move { driver.run_chat_turn(&mut turn, &request).await });

    wait_for_pending_request(&pending_store, "tc-cancel-request").await;
    pending_store
        .resolve(
            &ToolCallId::new("tc-cancel-request"),
            PendingPermissionResolution::Cancel {
                message: "Permission request cancelled by user.".to_string(),
            },
        )
        .expect("cancel pending request");

    handle.await.unwrap().unwrap();

    assert!(
        pending_store
            .get(&ToolCallId::new("tc-cancel-request"))
            .is_none(),
        "cancelled request must be removed from pending store"
    );
    assert!(
        received_inputs.lock().unwrap().is_empty(),
        "cancelled permission request must not execute the tool"
    );

    let all_messages = executor.all_messages();
    let second_step_messages = &all_messages[1];
    let tool_result_message = second_step_messages
        .iter()
        .find(|message| {
            message.get("role").and_then(|value| value.as_str()) == Some("tool")
                && message.get("toolCallId").and_then(|value| value.as_str())
                    == Some("tc-cancel-request")
        })
        .expect("cancelled ask must resume with a terminal tool result");
    assert!(tool_result_message["content"]
        .as_str()
        .unwrap_or_default()
        .contains("cancelled by user"));
}

#[test]
fn permission_control_plane_review_resolution_carries_remember_destination_fields() {
    let source = std::fs::read_to_string("src/runtime/store/pending_permission_request_store.rs")
        .expect("read pending_permission_request_store.rs");
    assert!(
        source.contains("remember: bool"),
        "Allow/Deny resolution should persist remember flag for later rule writes"
    );
    assert!(
        source.contains("destination: Option<PermissionDestination>"),
        "Allow/Deny resolution should carry destination so SessionRuntime can write to the selected rule layer"
    );
}

#[test]
fn review_driver_does_not_own_pending_permission_store_field() {
    let driver_src = std::fs::read_to_string("src/runtime/chat/chat_turn_driver.rs")
        .expect("read chat_turn_driver.rs");
    let session_runtime_src =
        std::fs::read_to_string("src/runtime/session_runtime.rs").expect("read session_runtime.rs");
    assert!(
        !driver_src.contains("pending_permission_store:"),
        "driver 不应再持有 pending_permission_store 字段，owner 应收敛到 SessionRuntime/runtime service"
    );
    assert!(
        !driver_src.contains("PendingPermissionRequestStore::new()"),
        "driver 不应再偷偷创建私有 PendingPermissionRequestStore"
    );
    assert!(
        session_runtime_src.contains("with_llm_executor_and_permission_control_plane("),
        "SessionRuntime 应负责把统一的 permission control plane 注入 driver"
    );
    assert!(
        session_runtime_src.contains("self.pending_permission_store.clone()"),
        "SessionRuntime 注入 driver 时应继续复用自己的 pending_permission_store 真源"
    );
}

#[tokio::test]
async fn driver_without_permission_control_plane_fails_fast_on_ask_required() {
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AlwaysAskPermissionPipeline)));
    dispatcher.register(Arc::new(EchoInputTool {
        received_inputs: Arc::new(Mutex::new(Vec::new())),
    }));

    let executor = Arc::new(ToolCallExecutor::new(vec![LlmStepResult::ToolCalls {
        assistant_content: "".to_string(),
        tool_calls: vec![RuntimeToolCallRequest {
            tool_call_id: "tc-ask-no-control-plane".to_string(),
            tool_name: "echo_tool".to_string(),
            args: json!({ "value": "original" }),
            purpose: None,
        }],
        tokens_in: 2,
        tokens_out: 3,
    }]));

    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::with_dispatcher(dispatcher),
        RuntimeEventBus::new(),
        executor,
    );
    let mut turn = make_test_turn("conv-ask-no-control-plane");
    let request = ChatTurnRequest::new("conv-ask-no-control-plane", "hi", vec![]);

    let result = tokio::time::timeout(
        Duration::from_millis(300),
        driver.run_chat_turn(&mut turn, &request),
    )
    .await;

    let run_outcome = result.expect("driver should fail fast instead of waiting forever");
    let err = run_outcome.expect_err("missing permission control plane should return error");
    assert!(
        err.to_string().contains("permission control plane"),
        "unexpected error: {err:#}"
    );
}
