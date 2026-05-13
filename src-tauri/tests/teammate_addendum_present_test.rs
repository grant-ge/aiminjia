//! P2.3: TEAMMATE_ADDENDUM rendering + boot-prompt composition contract.
//!
//! Locks down:
//! - placeholders {team_name} / {teammate_name} get substituted
//! - critical guidance markers (SendMessage, team-lead, shutdown_request,
//!   TaskClaim) are present after rendering
//! - compose_boot_prompt handles empty + non-empty bases

use app_lib::runtime::agent::teammate_addendum::{compose_boot_prompt, render};

#[test]
fn render_includes_critical_markers() {
    let out = render("Acme Researchers", "alice");
    for marker in [
        "Teammate 身份",
        "SendMessage",
        "team-lead",
        "shutdown_request",
        "TaskClaim",
        "TaskUpdate",
        "Acme Researchers",
        "alice",
    ] {
        assert!(
            out.contains(marker),
            "rendered addendum missing marker {marker:?}; got:\n{out}"
        );
    }
}

#[test]
fn render_does_not_leak_placeholder_braces() {
    let out = render("X", "Y");
    assert!(!out.contains("{team_name}"));
    assert!(!out.contains("{teammate_name}"));
}

#[test]
fn compose_boot_prompt_with_employee_extra_keeps_both_parts() {
    let base = "You are 小研，always cite sources.";
    let composed = compose_boot_prompt(base, "research-team", "researcher");
    assert!(composed.starts_with("You are 小研"));
    assert!(composed.contains("Teammate 身份"));
    assert!(composed.contains("`research-team`"));
    assert!(composed.contains("`researcher`"));
}

#[test]
fn compose_boot_prompt_with_empty_base_returns_addendum_only() {
    let composed = compose_boot_prompt("", "t", "n");
    assert!(composed.trim_start().starts_with("## 你正在以 Teammate"));
}
