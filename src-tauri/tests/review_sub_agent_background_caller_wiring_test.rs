#[test]
fn review_browser_agent_entrypoint_should_not_force_foreground_sub_agent_runs() {
    let source = include_str!("../src/llm/tool_executor/internal_system.rs");
    assert!(
        !source.contains("background: false"),
        "current production browser-agent entrypoint still hardcodes foreground sub-agent runs, so background completion remains unreachable from the real caller"
    );
}
