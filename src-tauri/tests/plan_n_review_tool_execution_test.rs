use app_lib::runtime::cancellation::{CancellationReason, CancellationToken};
use app_lib::runtime::chat::tool_round_driver::ToolRoundDriver;
use app_lib::runtime::tools::dispatcher::{InterruptBehavior, RuntimeTool};
use app_lib::runtime::tools::{ToolDefinition, ToolError, ToolExecutionContext, ToolResult};

#[test]
fn review_default_interrupt_behavior_is_block() {
    struct MinimalTool;

    #[async_trait::async_trait]
    impl RuntimeTool for MinimalTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("minimal", "minimal tool")
        }

        async fn execute(
            &self,
            _: serde_json::Value,
            _: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new("minimal", "ok", None))
        }
    }

    let tool = MinimalTool;
    assert!(matches!(
        tool.interrupt_behavior(),
        InterruptBehavior::Block
    ));
}

#[test]
fn review_default_context_modifier_is_none() {
    struct MinimalTool;

    #[async_trait::async_trait]
    impl RuntimeTool for MinimalTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("minimal2", "minimal tool 2")
        }

        async fn execute(
            &self,
            _: serde_json::Value,
            _: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new("minimal2", "ok", None))
        }
    }

    let tool = MinimalTool;
    assert!(tool.context_modifier().is_none());
}

#[test]
fn review_sibling_error_does_not_cancel_turn() {
    let turn_cancel = CancellationToken::new();
    let sibling_cancel = turn_cancel.child_token();

    sibling_cancel.cancel_with_reason(CancellationReason::SiblingError);

    assert!(!turn_cancel.is_cancelled());
    assert_eq!(sibling_cancel.reason(), Some(CancellationReason::SiblingError));
}

#[test]
fn review_tool_round_driver_no_tauri() {
    let _ = std::mem::size_of::<ToolRoundDriver>();
}
