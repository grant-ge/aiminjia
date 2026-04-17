use app_lib::runtime::tools::definition::{ToolDefinition, ToolKind};

#[test]
fn tool_definition_has_kind_field() {
    let def = ToolDefinition::new("web_search", "Search the web")
        .with_kind(ToolKind::Primitive);
    assert!(matches!(def.kind, ToolKind::Primitive));
}

#[test]
fn tool_kind_default_is_primitive() {
    let def = ToolDefinition::new("echo", "Echo test");
    assert!(matches!(def.kind, ToolKind::Primitive));
}

#[test]
fn execute_python_kind_is_power() {
    let def = ToolDefinition::new("execute_python", "Run Python")
        .with_kind(ToolKind::Power);
    assert!(matches!(def.kind, ToolKind::Power));
}

#[test]
fn browse_data_kind_is_composite() {
    let def = ToolDefinition::new("browse_data", "Multi-step browser agent")
        .with_kind(ToolKind::Composite);
    assert!(matches!(def.kind, ToolKind::Composite));
}

#[tokio::test]
async fn get_all_schemas_returns_sorted_by_name() {
    use app_lib::plugin::registry::ToolRegistry;
    let registry = ToolRegistry::new();
    let schemas = registry.get_all_schemas().await;
    let names: Vec<_> = schemas.iter().map(|s| s.name.clone()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "get_all_schemas must return tools sorted by name");
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
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "get_schemas_filtered must return tools sorted by name");
}
