use app_lib::commands::chat::testsupport::run_workspace_tool_with_authorized_session;
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn authorized_session_exposes_workspace_tools_and_lists_local_directory() {
    let authorized_root = TempDir::new().unwrap();
    std::fs::write(authorized_root.path().join("sales_2026.csv"), b"month,revenue\n1,100\n")
        .unwrap();
    std::fs::write(authorized_root.path().join("notes.txt"), b"hello workspace").unwrap();

    let trace = run_workspace_tool_with_authorized_session(
        "conv-workspace",
        authorized_root.path(),
        "list_directory",
        json!({ "path": "." }),
    )
    .await
    .unwrap();

    for tool_name in &[
        "list_directory",
        "read_workspace_file",
        "search_files",
        "get_file_info",
    ] {
        assert!(
            trace.visible_tool_names.iter().any(|name| name == tool_name),
            "authorized session should expose workspace tool '{}', got {:?}",
            tool_name,
            trace.visible_tool_names
        );
    }

    assert!(
        trace.tool_output.contains("sales_2026.csv"),
        "agent tool path should read authorized directory contents: {}",
        trace.tool_output
    );
    assert!(
        trace.tool_output.contains("notes.txt"),
        "authorized directory listing should include all external files: {}",
        trace.tool_output
    );
}

#[tokio::test]
async fn authorized_session_reads_workspace_file_without_upload_flow() {
    let authorized_root = TempDir::new().unwrap();
    std::fs::write(
        authorized_root.path().join("sales_2026.csv"),
        b"month,revenue\n1,100\n2,120\n",
    )
    .unwrap();

    let trace = run_workspace_tool_with_authorized_session(
        "conv-workspace-read",
        authorized_root.path(),
        "read_workspace_file",
        json!({ "path": "sales_2026.csv" }),
    )
    .await
    .unwrap();

    assert!(
        trace.visible_tool_names.iter().any(|name| name == "read_workspace_file"),
        "authorized session should expose read_workspace_file, got {:?}",
        trace.visible_tool_names
    );
    assert!(
        trace.tool_output.contains("month,revenue"),
        "read_workspace_file should return local file content without upload: {}",
        trace.tool_output
    );
    assert!(
        trace.tool_output.contains("2,120"),
        "full file content should be accessible from authorized directory: {}",
        trace.tool_output
    );
}
