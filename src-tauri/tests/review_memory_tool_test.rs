use app_lib::runtime::tools::builtin::memory::{
    MemoryDeps, SearchMemoryRuntimeTool, WriteMemoryRuntimeTool,
};
use app_lib::runtime::tools::catalog::TOOL_CATALOG;
use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
use serde_json::{json, Value};
use tempfile::TempDir;

fn deps(dir: &std::path::Path) -> MemoryDeps {
    let workspace = dir.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    MemoryDeps {
        app_data_dir: dir.to_path_buf(),
        workspace_path: workspace,
    }
}

fn ctx() -> ToolExecutionContext {
    ToolExecutionContext::for_test("conv-memory-tool", "run-memory-tool", "tc-memory-tool")
}

fn parse_result(result: &app_lib::runtime::tools::ToolResult) -> Value {
    result.data.clone().unwrap_or_else(|| {
        serde_json::from_str(&result.content).expect("tool content should be json")
    })
}

fn entries_dir(dir: &std::path::Path) -> std::path::PathBuf {
    let service =
        app_lib::runtime::project_memory::ProjectMemoryService::new(dir, &dir.join("workspace"));
    service.memory_root().join("entries")
}

#[tokio::test]
async fn write_memory_saves_entry_and_returns_structured_saved_result() {
    let dir = TempDir::new().unwrap();
    let tool = WriteMemoryRuntimeTool::new(deps(dir.path()));

    let result = tool
        .execute(
            json!({
                "name": "user-prefers-boxplot",
                "memory_type": "user_preference",
                "description": "用户偏好用箱型图展示薪资分布",
                "content": "用户明确表示喜欢用箱型图展示薪资分布。"
            }),
            ctx(),
        )
        .await
        .unwrap();

    let parsed = parse_result(&result);
    assert_eq!(parsed["status"], "saved");
    assert_eq!(parsed["name"], "user-prefers-boxplot");
    let path = parsed["path"].as_str().expect("path should be string");
    assert!(path.ends_with(".md"));
    assert!(!path.starts_with('/'), "path should be relative: {path}");
    assert!(entries_dir(dir.path())
        .join(path.strip_prefix("entries/").unwrap_or(path))
        .exists());
}

#[tokio::test]
async fn write_memory_invalid_memory_type_returns_clear_error_without_writing_files() {
    let dir = TempDir::new().unwrap();
    let tool = WriteMemoryRuntimeTool::new(deps(dir.path()));

    let err = tool
        .execute(
            json!({
                "name": "bad-type",
                "memory_type": "custom",
                "description": "desc",
                "content": "content"
            }),
            ctx(),
        )
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("unknown memory_type"));
    for valid in [
        "user_preference",
        "project_constraint",
        "reference_info",
        "feedback",
    ] {
        assert!(err.contains(valid), "error should list {valid}: {err}");
    }
    assert!(
        !entries_dir(dir.path()).exists(),
        "invalid type must not write entry files"
    );
}

#[tokio::test]
async fn write_memory_missing_required_fields_return_clear_errors_without_writing_files() {
    let dir = TempDir::new().unwrap();
    let tool = WriteMemoryRuntimeTool::new(deps(dir.path()));

    let missing_name = tool
        .execute(
            json!({
                "memory_type": "user_preference",
                "description": "desc",
                "content": "content"
            }),
            ctx(),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(missing_name.contains("missing 'name'"));

    let missing_content = tool
        .execute(
            json!({
                "name": "missing-content",
                "memory_type": "user_preference",
                "description": "desc"
            }),
            ctx(),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(missing_content.contains("missing 'content'"));

    assert!(
        !entries_dir(dir.path()).exists(),
        "missing fields must not write entry files"
    );
}

#[tokio::test]
async fn search_memory_recalls_relevant_entry_as_structured_results() {
    let dir = TempDir::new().unwrap();
    let write = WriteMemoryRuntimeTool::new(deps(dir.path()));
    write
        .execute(
            json!({
                "name": "boxplot-salary-preference",
                "memory_type": "user_preference",
                "description": "boxplot salary distribution preference",
                "content": "Use boxplot when analyzing salary distribution."
            }),
            ctx(),
        )
        .await
        .unwrap();
    write
        .execute(
            json!({
                "name": "release-freeze",
                "memory_type": "project_constraint",
                "description": "mobile release freeze",
                "content": "Avoid non-critical mobile release changes."
            }),
            ctx(),
        )
        .await
        .unwrap();

    let search = SearchMemoryRuntimeTool::new(deps(dir.path()));
    let result = search
        .execute(json!({ "query": "boxplot salary" }), ctx())
        .await
        .unwrap();

    let parsed = parse_result(&result);
    assert_eq!(parsed["status"], "ok");
    assert_eq!(parsed["count"], 1);
    assert_eq!(parsed["results"][0]["name"], "boxplot-salary-preference");
    assert_eq!(parsed["results"][0]["type"], "user_preference");
    assert!(parsed["results"][0]["content"]
        .as_str()
        .unwrap()
        .contains("salary distribution"));
    assert!(!parsed.to_string().contains("release-freeze"));
}

#[tokio::test]
async fn search_memory_returns_empty_ok_result_when_nothing_matches() {
    let dir = TempDir::new().unwrap();
    let write = WriteMemoryRuntimeTool::new(deps(dir.path()));
    write
        .execute(
            json!({
                "name": "boxplot-salary-preference",
                "memory_type": "user_preference",
                "description": "boxplot salary distribution preference",
                "content": "Use boxplot when analyzing salary distribution."
            }),
            ctx(),
        )
        .await
        .unwrap();

    let search = SearchMemoryRuntimeTool::new(deps(dir.path()));
    let result = search
        .execute(json!({ "query": "totally unrelated query" }), ctx())
        .await
        .unwrap();

    let parsed = parse_result(&result);
    assert_eq!(parsed["status"], "ok");
    assert_eq!(parsed["count"], 0);
    assert_eq!(parsed["results"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn write_and_search_memory_share_same_workspace_bucket() {
    let dir = TempDir::new().unwrap();
    let write = WriteMemoryRuntimeTool::new(deps(dir.path()));
    let search = SearchMemoryRuntimeTool::new(deps(dir.path()));

    write
        .execute(
            json!({
                "name": "shared-bucket-memory",
                "memory_type": "reference_info",
                "description": "shared bucket keyword",
                "content": "shared bucket content"
            }),
            ctx(),
        )
        .await
        .unwrap();

    let result = search
        .execute(json!({ "query": "shared bucket" }), ctx())
        .await
        .unwrap();
    let parsed = parse_result(&result);
    assert_eq!(parsed["count"], 1);
    assert_eq!(parsed["results"][0]["name"], "shared-bucket-memory");
}

#[test]
fn write_memory_is_write_operation_and_search_memory_is_read_only() {
    let dir = TempDir::new().unwrap();
    let write = WriteMemoryRuntimeTool::new(deps(dir.path()));
    let search = SearchMemoryRuntimeTool::new(deps(dir.path()));

    assert!(!write.is_read_only(&json!({})));
    assert!(search.is_read_only(&json!({})));
}

#[test]
fn memory_tool_definition_names_match_tool_catalog_registration() {
    let dir = TempDir::new().unwrap();
    let write = WriteMemoryRuntimeTool::new(deps(dir.path()));
    let search = SearchMemoryRuntimeTool::new(deps(dir.path()));

    assert_eq!(write.definition().id, "write_memory");
    assert_eq!(search.definition().id, "search_memory");
    assert!(TOOL_CATALOG.get("write_memory").is_some());
    assert!(TOOL_CATALOG.get("search_memory").is_some());
}
