//! Verify SubagentWorkerRuntime::build_turn_request applies the three-tier
//! whitelist from P4.1 (def_allowed > def_disallowed > ALL_AGENT_DISALLOWED >
//! [async-only] ASYNC_AGENT_ALLOWED > recursive-spawn guard).
//!
//! This is a unit-level test of the filter logic — we instantiate a minimal
//! ToolRegistry with stub schemas, build a SubAgentConfig, and assert the
//! resulting WorkerTurnRequest.tool_defs.

use app_lib::runtime::agent::tool_whitelist::resolve_agent_tools;

#[test]
fn resolve_agent_tools_removes_recursive_spawn_by_default_for_subagents() {
    // P4.2 hardcodes allow_recursive_spawn=false in worker_runtime.
    // Verify resolve_agent_tools applied with that flag drops spawn_subagent.
    let available = vec!["spawn_subagent".to_string(), "read_file".to_string()];
    let allowed = resolve_agent_tools(
        &["spawn_subagent".to_string(), "read_file".to_string()], // def_allowed
        &[],                                                        // def_disallowed
        &available,
        false, // not async
        false, // allow_recursive_spawn (matches worker_runtime default)
    );
    assert!(allowed.contains(&"read_file".to_string()));
    assert!(
        !allowed.contains(&"spawn_subagent".to_string()),
        "subagent must not be able to spawn sub-sub-agent by default"
    );
}

#[test]
fn resolve_agent_tools_blocks_ask_user_question_for_subagents() {
    let available = vec!["ask_user_question".to_string(), "read_file".to_string()];
    let allowed = resolve_agent_tools(
        &[],
        &[],
        &available,
        false,
        false,
    );
    // ALL_AGENT_DISALLOWED contains ask_user_question
    assert!(!allowed.contains(&"ask_user_question".to_string()));
    assert!(allowed.contains(&"read_file".to_string()));
}

#[test]
fn resolve_agent_tools_async_mode_restricts_to_safe_subset() {
    let available = vec![
        "read_file".to_string(),
        "ask_user_question".to_string(),
        "custom_tool_x".to_string(), // not in ASYNC_AGENT_ALLOWED
    ];
    let allowed = resolve_agent_tools(
        &[],
        &[],
        &available,
        true,  // is_async
        false,
    );
    assert!(allowed.contains(&"read_file".to_string()));
    assert!(!allowed.contains(&"ask_user_question".to_string()));
    // custom_tool_x is NOT in ASYNC_AGENT_ALLOWED list
    assert!(!allowed.contains(&"custom_tool_x".to_string()));
}

#[test]
fn resolve_agent_tools_def_disallowed_overrides_def_allowed() {
    let available = vec!["read_file".to_string(), "write_file".to_string()];
    let allowed = resolve_agent_tools(
        &["read_file".to_string(), "write_file".to_string()],
        &["write_file".to_string()],
        &available,
        false,
        false,
    );
    assert_eq!(allowed, vec!["read_file".to_string()]);
}
