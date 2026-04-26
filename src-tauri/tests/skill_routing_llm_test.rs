mod common;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn switch_skill_definition_contains_registered_skill_ids() {
    use std::sync::Arc;
    use app_lib::plugin::{SkillRegistry, ToolRegistry};
    use app_lib::runtime::chat::SkillSessionStore;
    use app_lib::runtime::tools::builtin::switch_skill::SwitchSkillRuntimeTool;
    use app_lib::runtime::tools::RuntimeTool;
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    let skill_registry = Arc::new(SkillRegistry::new("daily-assistant"));
    common::register_mock_skill(&skill_registry, "comp-analysis-v2", "薪酬分析").await;
    common::register_mock_skill(&skill_registry, "sales-analysis", "销售分析").await;

    let tool_registry = Arc::new(ToolRegistry::new());
    let skill_sessions = Arc::new(SkillSessionStore::new());
    let tool = SwitchSkillRuntimeTool::new(
        skill_registry,
        skill_sessions,
        tool_registry,
    );

    // Calling definition() must update TOOL_CATALOG with the dynamic enum.
    let _def = tool.definition();

    let entry = TOOL_CATALOG
        .get_entry("switch_skill")
        .expect("switch_skill must be in TOOL_CATALOG");

    let skill_id_enum = entry
        .json_schema
        .get("properties")
        .and_then(|p| p.get("skill_id"))
        .and_then(|p| p.get("enum"))
        .and_then(|e| e.as_array())
        .expect("switch_skill json_schema must have skill_id enum");

    let ids: Vec<&str> = skill_id_enum
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    assert!(ids.contains(&"comp-analysis-v2"), "must list comp-analysis-v2, got: {:?}", ids);
    assert!(ids.contains(&"sales-analysis"), "must list sales-analysis, got: {:?}", ids);
    assert!(!ids.contains(&"data-analysis-v2"), "must not list non-existent skill");
}
