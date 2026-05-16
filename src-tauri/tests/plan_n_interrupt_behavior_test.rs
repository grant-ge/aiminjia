use async_trait::async_trait;
use serde_json::Value;

use app_lib::runtime::tools::dispatcher::{InterruptBehavior, RuntimeTool};
use app_lib::runtime::tools::description_context::ToolDescriptionContext;
use app_lib::runtime::tools::{ToolDefinition, ToolError, ToolExecutionContext, ToolResult};

struct CancelTool;
struct BlockTool;

#[async_trait]
impl RuntimeTool for CancelTool {
    fn id(&self) -> &str {
        "cancel_tool"
    }

    async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
        ToolDefinition::new("cancel_tool", "cancellable")
    }

    fn interrupt_behavior(&self) -> InterruptBehavior {
        InterruptBehavior::Cancel
    }

    async fn execute(&self, _: Value, _: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new("cancel_tool", "ok", None))
    }
}

#[async_trait]
impl RuntimeTool for BlockTool {
    fn id(&self) -> &str {
        "block_tool"
    }

    async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
        ToolDefinition::new("block_tool", "blocking")
    }

    async fn execute(&self, _: Value, _: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new("block_tool", "ok", None))
    }
}

#[test]
fn default_interrupt_behavior_is_block() {
    let tool = BlockTool;
    assert!(matches!(
        tool.interrupt_behavior(),
        InterruptBehavior::Block
    ));
}

#[test]
fn cancel_tool_declares_cancel() {
    let tool = CancelTool;
    assert!(matches!(
        tool.interrupt_behavior(),
        InterruptBehavior::Cancel
    ));
}

#[test]
fn interrupt_behavior_debug_format() {
    let cancel = InterruptBehavior::Cancel;
    let block = InterruptBehavior::Block;
    assert!(format!("{:?}", cancel).contains("Cancel"));
    assert!(format!("{:?}", block).contains("Block"));
}

#[tokio::test]
async fn interrupt_only_cancels_cancel_behavior_tools() {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use app_lib::runtime::cancellation::CancellationReason;
    use app_lib::runtime::chat::tool_round_driver::ToolRoundDriver;
    use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
    use app_lib::runtime::event_bus::RuntimeEventBus;
    use app_lib::runtime::identity::IdentityMapping;
    use app_lib::runtime::ids::RunId;
    use app_lib::runtime::query_engine::QueryEngine;
    use app_lib::runtime::state::TurnState;
    use app_lib::runtime::tools::{AllowAllPermissionPipeline, ToolDispatcher};
    use serde_json::json;

    struct CancelAwareTool {
        cancelled: Arc<Mutex<bool>>,
    }

    #[async_trait]
    impl RuntimeTool for CancelAwareTool {
        fn id(&self) -> &str {
            "cancel_aware"
        }

        async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
            ToolDefinition::new("cancel_aware", "cancel")
        }

        fn is_concurrency_safe(&self, _: &Value) -> bool {
            true
        }

        fn interrupt_behavior(&self) -> InterruptBehavior {
            InterruptBehavior::Cancel
        }

        async fn execute(
            &self,
            _: Value,
            ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            for _ in 0..100 {
                if ctx.cancellation.is_cancelled() {
                    *self.cancelled.lock().unwrap() = true;
                    return Err(ToolError::ExecutionFailed("interrupted".to_string()));
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Ok(ToolResult::new("cancel_aware", "done", None))
        }
    }

    struct BlockAwareTool {
        completed: Arc<Mutex<bool>>,
    }

    #[async_trait]
    impl RuntimeTool for BlockAwareTool {
        fn id(&self) -> &str {
            "block_aware"
        }

        async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
            ToolDefinition::new("block_aware", "block")
        }

        fn is_concurrency_safe(&self, _: &Value) -> bool {
            true
        }

        fn interrupt_behavior(&self) -> InterruptBehavior {
            InterruptBehavior::Block
        }

        async fn execute(
            &self,
            _: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            tokio::time::sleep(Duration::from_millis(50)).await;
            *self.completed.lock().unwrap() = true;
            Ok(ToolResult::new("block_aware", "completed", None))
        }
    }

    let cancel_flag = Arc::new(Mutex::new(false));
    let block_flag = Arc::new(Mutex::new(false));

    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(Arc::new(CancelAwareTool {
        cancelled: cancel_flag.clone(),
    }));
    dispatcher.register(Arc::new(BlockAwareTool {
        completed: block_flag.clone(),
    }));

    let engine = QueryEngine::with_dispatcher(dispatcher);
    let driver = ToolRoundDriver::new(engine);
    let bus = RuntimeEventBus::new();

    let mapping = IdentityMapping::from_legacy_conversation_id("test-interrupt");
    let turn = TurnState::new(mapping, RunId::new("r1"), "test".to_string());

    let cancel_token = turn.cancellation();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        cancel_token.cancel_with_reason(CancellationReason::Interrupt);
    });

    let results = driver
        .execute_round(
            &turn,
            &bus,
            vec![
                RuntimeToolCallRequest {
                    tool_call_id: "tc-cancel".into(),
                    tool_name: "cancel_aware".into(),
                    args: json!({}),
                    purpose: None,
                },
                RuntimeToolCallRequest {
                    tool_call_id: "tc-block".into(),
                    tool_name: "block_aware".into(),
                    args: json!({}),
                    purpose: None,
                },
            ],
        )
        .await;

    assert_eq!(results.len(), 2);
    assert!(
        *cancel_flag.lock().unwrap(),
        "Cancel tool should have been interrupted"
    );
    assert!(
        *block_flag.lock().unwrap(),
        "Block tool should have completed despite interrupt"
    );
}
