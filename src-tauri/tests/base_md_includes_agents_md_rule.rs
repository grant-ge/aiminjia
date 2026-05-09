/// Verify that base.md contains the AGENTS.md rule (Task 4.10, Phase 3).
/// This test uses include_str! so it compiles the content into the binary and
/// will fail at compile time if the file is missing.
#[test]
fn base_md_mentions_agents_md_rule() {
    let base_md = include_str!("../prompts/base.md");
    assert!(
        base_md.contains("AGENTS.md"),
        "base.md must mention AGENTS.md in rule 7"
    );
    assert!(
        base_md.contains("agentsMd"),
        "base.md must reference the agentsMd context tag in rule 7"
    );
}
