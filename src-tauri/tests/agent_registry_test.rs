use app_lib::runtime::agent::definition::{AgentDefinition, AgentModel, AgentPrompt, AgentSource};
use app_lib::runtime::agent::registry::AgentRegistry;
use app_lib::runtime::tools::catalog::DAILY_ALLOWED_TOOLS;

#[test]
fn registry_with_builtins_has_daily_assistant_agent() {
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("daily_assistant_agent");
    assert!(def.is_some(), "daily_assistant_agent must be registered");
}

#[test]
fn daily_assistant_agent_tools_match_runtime_allowed_tools() {
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("daily_assistant_agent").unwrap();
    assert_eq!(def.allowed_tools.len(), DAILY_ALLOWED_TOOLS.len());
    for tool in DAILY_ALLOWED_TOOLS {
        assert!(
            def.allowed_tools.contains(&tool.to_string()),
            "daily_assistant_agent must contain tool: {}",
            tool
        );
    }
}

#[test]
fn registry_list_returns_all_builtins() {
    let registry = AgentRegistry::with_builtins();
    let list = registry.list();
    assert!(list.len() >= 2);
}

#[allow(dead_code)]
fn _keep_definition_types_used(_: AgentDefinition, _: AgentModel, _: AgentPrompt, _: AgentSource) {}
