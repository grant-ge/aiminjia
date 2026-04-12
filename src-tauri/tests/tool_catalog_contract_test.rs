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
