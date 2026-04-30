use app_lib::plugin::builtin::tools::echo_runtime::EchoRuntimeTool;
use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
use serde_json::json;

#[tokio::test]
async fn builtin_tool_can_migrate_off_legacy_trait_incrementally() {
    let tool = EchoRuntimeTool;
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tool-1");
    let result = RuntimeTool::execute(&tool, json!({"text":"hi"}), ctx)
        .await
        .unwrap();
    assert_eq!(result.output_text(), "hi");
}
