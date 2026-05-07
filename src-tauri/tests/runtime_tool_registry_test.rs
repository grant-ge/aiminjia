use app_lib::runtime::tools::builtin::workspace::{
    GetFileInfoRuntimeTool, ReadWorkspaceFileRuntimeTool,
    SearchFilesRuntimeTool,
};
use app_lib::runtime::tools::capability::CapabilityContext;
use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

fn make_ctx_with_workspace(tmp: &TempDir) -> ToolExecutionContext {
    let cap = CapabilityContext::with_workspace(tmp.path().to_path_buf(), "test-ws");
    ToolExecutionContext::for_test("conv-1", "run-1", "tc-1").with_capability(Arc::new(cap))
}

#[tokio::test]
async fn read_workspace_file_runtime_tool_reads_content() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("hello.txt"), b"hello world").unwrap();
    let ctx = make_ctx_with_workspace(&tmp);
    let tool = ReadWorkspaceFileRuntimeTool;
    let result = RuntimeTool::execute(&tool, json!({"path": "hello.txt"}), ctx)
        .await
        .unwrap();
    assert!(
        result.content.contains("hello world"),
        "Should contain file content"
    );
}

#[tokio::test]
async fn search_files_runtime_tool_finds_csv() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("a.csv"), b"").unwrap();
    std::fs::write(tmp.path().join("b.txt"), b"").unwrap();
    let ctx = make_ctx_with_workspace(&tmp);
    let tool = SearchFilesRuntimeTool;
    let result = RuntimeTool::execute(&tool, json!({"pattern": "*.csv"}), ctx)
        .await
        .unwrap();
    assert!(result.content.contains("a.csv"), "Should find a.csv");
    assert!(!result.content.contains("b.txt"), "Should not find b.txt");
}

#[tokio::test]
async fn workspace_runtime_tools_have_correct_kind() {
    use app_lib::runtime::tools::definition::ToolKind;
    let tools: Vec<Box<dyn RuntimeTool>> = vec![
        Box::new(ReadWorkspaceFileRuntimeTool),
        Box::new(SearchFilesRuntimeTool),
        Box::new(GetFileInfoRuntimeTool),
    ];
    for tool in &tools {
        let def = tool.definition();
        assert!(
            matches!(def.kind, ToolKind::Primitive),
            "Tool '{}' should be Primitive kind",
            def.id
        );
    }
}
