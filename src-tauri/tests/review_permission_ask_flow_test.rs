use std::sync::{Arc, Mutex};
use std::time::Duration;

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
use app_lib::runtime::store::{PendingPermissionRequestStore, PendingPermissionResolution};
use app_lib::runtime::tools::description_context::ToolDescriptionContext;
use app_lib::runtime::tools::permission::{
    default_permission_ask, PermissionDecision, PermissionDestination, PermissionMode,
    PermissionReason,
};
use app_lib::runtime::tools::{
    PermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatcher, ToolError,
    ToolExecutionContext, ToolResult,
};
use app_lib::transport::tauri_event_adapter::map_runtime_event;
use async_trait::async_trait;
use serde_json::{json, Value};

fn make_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(
        mapping,
        RunId::new("run-permission-ask-flow"),
        "hi".to_string(),
    )
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

struct DemoActionTool {
    name: &'static str,
    output: &'static str,
    received_inputs: Arc<Mutex<Vec<Value>>>,
}

#[async_trait]
impl RuntimeTool for DemoActionTool {
    fn id(&self) -> &str {
        self.name
    }

    async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
        ToolDefinition::new(self.name, "demo mcp action").with_capability_scope(["mcp"])
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        self.received_inputs.lock().unwrap().push(input);
        Ok(ToolResult::new(self.name, self.output.to_string(), None))
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

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![]) // 显式声明此 mock 不关心 tool_defs
    }
}

fn make_driver(
    tools: Vec<Arc<dyn RuntimeTool>>,
    executor: Arc<ToolCallExecutor>,
    bus: RuntimeEventBus,
    pending_store: Arc<PendingPermissionRequestStore>,
) -> RuntimeChatTurnDriver {
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AlwaysAskPermissionPipeline)));
    for tool in tools {
        dispatcher.register(tool);
    }
    RuntimeChatTurnDriver::with_llm_executor_and_permission_control_plane(
        QueryEngine::with_dispatcher(dispatcher),
        bus,
        executor,
        pending_store,
    )
}

fn tool_call(id: &str, name: &str) -> RuntimeToolCallRequest {
    RuntimeToolCallRequest {
        tool_call_id: id.to_string(),
        tool_name: name.to_string(),
        args: json!({ "value": id }),
        purpose: None,
    }
}

fn content_complete() -> LlmStepResult {
    LlmStepResult::ContentComplete {
        content: "done".to_string(),
        tokens_in: 1,
        tokens_out: 1,
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: 0,
        stop_reason: Some("end_turn".to_string()),
    }
}

#[tokio::test]
async fn ask_event_contains_full_permission_information() {
    let received_inputs = Arc::new(Mutex::new(Vec::new()));
    let executor = Arc::new(ToolCallExecutor::new(vec![
        LlmStepResult::ToolCalls {
            assistant_content: "".to_string(),
            tool_calls: vec![tool_call("tc-ask-info", "mcp__demo__action")],
            tokens_in: 5,
            tokens_out: 7,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        },
        content_complete(),
    ]));
    let bus = RuntimeEventBus::new();
    let pending_store = Arc::new(PendingPermissionRequestStore::new());
    let driver = make_driver(
        vec![Arc::new(DemoActionTool {
            name: "mcp__demo__action",
            output: "执行完成",
            received_inputs,
        })],
        executor,
        bus.clone(),
        pending_store.clone(),
    );

    let mut turn = make_turn("conv-ask-info");
    let request = ChatTurnRequest::new("conv-ask-info", "hi", vec![]);
    let handle = tokio::spawn(async move { driver.run_chat_turn(&mut turn, &request).await });

    let pending = wait_for_pending_request(&pending_store, "tc-ask-info").await;
    assert_eq!(pending.tool_name, "mcp__demo__action");
    assert!(!pending.message.is_empty());
    assert!(pending.suggestions.contains(&"Allow once".to_string()));
    assert!(pending.suggestions.contains(&"Deny".to_string()));
    assert!(pending
        .remember_options
        .contains(&PermissionDestination::Session));
    assert!(pending
        .remember_options
        .contains(&PermissionDestination::Workspace));
    assert!(pending
        .remember_options
        .contains(&PermissionDestination::User));
    assert_eq!(pending.mode, PermissionMode::Default);
    assert_eq!(
        pending.default_destination,
        Some(PermissionDestination::Session)
    );

    let events = bus.recorded();
    let ask_event = events
        .iter()
        .find(|event| matches!(event.kind, RuntimeEventKind::PermissionAskRequired { .. }))
        .expect("PermissionAskRequired should be emitted");
    match &ask_event.kind {
        RuntimeEventKind::PermissionAskRequired {
            tool_name,
            message,
            suggestions,
            mode,
            remember_options,
            default_destination,
            ..
        } => {
            assert_eq!(tool_name, "mcp__demo__action");
            assert!(!message.is_empty());
            assert!(suggestions.contains(&"Allow once".to_string()));
            assert!(suggestions.contains(&"Deny".to_string()));
            assert_eq!(*mode, PermissionMode::Default);
            assert!(remember_options.contains(&PermissionDestination::Session));
            assert!(remember_options.contains(&PermissionDestination::Workspace));
            assert!(remember_options.contains(&PermissionDestination::User));
            assert_eq!(*default_destination, Some(PermissionDestination::Session));
        }
        _ => unreachable!(),
    }

    pending_store
        .resolve(
            &ToolCallId::new("tc-ask-info"),
            PendingPermissionResolution::Deny {
                message: "用户拒绝".to_string(),
                remember: false,
                destination: None,
            },
        )
        .expect("resolve ask");
    handle.await.unwrap().unwrap();
}

