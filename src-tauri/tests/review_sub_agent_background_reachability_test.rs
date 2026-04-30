#[test]
fn review_sub_agent_should_not_hardcode_foreground_child_runs() {
    let source = include_str!("../src/llm/sub_agent.rs");
    assert!(
        !source.contains("background: false"),
        "sub-agent production path still hardcodes foreground child runs, so background summary/message bridge wiring is unreachable"
    );
}
