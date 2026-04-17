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
        GetFileInfoRuntimeTool, ListDirectoryRuntimeTool, ReadWorkspaceFileRuntimeTool,
        SearchFilesRuntimeTool,
    };

    assert!(
        ListDirectoryRuntimeTool.is_concurrency_safe(&json!({})),
        "list_directory should be concurrency safe"
    );
    assert!(
        ReadWorkspaceFileRuntimeTool.is_concurrency_safe(&json!({})),
        "read_workspace_file should be concurrency safe"
    );
    assert!(
        SearchFilesRuntimeTool.is_concurrency_safe(&json!({})),
        "search_files should be concurrency safe"
    );
    assert!(
        GetFileInfoRuntimeTool.is_concurrency_safe(&json!({})),
        "get_file_info should be concurrency safe"
    );
}

#[test]
fn workspace_read_tools_are_read_only() {
    use app_lib::runtime::tools::builtin::workspace::{
        GetFileInfoRuntimeTool, ListDirectoryRuntimeTool, ReadWorkspaceFileRuntimeTool,
        SearchFilesRuntimeTool,
    };

    assert!(ListDirectoryRuntimeTool.is_read_only(&json!({})));
    assert!(ReadWorkspaceFileRuntimeTool.is_read_only(&json!({})));
    assert!(SearchFilesRuntimeTool.is_read_only(&json!({})));
    assert!(GetFileInfoRuntimeTool.is_read_only(&json!({})));
}
