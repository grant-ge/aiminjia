use std::sync::Arc;

use app_lib::runtime::tools::{
    RuntimeTool, ToolDefinition, ToolDispatcher, ToolError, ToolExecutionContext, ToolResult,
};
use async_trait::async_trait;
use serde_json::{json, Value};

struct FailingRuntimeTool;

#[async_trait]
impl RuntimeTool for FailingRuntimeTool {
    fn definition(&self) -> ToolDefinition {
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
    let dispatcher = ToolDispatcher::allow_all();
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
