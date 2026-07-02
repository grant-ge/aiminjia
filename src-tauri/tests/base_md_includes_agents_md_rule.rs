/// Verify that system.md contains the AGENTS.md rule (Task 4.10, Phase 3).
/// This test uses include_str! so it compiles the content into the binary and
/// will fail at compile time if the file is missing.
#[test]
fn system_md_mentions_agents_md_rule() {
    let system_md = include_str!("../prompts/system.md");
    assert!(
        system_md.contains("AGENTS.md"),
        "system.md must mention AGENTS.md in rule 7"
    );
    assert!(
        system_md.contains("agentsMd"),
        "system.md must reference the agentsMd context tag in rule 7"
    );
}
