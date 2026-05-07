use app_lib::runtime::tools::catalog::{CatalogEntry, TOOL_CATALOG};
use app_lib::runtime::tools::definition::ToolDefinition;
use serde_json::json;

#[tokio::test]
async fn catalog_supports_dynamic_registration() {
    let tool_id = format!("dynamic_catalog_test_{}", uuid::Uuid::new_v4());
    let entry = CatalogEntry::new(
        ToolDefinition::new(&tool_id, "A dynamic tool"),
        json!({"type": "object", "properties": {}}),
    );

    TOOL_CATALOG.register_entry(entry);

    assert!(TOOL_CATALOG.get(&tool_id).is_some());
}

#[tokio::test]
async fn builtin_tools_present_after_dynamic_catalog_init() {
    assert!(TOOL_CATALOG.get("bash").is_some());
    assert!(TOOL_CATALOG.get("web_search").is_some());
}

#[tokio::test]
async fn concurrent_catalog_access_remains_safe() {
    let prefix = format!("dynamic_catalog_concurrent_{}", uuid::Uuid::new_v4());

    let writer_prefix = prefix.clone();
    let writer = tokio::spawn(async move {
        for i in 0..10 {
            let id = format!("{writer_prefix}_{i}");
            let entry = CatalogEntry::new(
                ToolDefinition::new(&id, "concurrent test tool"),
                json!({"type": "object", "properties": {}}),
            );
            TOOL_CATALOG.register_entry(entry);
        }
    });

    let reader_prefix = prefix.clone();
    let reader = tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        let ids = TOOL_CATALOG.all_ids();
        assert!(
            ids.iter().any(|id| id.starts_with(&reader_prefix)),
            "reader should observe at least one dynamically registered tool id"
        );
    });

    tokio::try_join!(writer, reader).unwrap();
}
