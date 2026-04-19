use app_lib::runtime::tools::definition::{ToolDefinition, ToolKind};

#[test]
fn tool_definition_has_kind_field() {
    let def = ToolDefinition::new("web_search", "Search the web").with_kind(ToolKind::Primitive);
    assert!(matches!(def.kind, ToolKind::Primitive));
}

#[test]
fn tool_kind_default_is_primitive() {
    let def = ToolDefinition::new("echo", "Echo test");
    assert!(matches!(def.kind, ToolKind::Primitive));
}

#[test]
fn execute_python_kind_is_power() {
    let def = ToolDefinition::new("execute_python", "Run Python").with_kind(ToolKind::Power);
    assert!(matches!(def.kind, ToolKind::Power));
}

#[test]
fn browse_data_kind_is_composite() {
    let def = ToolDefinition::new("browse_data", "Multi-step browser agent")
        .with_kind(ToolKind::Composite);
    assert!(matches!(def.kind, ToolKind::Composite));
}

#[test]
fn all_new_plan_c_tools_are_in_catalog() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    for id in &["write_file", "edit_file", "bash", "grep_content"] {
        assert!(
            TOOL_CATALOG.get(id).is_some(),
            "Tool '{id}' should be registered in TOOL_CATALOG"
        );
    }
}

#[tokio::test]
async fn get_all_schemas_returns_builtin_then_mcp_partitions() {
    use app_lib::plugin::registry::ToolRegistry;
    let registry = ToolRegistry::new();
    let schemas = registry.get_all_schemas().await;
    let names: Vec<_> = schemas.iter().map(|s| s.name.clone()).collect();
    let builtin: Vec<_> = names
        .iter()
        .filter(|name| !name.starts_with("mcp__"))
        .cloned()
        .collect();
    let mcp: Vec<_> = names
        .iter()
        .filter(|name| name.starts_with("mcp__"))
        .cloned()
        .collect();
    let mut builtin_sorted = builtin.clone();
    builtin_sorted.sort();
    let mut mcp_sorted = mcp.clone();
    mcp_sorted.sort();
    assert_eq!(builtin, builtin_sorted, "built-in partition must be sorted");
    assert_eq!(mcp, mcp_sorted, "MCP partition must be sorted");
}

#[tokio::test]
async fn get_schemas_filtered_returns_sorted_by_name() {
    use app_lib::plugin::registry::ToolRegistry;
    use app_lib::plugin::skill_trait::ToolFilter;
    let registry = ToolRegistry::new();
    let schemas = registry
        .get_schemas_filtered(&ToolFilter::Only(vec![
            "web_search".to_string(),
            "browse_navigate".to_string(),
            "list_directory".to_string(),
        ]))
        .await;
    let names: Vec<_> = schemas.iter().map(|s| s.name.clone()).collect();
    assert_eq!(
        names,
        vec!["browse_navigate".to_string(), "web_search".to_string()],
        "get_schemas_filtered must return the expected filtered tool set"
    );
    let builtin: Vec<_> = names
        .iter()
        .filter(|name| !name.starts_with("mcp__"))
        .cloned()
        .collect();
    let mcp: Vec<_> = names
        .iter()
        .filter(|name| name.starts_with("mcp__"))
        .cloned()
        .collect();
    let mut builtin_sorted = builtin.clone();
    builtin_sorted.sort();
    let mut mcp_sorted = mcp.clone();
    mcp_sorted.sort();
    assert_eq!(builtin, builtin_sorted, "filtered built-in partition must be sorted");
    assert_eq!(mcp, mcp_sorted, "filtered MCP partition must be sorted");
}

// Task 2.1 tests

#[test]
fn tool_definition_default_read_only_is_false() {
    let def = ToolDefinition::new("test_tool", "desc");
    assert!(!def.default_read_only);
    assert!(!def.default_destructive);
}

#[test]
fn tool_definition_with_read_only_flag() {
    let def = ToolDefinition::new("read_tool", "desc").with_read_only(true);
    assert!(def.default_read_only);
}

#[test]
fn tool_definition_with_destructive_flag() {
    let def = ToolDefinition::new("write_tool", "desc").with_destructive(true);
    assert!(def.default_destructive);
}

#[test]
fn runtime_tool_default_predicates_follow_definition_flags() {
    use app_lib::runtime::tools::{RuntimeTool, ToolError, ToolExecutionContext, ToolResult};
    use async_trait::async_trait;
    use serde_json::{json, Value};

    struct PredicateTool(ToolDefinition);

    #[async_trait]
    impl RuntimeTool for PredicateTool {
        fn definition(&self) -> ToolDefinition {
            self.0.clone()
        }

        async fn execute(
            &self,
            _input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new(self.0.id.clone(), "ok", None))
        }
    }

    let default_tool = PredicateTool(ToolDefinition::new("default_tool", "desc"));
    assert!(!default_tool.is_concurrency_safe(&json!({})));
    assert!(!default_tool.is_read_only(&json!({})));
    assert!(!default_tool.is_destructive(&json!({})));

    let flagged_tool = PredicateTool(
        ToolDefinition::new("flagged_tool", "desc")
            .with_read_only(true)
            .with_destructive(true),
    );
    assert!(flagged_tool.is_read_only(&json!({})));
    assert!(flagged_tool.is_destructive(&json!({})));
}

// ── Plan-D1: ToolDefinition.default_max_result_size_chars ─────────────────

#[test]
fn tool_definition_default_max_result_size_chars_is_8000() {
    let def = ToolDefinition::new("some_tool", "desc");
    assert_eq!(def.default_max_result_size_chars, 8_000);
}

#[test]
fn tool_definition_with_max_result_size_chars_sets_field() {
    let def = ToolDefinition::new("execute_python", "desc").with_max_result_size_chars(32_000);
    assert_eq!(def.default_max_result_size_chars, 32_000);
}

#[test]
fn catalog_execute_python_has_32000_limit() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;
    let def = TOOL_CATALOG.get("execute_python").unwrap();
    assert_eq!(def.default_max_result_size_chars, 32_000);
}

#[test]
fn catalog_read_workspace_file_has_16000_limit() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;
    let def = TOOL_CATALOG.get("read_workspace_file").unwrap();
    assert_eq!(def.default_max_result_size_chars, 16_000);
}

#[test]
fn catalog_list_directory_has_4000_limit() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;
    let def = TOOL_CATALOG.get("list_directory").unwrap();
    assert_eq!(def.default_max_result_size_chars, 4_000);
}

#[test]
fn catalog_search_files_has_4000_limit() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;
    let def = TOOL_CATALOG.get("search_files").unwrap();
    assert_eq!(def.default_max_result_size_chars, 4_000);
}

#[test]
fn catalog_other_tools_default_to_8000_when_not_overridden() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    for id in [
        "web_search",
        "plan_update",
        "progress_update",
        "save_analysis_note",
    ] {
        let def = TOOL_CATALOG.get(id).unwrap();
        assert_eq!(
            def.default_max_result_size_chars, 8_000,
            "{} should default to 8000",
            id
        );
    }
}