#[tokio::test]
async fn allow_resolution_replays_tool_and_returns_successful_tool_result_to_llm() {
    let received_inputs = Arc::new(Mutex::new(Vec::new()));
    let executor = Arc::new(ToolCallExecutor::new(vec![
        LlmStepResult::ToolCalls {
            assistant_content: "".to_string(),
            tool_calls: vec![tool_call("tc-allow", "mcp__demo__action")],
            tokens_in: 5,
            tokens_out: 7,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        },
        content_complete(),
    ]));
    let bus = RuntimeEventBus::new();
    let pending_store = Arc::new(PendingPermissionRequestStore::new());
    let driver = make_driver(
        vec![Arc::new(DemoActionTool {
            name: "mcp__demo__action",
            output: "执行完成",
            received_inputs: received_inputs.clone(),
        })],
        executor.clone(),
        bus.clone(),
        pending_store.clone(),
    );

    let mut turn = make_turn("conv-allow");
    let request = ChatTurnRequest::new("conv-allow", "hi", vec![]);
    let handle = tokio::spawn(async move { driver.run_chat_turn(&mut turn, &request).await });

    wait_for_pending_request(&pending_store, "tc-allow").await;
    pending_store
        .resolve(
            &ToolCallId::new("tc-allow"),
            PendingPermissionResolution::Allow {
                updated_input: None,
                remember: false,
                destination: None,
            },
        )
        .expect("allow ask");
    handle.await.unwrap().unwrap();

    assert_eq!(received_inputs.lock().unwrap().len(), 1);
    let events = bus.recorded();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.kind, RuntimeEventKind::PermissionAskRequired { .. }))
            .count(),
        1
    );
    assert!(events.iter().any(|event| matches!(
        &event.kind,
        RuntimeEventKind::ToolCallCompleted { tool_call_id, is_error: false, .. }
            if tool_call_id.as_str() == "tc-allow"
    )));
    let second_step = &executor.all_messages()[1];
    let tool_result = second_step
        .iter()
        .find(|message| message.get("toolCallId").and_then(Value::as_str) == Some("tc-allow"))
        .expect("tool result should be sent to LLM");
    assert!(tool_result["content"]
        .as_str()
        .unwrap_or_default()
        .contains("执行完成"));
}

