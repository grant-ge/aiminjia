use std::sync::Arc;

use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use app_lib::runtime::tools::description_context::ToolDescriptionContext;
use app_lib::runtime::tools::{
    AllowAllPermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatcher, ToolError,
    ToolExecutionContext, ToolResult,
};
use app_lib::transport::tauri_event_adapter::TauriEventAdapter;
use app_lib::transport::testing::RecordingRuntimeHost;
use async_trait::async_trait;
use serde_json::{json, Value};

struct FailingRuntimeTool;

#[async_trait]
impl RuntimeTool for FailingRuntimeTool {
    fn id(&self) -> &str {
        "failing_runtime_tool"
    }

    async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
        ToolDefinition::new("failing_runtime_tool", "always fails for review")
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        Err(ToolError::ExecutionFailed("boom".to_string()))
    }
}

#[tokio::test]
async fn review_failing_tool_should_still_emit_terminal_completed_event() {
    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(Arc::new(FailingRuntimeTool));

    let ctx = ToolExecutionContext::for_test("conv-tool-error", "run-tool-error", "tool-call-1");
    let sink = ctx.event_sink.clone();

    let result = dispatcher
        .dispatch("failing_runtime_tool", json!({"input": "x"}), ctx)
        .await;
    assert!(result.is_err(), "tool should fail in review scenario");

    let events = sink.snapshot();
    assert_eq!(
        events,
        vec!["tool:executing".to_string(), "tool:completed".to_string()],
        "tool error path must still emit a terminal completion event so UI/runtime can clear in-flight state"
    );
}

#[tokio::test]
async fn review_runtime_tool_failure_maps_tool_completed_success_false() {
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(Arc::new(FailingRuntimeTool));

    let engine = QueryEngine::with_dispatcher(dispatcher);
    let host = RecordingRuntimeHost::new();
    let bus = RuntimeEventBus::new();
    let _adapter = Arc::new(TauriEventAdapter::new(host.clone()));
    bus.subscribe(_adapter.clone());

    let mapping = IdentityMapping::from_legacy_conversation_id("conv-tool-error".to_string());
    let turn = TurnState::new(mapping, RunId::new("run-tool-error"), "fail".to_string());

    let outcome = engine
        .run_tool_call_with_bus(
            &turn,
            &bus,
            RuntimeToolCallRequest {
                tool_call_id: "tool-call-1".to_string(),
                tool_name: "failing_runtime_tool".to_string(),
                args: json!({"input": "x"}),
                purpose: Some("review error payload".to_string()),
            },
        )
        .await
        .expect("runtime tool round should convert tool failure into typed outcome");

    assert!(
        outcome.is_error(),
        "failing tool must surface is_error=true"
    );

    let trace = host.trace();
    let completed = trace
        .events
        .iter()
        .find(|event| event.name == "tool:completed")
        .expect("runtime bus should emit tool:completed to host");

    assert_eq!(
        completed.payload.get("success").and_then(|v| v.as_bool()),
        Some(false),
        "tool:completed.success must be false when runtime tool execution fails; payload={}",
        completed.payload
    );
}
