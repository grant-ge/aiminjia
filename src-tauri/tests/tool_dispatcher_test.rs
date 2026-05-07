use app_lib::runtime::tools::{LegacyToolAdapter, RuntimeTool, ToolExecutionContext};
use serde_json::json;

#[tokio::test]
async fn legacy_tool_adapter_executes_builtin_tool_through_runtime_contract() {
    let tool = LegacyToolAdapter::for_test("python_exec");
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tool-1");
    let result = RuntimeTool::execute(&tool, json!({"code":"print(1)"}), ctx)
        .await
        .unwrap();
    assert_eq!(result.tool_name, "python_exec");
}

// Task 2.2 tests

#[test]
fn workspace_read_tools_are_concurrency_safe() {
    use app_lib::runtime::tools::builtin::workspace::{
        ReadWorkspaceFileRuntimeTool, SearchFilesRuntimeTool,
    };

    assert!(
        ReadWorkspaceFileRuntimeTool.is_concurrency_safe(&json!({})),
        "read_workspace_file should be concurrency safe"
    );
    assert!(
        SearchFilesRuntimeTool.is_concurrency_safe(&json!({})),
        "search_files should be concurrency safe"
    );
}

#[test]
fn workspace_read_tools_are_read_only() {
    use app_lib::runtime::tools::builtin::workspace::{
        ReadWorkspaceFileRuntimeTool, SearchFilesRuntimeTool,
    };

    assert!(ReadWorkspaceFileRuntimeTool.is_read_only(&json!({})));
    assert!(SearchFilesRuntimeTool.is_read_only(&json!({})));
}

// Task 2.3 tests

#[tokio::test]
async fn dispatch_batch_returns_results_for_all_calls() {
    use app_lib::runtime::tools::{
        AllowAllPermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatchOutcome,
        ToolDispatcher, ToolError, ToolExecutionContext, ToolResult,
    };
    use async_trait::async_trait;
    use serde_json::Value;
    use std::sync::Arc;

    struct EchoTool;

    #[async_trait]
    impl RuntimeTool for EchoTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("echo", "echo")
        }

        async fn execute(
            &self,
            input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new("echo", input.to_string(), None))
        }

        fn is_concurrency_safe(&self, _input: &Value) -> bool {
            true
        }
    }

    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(Arc::new(EchoTool));

    let calls = vec![
        (
            "echo".to_string(),
            json!({"n": 1}),
            ToolExecutionContext::for_test("c", "r", "t1"),
        ),
        (
            "echo".to_string(),
            json!({"n": 2}),
            ToolExecutionContext::for_test("c", "r", "t2"),
        ),
        (
            "echo".to_string(),
            json!({"n": 3}),
            ToolExecutionContext::for_test("c", "r", "t3"),
        ),
    ];

    let results = dispatcher.dispatch_batch(calls).await;
    assert_eq!(results.len(), 3);
    for result in &results {
        assert!(matches!(result, Ok(ToolDispatchOutcome::Completed { .. })));
    }
}

#[tokio::test]
async fn dispatch_batch_serial_tool_runs_after_concurrent_batch() {
    use app_lib::runtime::tools::{
        AllowAllPermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatcher, ToolError,
        ToolExecutionContext, ToolResult,
    };
    use async_trait::async_trait;
    use serde_json::Value;
    use std::sync::{Arc, Mutex};

    let order: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    struct OrderedTool {
        name: &'static str,
        concurrent: bool,
        order: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl RuntimeTool for OrderedTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new(self.name, "ordered")
        }

        async fn execute(
            &self,
            _input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            self.order.lock().unwrap().push(self.name.to_string());
            Ok(ToolResult::new(self.name, "ok", None))
        }

        fn is_concurrency_safe(&self, _input: &Value) -> bool {
            self.concurrent
        }
    }

    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(Arc::new(OrderedTool {
        name: "read_a",
        concurrent: true,
        order: order.clone(),
    }));
    dispatcher.register(Arc::new(OrderedTool {
        name: "write_b",
        concurrent: false,
        order: order.clone(),
    }));

    let calls = vec![
        (
            "read_a".to_string(),
            json!({}),
            ToolExecutionContext::for_test("c", "r", "t1"),
        ),
        (
            "write_b".to_string(),
            json!({}),
            ToolExecutionContext::for_test("c", "r", "t2"),
        ),
    ];

    let results = dispatcher.dispatch_batch(calls).await;
    assert_eq!(results.len(), 2);

    let execution_order = order.lock().unwrap();
    assert_eq!(execution_order[0], "read_a");
    assert_eq!(execution_order[1], "write_b");
}

#[tokio::test]
async fn dispatch_completed_outcome_carries_declared_max_result_size_chars() {
    use app_lib::runtime::tools::{
        AllowAllPermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatchOutcome,
        ToolDispatcher, ToolError, ToolExecutionContext, ToolResult,
    };
    use async_trait::async_trait;
    use serde_json::Value;
    use std::sync::Arc;

    struct EchoTool;

    #[async_trait]
    impl RuntimeTool for EchoTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("echo", "echo").with_max_result_size_chars(12_345)
        }

        async fn execute(
            &self,
            _input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new("echo", "ok", None))
        }
    }

    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(Arc::new(EchoTool));

    let ctx = ToolExecutionContext::for_test("conv", "run", "tc");
    let result = dispatcher.dispatch("echo", json!({}), ctx).await;

    match result.expect("dispatch ok") {
        ToolDispatchOutcome::Completed {
            max_result_size_chars,
            ..
        } => {
            assert_eq!(max_result_size_chars, 12_345);
        }
        _ => panic!("expected Completed outcome"),
    }
}