#[tokio::test]
async fn deny_resolution_returns_error_tool_result_and_turn_continues() {
    let received_inputs = Arc::new(Mutex::new(Vec::new()));
    let executor = Arc::new(ToolCallExecutor::new(vec![
        LlmStepResult::ToolCalls {
            assistant_content: "".to_string(),
            tool_calls: vec![tool_call("tc-deny", "mcp__demo__action")],
            tokens_in: 5,
            tokens_out: 7,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        },
        content_complete(),
    ]));
    let bus = RuntimeEventBus::new();
    let pending_store = Arc::new(PendingPermissionRequestStore::new());
    let driver = make_driver(
        vec![Arc::new(DemoActionTool {
            name: "mcp__demo__action",
            output: "执行完成",
            received_inputs: received_inputs.clone(),
        })],
        executor.clone(),
        bus.clone(),
        pending_store.clone(),
    );

    let mut turn = make_turn("conv-deny");
    let request = ChatTurnRequest::new("conv-deny", "hi", vec![]);
    let handle = tokio::spawn(async move { driver.run_chat_turn(&mut turn, &request).await });

    wait_for_pending_request(&pending_store, "tc-deny").await;
    pending_store
        .resolve(
            &ToolCallId::new("tc-deny"),
            PendingPermissionResolution::Deny {
                message: "用户拒绝".to_string(),
                remember: false,
                destination: None,
            },
        )
        .expect("deny ask");
    handle.await.unwrap().unwrap();

    assert!(received_inputs.lock().unwrap().is_empty());
    assert!(bus.recorded().iter().any(|event| matches!(
        &event.kind,
        RuntimeEventKind::ToolCallCompleted { tool_call_id, is_error: true, .. }
            if tool_call_id.as_str() == "tc-deny"
    )));
    let all_messages = executor.all_messages();
    assert!(all_messages.len() >= 2, "turn should continue after deny");
    let tool_result = all_messages[1]
        .iter()
        .find(|message| {
            message.get("role").and_then(Value::as_str) == Some("tool")
                && message.get("toolCallId").and_then(Value::as_str) == Some("tc-deny")
        })
        .expect("denied ask should produce tool result");
    assert!(tool_result["content"]
        .as_str()
        .unwrap_or_default()
        .contains("用户拒绝"));
}

#[tokio::test]
async fn cancel_resolution_is_treated_as_denied_tool_result_without_hanging() {
    let received_inputs = Arc::new(Mutex::new(Vec::new()));
    let executor = Arc::new(ToolCallExecutor::new(vec![
        LlmStepResult::ToolCalls {
            assistant_content: "".to_string(),
            tool_calls: vec![tool_call("tc-cancel", "mcp__demo__action")],
            tokens_in: 5,
            tokens_out: 7,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        },
        content_complete(),
    ]));
    let bus = RuntimeEventBus::new();
    let pending_store = Arc::new(PendingPermissionRequestStore::new());
    let driver = make_driver(
        vec![Arc::new(DemoActionTool {
            name: "mcp__demo__action",
            output: "执行完成",
            received_inputs: received_inputs.clone(),
        })],
        executor.clone(),
        bus.clone(),
        pending_store.clone(),
    );

    let mut turn = make_turn("conv-cancel");
    let request = ChatTurnRequest::new("conv-cancel", "hi", vec![]);
    let handle = tokio::spawn(async move { driver.run_chat_turn(&mut turn, &request).await });

    wait_for_pending_request(&pending_store, "tc-cancel").await;
    pending_store
        .resolve(
            &ToolCallId::new("tc-cancel"),
            PendingPermissionResolution::Cancel {
                message: "Permission request cancelled by user.".to_string(),
            },
        )
        .expect("cancel ask");
    tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .expect("turn should not hang")
        .unwrap()
        .unwrap();

    assert!(received_inputs.lock().unwrap().is_empty());
    assert!(bus.recorded().iter().any(|event| matches!(
        &event.kind,
        RuntimeEventKind::ToolCallCompleted { tool_call_id, is_error: true, content, .. }
            if tool_call_id.as_str() == "tc-cancel" && !content.is_empty()
    )));
    assert!(
        executor.all_messages().len() >= 2,
        "turn should continue after cancel"
    );
}

