use app_lib::auth::AuthManager;
use app_lib::plugin::builtin::skills::daily_assistant::DailyAssistantSkill;
use app_lib::plugin::skill_trait::{Skill, SkillState, ToolFilter};
use app_lib::runtime::agent::definition::{AgentDefinition, AgentModel, AgentPrompt, AgentSource};
use app_lib::runtime::agent::registry::AgentRegistry;
use app_lib::storage::file_store::AppStorage;
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn registry_with_builtins_has_browse_data_agent() {
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("browse_data_agent");
    assert!(def.is_some(), "browse_data_agent must be registered");
}

#[test]
fn registry_with_builtins_has_daily_assistant_agent() {
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("daily_assistant_agent");
    assert!(def.is_some(), "daily_assistant_agent must be registered");
}

#[test]
fn browse_data_agent_has_six_browser_tools() {
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("browse_data_agent").unwrap();
    assert_eq!(def.allowed_tools.len(), 6);
    assert!(def
        .allowed_tools
        .contains(&"browse_and_extract".to_string()));
    assert!(def.allowed_tools.contains(&"browse_navigate".to_string()));
    assert!(def.allowed_tools.contains(&"read_page_content".to_string()));
    assert!(def.allowed_tools.contains(&"page_execute_js".to_string()));
    assert!(def
        .allowed_tools
        .contains(&"extract_table_data".to_string()));
    assert!(def
        .allowed_tools
        .contains(&"extract_with_pagination".to_string()));
}

#[test]
fn browse_data_agent_max_iterations_is_30() {
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("browse_data_agent").unwrap();
    assert_eq!(def.max_iterations, 30);
}

#[test]
fn daily_assistant_agent_has_ten_tools() {
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("daily_assistant_agent").unwrap();
    assert_eq!(def.allowed_tools.len(), 14);
    assert!(def.allowed_tools.contains(&"bash".to_string()));
    assert!(def.allowed_tools.contains(&"write_memory".to_string()));
    assert!(def.allowed_tools.contains(&"search_memory".to_string()));
}

#[test]
fn registry_list_returns_all_builtins() {
    let registry = AgentRegistry::with_builtins();
    let list = registry.list();
    assert!(list.len() >= 2);
}

#[allow(dead_code)]
fn _keep_definition_types_used(_: AgentDefinition, _: AgentModel, _: AgentPrompt, _: AgentSource) {}

#[test]
fn browse_data_agent_tools_match_legacy_hardcoded_list() {
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("browse_data_agent").unwrap();
    let expected = vec![
        "browse_and_extract",
        "browse_navigate",
        "read_page_content",
        "page_execute_js",
        "extract_table_data",
        "extract_with_pagination",
    ];
    for tool in &expected {
        assert!(
            def.allowed_tools.contains(&tool.to_string()),
            "browse_data_agent must contain tool: {}",
            tool
        );
    }
    assert_eq!(def.allowed_tools.len(), expected.len());
}

#[test]
fn daily_assistant_tool_filter_matches_registry_definition() {
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("daily_assistant_agent").unwrap();
    let workspace = TempDir::new().expect("TempDir::new failed");
    let storage = Arc::new(AppStorage::new(workspace.path()).expect("AppStorage::new failed"));
    let global_store = Arc::new(app_lib::storage::GlobalConfigStore::new(workspace.path().join("global")));
    let auth_manager = Arc::new(AuthManager::new(global_store, None));
    let skill = DailyAssistantSkill::new_with_registry(&registry, storage, auth_manager);
    let filter = skill.tool_filter(&SkillState::new("daily-assistant"));
    match filter {
        ToolFilter::Only(tools) => {
            assert_eq!(tools.len(), def.allowed_tools.len());
            for tool in &def.allowed_tools {
                assert!(tools.contains(tool), "filter must include {}", tool);
            }
        }
        _ => panic!("DailyAssistantSkill must use ToolFilter::Only"),
    }
}

#[test]
fn daily_assistant_token_budget_defaults_to_8192() {
    let workspace = TempDir::new().expect("TempDir::new failed");
    let storage = Arc::new(AppStorage::new(workspace.path()).expect("AppStorage::new failed"));
    let global_store = Arc::new(app_lib::storage::GlobalConfigStore::new(workspace.path().join("global")));
    let auth_manager = Arc::new(AuthManager::new(global_store, None));
    let skill = DailyAssistantSkill::new(storage, auth_manager);

    assert_eq!(
        skill.token_budget(&SkillState::new("daily-assistant")),
        8192
    );
}
