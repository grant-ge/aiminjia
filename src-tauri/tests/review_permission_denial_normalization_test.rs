use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use app_lib::runtime::tools::{
    PermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatcher, ToolError,
    ToolExecutionContext, ToolResult,
};
use app_lib::runtime::tools::permission::{PermissionDecision, PermissionReason};
use async_trait::async_trait;
use serde_json::{json, Value};

struct DenyAllPermissionPipeline;

impl PermissionPipeline for DenyAllPermissionPipeline {
    fn authorize(
        &self,
        _definition: &ToolDefinition,
        _input: &Value,
        _ctx: &ToolExecutionContext,
    ) -> PermissionDecision {
        PermissionDecision::Deny {
            message: "approval required".into(),
            reason: PermissionReason::Other("deny_all".into()),
        }
    }
}

struct RecordingTool {
    executions: Arc<AtomicUsize>,
}

#[async_trait]
impl RuntimeTool for RecordingTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("permission_probe_tool", "records whether execute ran")
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        self.executions.fetch_add(1, Ordering::SeqCst);
        Ok(ToolResult::new("permission_probe_tool", "ok", None))
    }
}

#[tokio::test]
async fn review_permission_denial_should_surface_permission_specific_error_without_running_tool() {
    let executions = Arc::new(AtomicUsize::new(0));
    let dispatcher = ToolDispatcher::new(Arc::new(DenyAllPermissionPipeline));
    dispatcher.register(Arc::new(RecordingTool {
        executions: executions.clone(),
    }));

    let ctx = ToolExecutionContext::for_test("conv-permission", "run-permission", "tool-call-1");
    let sink = ctx.event_sink.clone();

    let err = match dispatcher
        .dispatch("permission_probe_tool", json!({"cmd": "write"}), ctx)
        .await
    {
        Ok(_) => panic!("permission denial should fail the dispatch"),
        Err(err) => err,
    };

    assert!(
        matches!(err, ToolError::PermissionDenied(ref message) if message.contains("approval required")),
        "permission pipeline denials should be surfaced as ToolError::PermissionDenied, got: {err:?}"
    );
    assert_eq!(
        executions.load(Ordering::SeqCst),
        0,
        "permission-denied tools must not execute"
    );
    assert!(
        sink.snapshot().is_empty(),
        "permission rejection should not pretend the tool started running"
    );
}