#[tokio::test]
async fn multiple_asks_are_processed_in_order_with_independent_results() {
    let input_a = Arc::new(Mutex::new(Vec::new()));
    let input_b = Arc::new(Mutex::new(Vec::new()));
    let executor = Arc::new(ToolCallExecutor::new(vec![
        LlmStepResult::ToolCalls {
            assistant_content: "".to_string(),
            tool_calls: vec![
                tool_call("tc-a", "mcp__demo__action1"),
                tool_call("tc-b", "mcp__demo__action2"),
            ],
            tokens_in: 5,
            tokens_out: 7,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        },
        content_complete(),
    ]));
    let bus = RuntimeEventBus::new();
    let pending_store = Arc::new(PendingPermissionRequestStore::new());
    let driver = make_driver(
        vec![
            Arc::new(DemoActionTool {
                name: "mcp__demo__action1",
                output: "A 完成",
                received_inputs: input_a.clone(),
            }),
            Arc::new(DemoActionTool {
                name: "mcp__demo__action2",
                output: "B 完成",
                received_inputs: input_b.clone(),
            }),
        ],
        executor,
        bus.clone(),
        pending_store.clone(),
    );

    let mut turn = make_turn("conv-multi-ask");
    let request = ChatTurnRequest::new("conv-multi-ask", "hi", vec![]);
    let handle = tokio::spawn(async move { driver.run_chat_turn(&mut turn, &request).await });

    wait_for_pending_request(&pending_store, "tc-a").await;
    pending_store
        .resolve(
            &ToolCallId::new("tc-a"),
            PendingPermissionResolution::Allow {
                updated_input: None,
                remember: false,
                destination: None,
            },
        )
        .expect("allow first ask");
    wait_for_pending_request(&pending_store, "tc-b").await;
    pending_store
        .resolve(
            &ToolCallId::new("tc-b"),
            PendingPermissionResolution::Deny {
                message: "拒绝 B".to_string(),
                remember: false,
                destination: None,
            },
        )
        .expect("deny second ask");
    handle.await.unwrap().unwrap();

    let events = bus.recorded();
    let ask_tools: Vec<String> = events
        .iter()
        .filter_map(|event| match &event.kind {
            RuntimeEventKind::PermissionAskRequired { tool_name, .. } => Some(tool_name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(ask_tools, vec!["mcp__demo__action1", "mcp__demo__action2"]);
    assert!(events.iter().any(|event| matches!(
        &event.kind,
        RuntimeEventKind::ToolCallCompleted { tool_call_id, is_error: false, .. }
            if tool_call_id.as_str() == "tc-a"
    )));
    assert!(events.iter().any(|event| matches!(
        &event.kind,
        RuntimeEventKind::ToolCallCompleted { tool_call_id, is_error: true, .. }
            if tool_call_id.as_str() == "tc-b"
    )));
}

#[tokio::test]
async fn cancelling_turn_while_waiting_for_ask_exits_without_deadlock() {
    let received_inputs = Arc::new(Mutex::new(Vec::new()));
    let executor = Arc::new(ToolCallExecutor::new(vec![
        LlmStepResult::ToolCalls {
            assistant_content: "".to_string(),
            tool_calls: vec![tool_call("tc-turn-cancel", "mcp__demo__action")],
            tokens_in: 5,
            tokens_out: 7,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        },
        content_complete(),
    ]));
    let bus = RuntimeEventBus::new();
    let pending_store = Arc::new(PendingPermissionRequestStore::new());
    let driver = make_driver(
        vec![Arc::new(DemoActionTool {
            name: "mcp__demo__action",
            output: "执行完成",
            received_inputs,
        })],
        executor,
        bus,
        pending_store.clone(),
    );

    let cancel = CancellationToken::new();
    let mut turn = make_turn("conv-turn-cancel").with_cancellation(cancel.clone());
    let request = ChatTurnRequest::new("conv-turn-cancel", "hi", vec![]);
    let handle = tokio::spawn(async move { driver.run_chat_turn(&mut turn, &request).await });

    wait_for_pending_request(&pending_store, "tc-turn-cancel").await;
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(3), handle)
        .await
        .expect("turn should exit after cancellation")
        .unwrap()
        .unwrap();
    assert!(
        pending_store
            .get(&ToolCallId::new("tc-turn-cancel"))
            .is_none(),
        "cancelled turn should clear pending ask"
    );
}

#[test]
fn permission_ask_required_maps_to_frontend_permission_ask_payload() {
    let event = RuntimeEvent::new(
        SessionId::new("conv-123"),
        RunId::new("run-456"),
        RuntimeEventKind::PermissionAskRequired {
            tool_call_id: ToolCallId::new("tc-map"),
            tool_name: "mcp__demo__action".to_string(),
            message: "需要确认".to_string(),
            suggestions: vec!["Allow once".to_string(), "Deny".to_string()],
            mode: PermissionMode::Default,
            remember_options: vec![PermissionDestination::Session],
            default_destination: Some(PermissionDestination::Session),
            primary_model: "deepseek-v3".into(),
        },
    );

    let legacy = map_runtime_event(&event).expect("permission ask should map to frontend event");
    assert_eq!(legacy.name, "permission:ask");
    assert_eq!(legacy.payload["toolName"], "mcp__demo__action");
    assert_eq!(legacy.payload["message"], "需要确认");
    assert_eq!(legacy.payload["suggestions"].as_array().unwrap().len(), 2);
    assert_eq!(legacy.payload["mode"], "default");
}
