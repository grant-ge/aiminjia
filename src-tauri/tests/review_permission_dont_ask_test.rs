use std::sync::Arc;

use app_lib::runtime::tools::permission::{
    default_permission_ask, AllowAllPermissionPipeline, PermissionDecision, PermissionMode,
    PermissionPipeline,
};
use app_lib::runtime::tools::{
    RuntimeTool, ToolDefinition, ToolDispatcher, ToolError, ToolExecutionContext, ToolResult,
};
use async_trait::async_trait;
use serde_json::{json, Value};

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
            reason: app_lib::runtime::tools::permission::PermissionReason::UnknownScope,
        }
    }
}

struct EchoTool;

#[async_trait]
impl RuntimeTool for EchoTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("echo_tool", "echo tool")
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new("echo_tool", "ok", None))
    }
}

struct ExecuteAskTool;

#[async_trait]
impl RuntimeTool for ExecuteAskTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("execute_ask_tool", "execute ask tool")
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        Err(ToolError::AskRequired(PermissionDecision::Ask {
            message: "execute path approval required".to_string(),
            suggestions: vec!["Allow once".to_string(), "Deny".to_string()],
            remember_options: default_permission_ask().0,
            default_destination: default_permission_ask().1,
            reason: app_lib::runtime::tools::permission::PermissionReason::Other(
                "execute_ask".into(),
            ),
        }))
    }
}

#[test]
fn review_default_mode_preserves_ask() {
    let pipeline = AlwaysAskPermissionPipeline;
    let def = ToolDefinition::new("echo_tool", "echo tool");
    let ctx = ToolExecutionContext::for_test("conv-default", "run-default", "tc-default");

    let decision = pipeline.authorize(&def, &json!({}), &ctx);

    assert!(matches!(decision, PermissionDecision::Ask { .. }));
}

#[test]
fn review_plan_mode_converts_ask_to_deny() {
    let decision = app_lib::runtime::tools::permission::apply_permission_mode(
        PermissionDecision::Ask {
            message: "permission confirmation required".to_string(),
            suggestions: vec!["Allow once".to_string()],
            remember_options: default_permission_ask().0,
            default_destination: default_permission_ask().1,
            reason: app_lib::runtime::tools::permission::PermissionReason::UnknownScope,
        },
        "echo_tool",
        PermissionMode::Plan,
    );

    match decision {
        PermissionDecision::Deny { reason, .. } => {
            assert!(matches!(
                reason,
                app_lib::runtime::tools::permission::PermissionReason::Mode(ref mode)
                    if mode == "plan"
            ));
        }
        other => panic!("plan mode should convert ask to deny, got: {:?}", other),
    }
}

#[tokio::test]
async fn review_dont_ask_mode_converts_ask_to_deny_at_dispatch_boundary() {
    let dispatcher = ToolDispatcher::new(Arc::new(AlwaysAskPermissionPipeline));
    dispatcher.register(Arc::new(EchoTool));

    let ctx = ToolExecutionContext::for_test("conv-dont-ask", "run-dont-ask", "tc-dont-ask")
        .with_permission_mode(PermissionMode::DontAsk);
    let result = dispatcher.dispatch("echo_tool", json!({}), ctx).await;

    assert!(
        matches!(result, Err(ToolError::PermissionDenied(_))),
        "dontAsk mode should convert AskRequired into PermissionDenied at the dispatcher boundary"
    );
}

#[tokio::test]
async fn review_dont_ask_mode_does_not_block_stored_allow() {
    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(Arc::new(EchoTool));

    let ctx = ToolExecutionContext::for_test("conv-allow", "run-allow", "tc-allow")
        .with_permission_mode(PermissionMode::DontAsk);
    let result = dispatcher.dispatch("echo_tool", json!({}), ctx).await;

    assert!(matches!(
        result,
        Ok(app_lib::runtime::tools::ToolDispatchOutcome::Completed { .. })
    ));
}

#[tokio::test]
async fn review_dont_ask_mode_converts_execute_path_ask_to_deny() {
    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(Arc::new(ExecuteAskTool));

    let ctx = ToolExecutionContext::for_test(
        "conv-execute-dont-ask",
        "run-execute-dont-ask",
        "tc-execute-dont-ask",
    )
    .with_permission_mode(PermissionMode::DontAsk);
    let result = dispatcher
        .dispatch("execute_ask_tool", json!({}), ctx)
        .await;

    assert!(
        matches!(result, Err(ToolError::PermissionDenied(_))),
        "dontAsk mode should also convert execute-path AskRequired into PermissionDenied"
    );
}
