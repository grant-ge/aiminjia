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
