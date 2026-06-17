use app_lib::runtime::tools::definition::{ToolDefinition, ToolKind};

#[test]
fn tool_definition_has_kind_field() {
    let def = ToolDefinition::new("WebSearch", "Search the web").with_kind(ToolKind::Primitive);
    assert!(matches!(def.kind, ToolKind::Primitive));
}

#[test]
fn tool_kind_default_is_primitive() {
    let def = ToolDefinition::new("echo", "Echo test");
    assert!(matches!(def.kind, ToolKind::Primitive));
}

#[test]
fn spawn_subagent_kind_is_composite() {
    let def = ToolDefinition::new("Agent", "Launch sub-agent").with_kind(ToolKind::Composite);
    assert!(matches!(def.kind, ToolKind::Composite));
}

#[test]
fn all_new_plan_c_tools_are_in_catalog() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    for id in &["Write", "Edit", "Bash", "Grep"] {
        assert!(
            TOOL_CATALOG.get(id).is_some(),
            "Tool '{id}' should be registered in TOOL_CATALOG"
        );
    }
}

#[test]
fn ask_user_question_catalog_forbids_model_supplied_other_option() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    let entry = TOOL_CATALOG
        .get_entry("AskUserQuestion")
        .expect("AskUserQuestion should be registered in TOOL_CATALOG");

    let description = &entry.definition.description;
    assert!(
        description.contains("不要在 options 中添加")
            && description.contains("其他")
            && description.contains("Other"),
        "AskUserQuestion description must tell the model not to add custom/Other options: {description}"
    );

    let options_description = entry.json_schema["properties"]["questions"]["items"]["properties"]
        ["options"]["description"]
        .as_str()
        .expect("AskUserQuestion options schema should describe option constraints");
    assert!(
        options_description.contains("不要添加")
            && options_description.contains("其他")
            && options_description.contains("Other"),
        "AskUserQuestion options schema must forbid model-supplied Other options: {options_description}"
    );
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
    let ctx = app_lib::runtime::tools::ToolDescriptionContext::default();
    let schemas = registry
        .get_schemas_filtered(
            &ToolFilter::Only(vec!["WebSearch".to_string(), "WriteMemory".to_string()]),
            &ctx,
            &std::collections::HashMap::new(),
        )
        .await;
    let names: Vec<_> = schemas.iter().map(|s| s.name.clone()).collect();
    // Both are in REQUEST_SCOPED_RUNTIME_TOOL_NAMES so should appear when filtered
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
    assert_eq!(
        builtin, builtin_sorted,
        "filtered built-in partition must be sorted"
    );
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
    use app_lib::runtime::tools::description_context::ToolDescriptionContext;
    use app_lib::runtime::tools::{RuntimeTool, ToolError, ToolExecutionContext, ToolResult};
    use async_trait::async_trait;
    use serde_json::{json, Value};

    struct PredicateTool(ToolDefinition);

    #[async_trait]
    impl RuntimeTool for PredicateTool {
        fn id(&self) -> &str {
            &self.0.id
        }

        // The trait's default `is_read_only` / `is_destructive` call
        // `default_read_only()` / `default_destructive()`.  Tools whose
        // static flags live in `ToolDefinition` plumb them through here so
        // the predicates honor the definition without a per-call async
        // round-trip into `definition()`.
        fn default_read_only(&self) -> bool {
            self.0.default_read_only
        }

        fn default_destructive(&self) -> bool {
            self.0.default_destructive
        }

        async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
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
    let def =
        ToolDefinition::new("some_tool_with_limit", "desc").with_max_result_size_chars(32_000);
    assert_eq!(def.default_max_result_size_chars, 32_000);
}

#[test]
fn catalog_read_workspace_file_has_16000_limit() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;
    let def = TOOL_CATALOG.get("Read").unwrap();
    assert_eq!(def.default_max_result_size_chars, 16_000);
}

#[test]
fn catalog_search_files_has_4000_limit() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;
    let def = TOOL_CATALOG.get("Glob").unwrap();
    assert_eq!(def.default_max_result_size_chars, 4_000);
}

#[test]
fn catalog_other_tools_default_to_8000_when_not_overridden() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    for id in ["WebSearch"] {
        let def = TOOL_CATALOG.get(id).unwrap();
        assert_eq!(
            def.default_max_result_size_chars, 8_000,
            "{} should default to 8000",
            id
        );
    }
}

#[test]
fn catalog_long_running_tools_have_declared_default_timeouts() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    for (id, expected) in [("Bash", Some(120))] {
        let def = TOOL_CATALOG.get(id).unwrap();
        assert_eq!(
            def.default_timeout_secs, expected,
            "{id} should declare the expected default timeout"
        );
    }
}

#[test]
fn catalog_non_long_running_tools_keep_timeout_unset() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    for id in ["Read", "WebSearch"] {
        let def = TOOL_CATALOG.get(id).unwrap();
        assert_eq!(
            def.default_timeout_secs, None,
            "{id} should keep timeout declaration unset"
        );
    }
}
