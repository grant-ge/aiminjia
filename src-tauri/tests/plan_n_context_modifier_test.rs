use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use app_lib::runtime::chat::turn_config::{LlmStepInput, LlmStepResult, TurnError};
use app_lib::runtime::chat::{ChatTurnRequest, RuntimeChatTurnDriver, RuntimeLlmExecutor};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use app_lib::runtime::tools::description_context::ToolDescriptionContext;
use app_lib::runtime::tools::dispatcher::{RuntimeTool, ToolDispatchOutcome};
use app_lib::runtime::tools::{
    AllowAllPermissionPipeline, ToolDefinition, ToolDispatcher, ToolError, ToolExecutionContext,
    ToolResult,
};

struct ModifyingTool;

#[async_trait]
impl RuntimeTool for ModifyingTool {
    fn id(&self) -> &str {
        "modifying_tool"
    }

    async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
        ToolDefinition::new("modifying_tool", "modifies context")
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        false
    }

    fn context_modifier(&self) -> Option<serde_json::Value> {
        Some(json!({
            "role": "user",
            "content": "<context-update>File was modified by tool.</context-update>"
        }))
    }

    async fn execute(&self, _: Value, _: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new("modifying_tool", "file written", None))
    }
}

struct PlainTool;

#[async_trait]
impl RuntimeTool for PlainTool {
    fn id(&self) -> &str {
        "plain_tool"
    }

    async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
        ToolDefinition::new("plain_tool", "no modifier")
    }

    async fn execute(&self, _: Value, _: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new("plain_tool", "ok", None))
    }
}

#[test]
fn default_context_modifier_is_none() {
    let tool = PlainTool;
    assert!(tool.context_modifier().is_none());
}

#[test]
fn modifying_tool_context_modifier_returns_some() {
    let tool = ModifyingTool;
    let msg = tool.context_modifier();
    assert!(msg.is_some());
    let msg = msg.unwrap();
    assert_eq!(msg.get("role").and_then(|v| v.as_str()), Some("user"));
}

#[tokio::test]
async fn dispatch_outcome_includes_context_modifier_message() {
    let tool = Arc::new(ModifyingTool);
    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(tool);

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
    let outcome = dispatcher
        .dispatch("modifying_tool", json!({}), ctx)
        .await
        .unwrap();

    match outcome {
        ToolDispatchOutcome::Completed {
            context_modifier_message,
            ..
        } => {
            assert!(context_modifier_message.is_some());
            let msg = context_modifier_message.unwrap();
            assert_eq!(msg.get("role").and_then(|v| v.as_str()), Some("user"));
        }
        _ => panic!("expected Completed"),
    }
}

#[tokio::test]
async fn concurrent_safe_tool_modifier_ignored() {
    struct ConcurrentModifyingTool;

    #[async_trait]
    impl RuntimeTool for ConcurrentModifyingTool {
        fn id(&self) -> &str {
            "conc_mod"
        }

        async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
            ToolDefinition::new("conc_mod", "concurrent")
        }

        fn is_concurrency_safe(&self, _: &Value) -> bool {
            true
        }

        fn context_modifier(&self) -> Option<serde_json::Value> {
            Some(json!({"role": "user", "content": "should not appear"}))
        }

        async fn execute(
            &self,
            _: Value,
            _: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new("conc_mod", "ok", None))
        }
    }

    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(Arc::new(ConcurrentModifyingTool));

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
    let outcome = dispatcher
        .dispatch("conc_mod", json!({}), ctx)
        .await
        .unwrap();

    match outcome {
        ToolDispatchOutcome::Completed {
            context_modifier_message,
            ..
        } => {
            assert!(context_modifier_message.is_none());
        }
        _ => panic!("expected Completed"),
    }
}

#[tokio::test]
async fn plain_tool_no_context_modifier_in_outcome() {
    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(Arc::new(PlainTool));

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
    let outcome = dispatcher
        .dispatch("plain_tool", json!({}), ctx)
        .await
        .unwrap();

    match outcome {
        ToolDispatchOutcome::Completed {
            context_modifier_message,
            ..
        } => {
            assert!(context_modifier_message.is_none());
        }
        _ => panic!("expected Completed"),
    }
}

fn make_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(mapping, RunId::new("run-context-mod"), "test".to_string())
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
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            Ok(LlmStepResult::ContentComplete {
                content: "done".to_string(),
                tokens_in: 0,
                tokens_out: 0,
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: 0, thinking_blocks: Vec::new(),
                stop_reason: Some("end_turn".to_string()),
            })
        } else {
            Ok(responses.remove(0))
        }
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _tool_calls: &[serde_json::Value],
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value], _thinking_blocks: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("mock-msg-id".to_string())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![]) // 显式声明此 mock 不关心 tool_defs
    }
}

#[tokio::test]
async fn driver_appends_context_modifier_message_after_tool_result() {
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(Arc::new(ModifyingTool));

    let executor = Arc::new(RecordingExecutor::new(vec![
        LlmStepResult::ToolCalls {
            assistant_content: String::new(),
            tool_calls: vec![RuntimeToolCallRequest {
                tool_call_id: "tc-mod".to_string(),
                tool_name: "modifying_tool".to_string(),
                args: json!({}),
                purpose: None,
            }],
            tokens_in: 0,
            tokens_out: 0,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0, thinking_blocks: Vec::new(),
        },
        LlmStepResult::ContentComplete {
            content: "ok".to_string(),
            tokens_in: 0,
            tokens_out: 0,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0, thinking_blocks: Vec::new(),
            stop_reason: Some("end_turn".to_string()),
        },
    ]));

    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::with_dispatcher(dispatcher),
        RuntimeEventBus::new(),
        executor.clone(),
    );

    let mut turn = make_turn("conv-context-mod");
    let request = ChatTurnRequest::new("conv-context-mod", "hello", vec![]);
    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let all_messages = executor.all_messages();
    assert!(all_messages.len() >= 2, "expected at least two llm steps");
    let second_step_messages = &all_messages[1];

    let tool_index = second_step_messages
        .iter()
        .position(|msg| {
            msg.get("role").and_then(|v| v.as_str()) == Some("tool")
                && msg.get("toolCallId").and_then(|v| v.as_str()) == Some("tc-mod")
        })
        .expect("tool result should be present");
    let modifier_index = second_step_messages
        .iter()
        .position(|msg| {
            msg.get("role").and_then(|v| v.as_str()) == Some("user")
                && msg
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .contains("<context-update>File was modified by tool.</context-update>")
        })
        .expect("context modifier message should be appended");

    assert!(
        modifier_index > tool_index,
        "context modifier should be appended after the tool result"
    );
}
