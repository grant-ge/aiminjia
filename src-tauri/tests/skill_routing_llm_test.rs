mod common;

#[tokio::test]
async fn default_skill_system_prompt_contains_skill_directory() {
    use std::sync::Arc;
    use app_lib::plugin::SkillRegistry;
    use app_lib::runtime::chat::SkillSessionStore;

    let skill_registry = Arc::new(SkillRegistry::new("daily-assistant"));
    common::register_mock_skill(&skill_registry, "comp-analysis-v2", "专门用于薪酬数据对比分析").await;
    common::register_mock_skill(&skill_registry, "sales-analysis", "销售漏斗和业绩分析").await;
    common::register_mock_skill(&skill_registry, "daily-assistant", "通用日常助手").await;

    let all_tools: Vec<String> = vec!["switch_skill".to_string()];
    let skill_sessions = SkillSessionStore::new();

    let ctx = skill_sessions
        .resolve_turn_context(
            &skill_registry,
            &all_tools,
            "conv-test-001",
            "帮我生成一个 Excel 示例",
            false,
        )
        .await
        .expect("resolve_turn_context should succeed");

    assert_eq!(ctx.skill_id, "daily-assistant");
    assert!(
        ctx.system_prompt.contains("comp-analysis-v2"),
        "system_prompt must list available skill IDs, got: {}",
        &ctx.system_prompt[..200.min(ctx.system_prompt.len())]
    );
    assert!(
        ctx.system_prompt.contains("销售漏斗和业绩分析"),
        "system_prompt must include skill descriptions"
    );
}

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
