//! review_agent_registry — 防止 browse_data 工具硬编码退化的架构约束测试。

use app_lib::runtime::agent::registry::AgentRegistry;

/// browse_data_agent 必须在 registry 中注册，且工具列表完整。
#[test]
fn review_agent_registry_browse_data_agent_must_be_registered_with_six_tools() {
    let registry = AgentRegistry::with_builtins();
    let def = registry
        .get("browse_data_agent")
        .expect("browse_data_agent must be registered in AgentRegistry::with_builtins()");

    assert_eq!(
        def.allowed_tools.len(),
        6,
        "browse_data_agent must have exactly 6 browser tools"
    );
}

/// daily_assistant_agent 必须在 registry 中注册。
#[test]
fn review_agent_registry_daily_assistant_agent_must_be_registered() {
    let registry = AgentRegistry::with_builtins();
    let def = registry
        .get("daily_assistant_agent")
        .expect("daily_assistant_agent must be registered in AgentRegistry::with_builtins()");

    assert!(
        def.allowed_tools.len() >= 8,
        "daily_assistant_agent must have at least 8 tools"
    );
}

/// browse_data_agent max_iterations 必须 >= 20。
#[test]
fn review_agent_registry_browse_data_agent_max_iterations_reasonable() {
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("browse_data_agent").unwrap();
    assert!(
        def.max_iterations >= 20,
        "browse_data_agent max_iterations must be at least 20, got {}",
        def.max_iterations
    );
}
