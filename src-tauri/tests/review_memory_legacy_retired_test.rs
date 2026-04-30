//! review_memory_legacy_retired — 验证 legacy memory 工具名退场，
//! 同时新的 runtime-first memory 工具名仍然可见。

#[tokio::test]
async fn review_legacy_memory_tool_names_retired_but_runtime_names_available() {
    use app_lib::plugin::builtin::tools::register_builtin_tools;
    use app_lib::plugin::registry::ToolRegistry;

    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let names = registry
        .get_all_schemas()
        .await
        .into_iter()
        .map(|schema| schema.name)
        .collect::<Vec<_>>();

    for retired in ["save_memory", "load_core_memory", "distill_memories"] {
        assert!(
            !names.iter().any(|name| name == retired),
            "legacy memory tool '{}' must not remain visible in schema surface",
            retired
        );
    }

    for current in ["write_memory", "search_memory"] {
        assert!(
            names.iter().any(|name| name == current),
            "runtime memory tool '{}' should stay visible in schema surface",
            current
        );
    }
}

#[test]
fn review_new_memory_tools_in_catalog() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    assert!(
        TOOL_CATALOG.get("write_memory").is_some(),
        "write_memory should be in TOOL_CATALOG"
    );
    assert!(
        TOOL_CATALOG.get("search_memory").is_some(),
        "search_memory should be in TOOL_CATALOG"
    );
}
