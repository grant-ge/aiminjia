use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use app_lib::runtime::chat::tool_round_driver::ToolRoundDriver;
use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use app_lib::runtime::chat::ToolRoundResult;
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use app_lib::runtime::tools::dispatcher::RuntimeTool;
use app_lib::runtime::tools::{
    AllowAllPermissionPipeline, ToolDefinition, ToolDispatcher, ToolError, ToolExecutionContext,
    ToolResult,
};

fn make_turn() -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id("test-conv");
    TurnState::new(mapping, RunId::new("test-run"), "test".to_string())
}

struct FailTool {
    name: String,
}

#[async_trait]
impl RuntimeTool for FailTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(&self.name, "always fails")
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        Err(ToolError::ExecutionFailed("intentional failure".to_string()))
    }
}

struct SlowTool {
    name: String,
    started: Arc<Mutex<bool>>,
    cancelled: Arc<Mutex<bool>>,
}

#[async_trait]
impl RuntimeTool for SlowTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(&self.name, "slow tool")
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(&self, _input: Value, ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        *self.started.lock().unwrap() = true;
        for _ in 0..200 {
            if ctx.cancellation.is_cancelled() {
                *self.cancelled.lock().unwrap() = true;
                return Err(ToolError::ExecutionFailed("cancelled by sibling error".to_string()));
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok(ToolResult::new(&self.name, "slow done", None))
    }
}

#[tokio::test]
async fn sibling_error_cascades_to_concurrent_tool() {
    let slow_started = Arc::new(Mutex::new(false));
    let slow_cancelled = Arc::new(Mutex::new(false));

    let fail_tool = Arc::new(FailTool {
        name: "fail_tool".to_string(),
    });
    let slow_tool = Arc::new(SlowTool {
        name: "slow_tool".to_string(),
        started: slow_started.clone(),
        cancelled: slow_cancelled.clone(),
    });

    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(fail_tool);
    dispatcher.register(slow_tool);

    let engine = QueryEngine::with_dispatcher(dispatcher);
    let driver = ToolRoundDriver::new(engine);
    let bus = RuntimeEventBus::new();
    let turn = make_turn();

    let results = driver
        .execute_round(
            &turn,
            &bus,
            vec![
                RuntimeToolCallRequest {
                    tool_call_id: "tc-fail".into(),
                    tool_name: "fail_tool".into(),
                    args: json!({}),
                    purpose: None,
                },
                RuntimeToolCallRequest {
                    tool_call_id: "tc-slow".into(),
                    tool_name: "slow_tool".into(),
                    args: json!({}),
                    purpose: None,
                },
            ],
        )
        .await;

    assert_eq!(results.len(), 2);
    assert!(matches!(&results[0], ToolRoundResult::Ok(o) if o.is_error()));
    assert!(matches!(&results[1], ToolRoundResult::Ok(o) if o.is_error()));

    if let ToolRoundResult::Ok(outcome) = &results[1] {
        let content = outcome.content();
        assert!(
            content.contains("sibling") || content.contains("cancelled") || content.contains("parallel"),
            "sibling cancel message should mention the reason; got: {}",
            content
        );
    }

    assert!(*slow_started.lock().unwrap(), "slow tool should start");
    assert!(*slow_cancelled.lock().unwrap(), "slow tool should observe sibling cancellation");
}

#[tokio::test]
async fn single_tool_no_sibling_cascade() {
    let fail_tool = Arc::new(FailTool {
        name: "fail_tool".to_string(),
    });
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(fail_tool);

    let engine = QueryEngine::with_dispatcher(dispatcher);
    let driver = ToolRoundDriver::new(engine);
    let bus = RuntimeEventBus::new();
    let turn = make_turn();

    let results = driver
        .execute_round(
            &turn,
            &bus,
            vec![RuntimeToolCallRequest {
                tool_call_id: "tc-solo".into(),
                tool_name: "fail_tool".into(),
                args: json!({}),
                purpose: None,
            }],
        )
        .await;

    assert_eq!(results.len(), 1);
    assert!(matches!(&results[0], ToolRoundResult::Ok(o) if o.is_error()));
}

#[tokio::test]
async fn concurrent_success_no_spurious_cancels() {
    struct OkTool {
        name: String,
    }

    #[async_trait]
    impl RuntimeTool for OkTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new(&self.name, "ok")
        }

        fn is_concurrency_safe(&self, _: &Value) -> bool {
            true
        }

        async fn execute(
            &self,
            _: Value,
            _: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new(&self.name, "ok", None))
        }
    }

    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(Arc::new(OkTool { name: "tool_a".into() }));
    dispatcher.register(Arc::new(OkTool { name: "tool_b".into() }));

    let engine = QueryEngine::with_dispatcher(dispatcher);
    let driver = ToolRoundDriver::new(engine);
    let bus = RuntimeEventBus::new();
    let turn = make_turn();

    let results = driver
        .execute_round(
            &turn,
            &bus,
            vec![
                RuntimeToolCallRequest {
                    tool_call_id: "tc-a".into(),
                    tool_name: "tool_a".into(),
                    args: json!({}),
                    purpose: None,
                },
                RuntimeToolCallRequest {
                    tool_call_id: "tc-b".into(),
                    tool_name: "tool_b".into(),
                    args: json!({}),
                    purpose: None,
                },
            ],
        )
        .await;

    assert_eq!(results.len(), 2);
    assert!(matches!(&results[0], ToolRoundResult::Ok(o) if !o.is_error()));
    assert!(matches!(&results[1], ToolRoundResult::Ok(o) if !o.is_error()));
}
