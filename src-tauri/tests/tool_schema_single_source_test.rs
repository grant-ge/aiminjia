use app_lib::runtime::tools::catalog::ToolCatalog;
use app_lib::runtime::tools::definition::ToolKind;

#[test]
fn catalog_contains_all_registered_tools() {
    let required = vec![
        "Read",
        "Glob",
        "Write",
        "Edit",
        "Bash",
        "Grep",
        "WebSearch",
        "Agent",
        "TaskOutput",
        "Skill",
        "WriteMemory",
        "SearchMemory",
    ];
    let catalog = ToolCatalog::default_catalog();
    for name in &required {
        assert!(
            catalog.get(name).is_some(),
            "Tool '{}' not found in catalog",
            name
        );
    }
}

#[test]
fn spawn_subagent_is_composite_in_catalog() {
    let catalog = ToolCatalog::default_catalog();
    let def = catalog
        .get("Agent")
        .expect("spawn_subagent must be in catalog");
    assert!(
        matches!(def.kind, ToolKind::Composite),
        "spawn_subagent must be Composite kind"
    );
}

#[test]
fn workspace_tools_are_primitive_in_catalog() {
    let catalog = ToolCatalog::default_catalog();
    for name in &["Read", "Glob", "Write", "Edit", "Bash", "Grep"] {
        let def = catalog
            .get(name)
            .unwrap_or_else(|| panic!("{} must be in catalog", name));
        assert!(
            matches!(def.kind, ToolKind::Primitive),
            "{} must be Primitive kind, got {:?}",
            name,
            def.kind
        );
    }
}

#[test]
fn catalog_tool_ids_have_no_duplicates() {
    let catalog = ToolCatalog::default_catalog();
    let ids = catalog.all_ids();
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        ids.len(),
        "Catalog must not contain duplicate tool IDs"
    );
}
