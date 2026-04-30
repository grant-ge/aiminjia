#[test]
fn review_execute_python_runtime_tool_no_app_handle_passthrough() {
    let source = std::fs::read_to_string("src/runtime/tools/builtin/python.rs")
        .expect("python runtime tool source must exist");

    assert!(
        !source.contains("app_handle: None"),
        "ExecutePythonRuntimeTool must not pass app_handle: None to core params"
    );
}

#[test]
fn review_load_file_params_has_python_binary_not_app_handle() {
    let source = std::fs::read_to_string("src/llm/tool_executor/file_load.rs")
        .expect("file_load source must exist");

    assert!(
        source.contains("pub python_binary: Option<PathBuf>"),
        "LoadFileParams must have python_binary field"
    );
    assert!(
        !source.contains("pub app_handle"),
        "LoadFileParams must not have app_handle field"
    );
}

#[test]
fn review_execute_python_core_params_has_python_binary_not_app_handle() {
    let source = std::fs::read_to_string("src/llm/tool_executor/python.rs")
        .expect("python tool executor source must exist");

    assert!(
        source.contains("pub python_binary: Option<std::path::PathBuf>"),
        "ExecutePythonCoreParams must have python_binary field"
    );
    assert!(
        !source.contains("pub app_handle"),
        "ExecutePythonCoreParams must not have app_handle field"
    );
}
