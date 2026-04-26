mod common;

#[tokio::test]
async fn switch_skill_definition_contains_registered_skill_ids() {
    use std::sync::Arc;
    use app_lib::plugin::{SkillRegistry, ToolRegistry};
    use app_lib::runtime::chat::SkillSessionStore;
    use app_lib::runtime::tools::builtin::switch_skill::SwitchSkillRuntimeTool;
    use app_lib::runtime::tools::RuntimeTool;

    let skill_registry = Arc::new(SkillRegistry::new("daily-assistant"));
    common::register_mock_skill(&skill_registry, "comp-analysis-v2", "薪酬分析").await;
    common::register_mock_skill(&skill_registry, "sales-analysis", "销售分析").await;

    let tool_registry = Arc::new(ToolRegistry::new());
    let skill_sessions = Arc::new(SkillSessionStore::new());
    let tool = SwitchSkillRuntimeTool::new(
        skill_registry,
        skill_sessions,
        tool_registry,
    )
    .await;

    let def = tool.definition();

    // The description must enumerate the valid skill IDs so the LLM cannot
    // hallucinate a non-existent one.
    assert!(
        def.description.contains("comp-analysis-v2"),
        "description must list comp-analysis-v2, got: {}",
        def.description
    );
    assert!(
        def.description.contains("sales-analysis"),
        "description must list sales-analysis, got: {}",
        def.description
    );
    assert!(
        !def.description.contains("data-analysis-v2"),
        "description must not mention non-existent skill, got: {}",
        def.description
    );
}
