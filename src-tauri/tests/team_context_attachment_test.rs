//! P2.3b: team_context <system-reminder> attachment for Teammate first turn.
//!
//! Covers the rendering contract.  The "only-once" guarantee is structural —
//! the attachment is sent from `run_teammate_idle`'s init block which runs
//! exactly once per Teammate spawn; subsequent turns enter the select! loop
//! without revisiting that path.

use std::path::PathBuf;

use app_lib::runtime::agent::team_context::{render, render_for_conv_dir};

#[test]
fn render_emits_a_system_reminder_block() {
    let out = render(
        "research-team",
        "alice",
        &PathBuf::from("/a/team.json"),
        &PathBuf::from("/a/tasks"),
    );
    assert!(out.contains("<system-reminder>"));
    assert!(out.contains("</system-reminder>"));
    assert!(out.contains("# 团队协作"));
}

#[test]
fn render_substitutes_team_and_agent_name() {
    let out = render(
        "Acme Team",
        "research-bot",
        &PathBuf::from("/t.json"),
        &PathBuf::from("/tasks"),
    );
    assert!(out.contains("\"Acme Team\""), "team_name not quoted: {out}");
    assert!(out.contains("research-bot"), "agent_name missing: {out}");
}

#[test]
fn render_includes_absolute_paths_verbatim() {
    let team_json = PathBuf::from("/data/u1/c1/team.json");
    let tasks = PathBuf::from("/data/u1/c1/tasks");
    let out = render("t", "n", &team_json, &tasks);
    assert!(out.contains("/data/u1/c1/team.json"));
    assert!(out.contains("/data/u1/c1/tasks"));
}

#[test]
fn render_for_conv_dir_derives_canonical_subpaths() {
    // Per-team disk layout (spec §3): `<conv_dir>/teams/{team_name}/config.json`
    // + `<conv_dir>/teams/{team_name}/tasks`.  The old flat `team.json` /
    // `tasks` paths at the conv root are gone since the multi-team refactor.
    let conv = PathBuf::from("/x/users/scope/conversations/c-42");
    let out = render_for_conv_dir("t", "n", &conv);
    assert!(out.contains("/x/users/scope/conversations/c-42/teams/t/config.json"));
    assert!(out.contains("/x/users/scope/conversations/c-42/teams/t/tasks"));
}

#[test]
fn rendered_block_documents_send_message_shape() {
    let out = render("t", "n", &PathBuf::from("/a"), &PathBuf::from("/b"));
    assert!(
        out.contains("\"to\": \"team-lead\""),
        "missing send example"
    );
    assert!(out.contains("summary"), "missing summary hint");
}
