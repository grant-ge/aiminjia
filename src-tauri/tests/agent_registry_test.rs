use app_lib::runtime::agent::definition::{AgentDefinition, AgentModel, AgentPrompt, AgentSource};
use app_lib::runtime::agent::registry::AgentRegistry;

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
    assert!(def.allowed_tools.contains(&"browse_and_extract".to_string()));
    assert!(def.allowed_tools.contains(&"browse_navigate".to_string()));
    assert!(def.allowed_tools.contains(&"read_page_content".to_string()));
    assert!(def.allowed_tools.contains(&"page_execute_js".to_string()));
    assert!(def.allowed_tools.contains(&"extract_table_data".to_string()));
    assert!(def.allowed_tools.contains(&"extract_with_pagination".to_string()));
}

#[test]
fn browse_data_agent_max_iterations_is_30() {
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("browse_data_agent").unwrap();
    assert_eq!(def.max_iterations, 30);
}

#[test]
fn daily_assistant_agent_has_eight_tools() {
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("daily_assistant_agent").unwrap();
    assert_eq!(def.allowed_tools.len(), 8);
    assert!(def.allowed_tools.contains(&"bash".to_string()));
}

#[test]
fn registry_list_returns_all_builtins() {
    let registry = AgentRegistry::with_builtins();
    let list = registry.list();
    assert!(list.len() >= 2);
}

#[allow(dead_code)]
fn _keep_definition_types_used(
    _: AgentDefinition,
    _: AgentModel,
    _: AgentPrompt,
    _: AgentSource,
) {
}
