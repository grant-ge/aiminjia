//! PermissionStore PathGlob / CommandPattern 匹配路径测试。

use app_lib::runtime::store::permission_store::{
    PermissionRule, PermissionScope, PermissionSource, PermissionStore, PolicyDecision,
};
use app_lib::runtime::tools::permission::PermissionDestination;

#[test]
fn review_path_glob_allow_matches_file_inside_workspace() {
    let store = PermissionStore::in_memory();
    store.record_to(
        PermissionDestination::Session,
        PermissionRule::simple(
            "Write",
            PermissionScope::PathGlob("/tmp/ws/**".into()),
            PolicyDecision::AlwaysAllow,
            PermissionSource::Session,
        ),
    );
    let result = store.get_for_path("Write", "/tmp/ws/data/output.csv");
    assert_eq!(result, Some(PolicyDecision::AlwaysAllow));
}

#[test]
fn review_path_glob_does_not_match_outside_workspace() {
    let store = PermissionStore::in_memory();
    store.record_to(
        PermissionDestination::Session,
        PermissionRule::simple(
            "Write",
            PermissionScope::PathGlob("/tmp/ws/**".into()),
            PolicyDecision::AlwaysAllow,
            PermissionSource::Session,
        ),
    );
    let result = store.get_for_path("Write", "/etc/passwd");
    assert_eq!(result, None);
}

#[test]
fn review_command_pattern_matches_exact_prefix() {
    let store = PermissionStore::in_memory();
    store.record_to(
        PermissionDestination::Workspace,
        PermissionRule::simple(
            "Bash",
            PermissionScope::CommandPattern("git ".into()),
            PolicyDecision::AlwaysAllow,
            PermissionSource::Workspace,
        ),
    );
    let result = store.get_for_command("Bash", "git status --short");
    assert_eq!(result, Some(PolicyDecision::AlwaysAllow));
}

#[test]
fn review_command_pattern_does_not_match_different_command() {
    let store = PermissionStore::in_memory();
    store.record_to(
        PermissionDestination::Workspace,
        PermissionRule::simple(
            "Bash",
            PermissionScope::CommandPattern("git ".into()),
            PolicyDecision::AlwaysAllow,
            PermissionSource::Workspace,
        ),
    );
    let result = store.get_for_command("Bash", "rm -rf /tmp/old");
    assert_eq!(result, None);
}

#[test]
fn review_path_glob_session_overrides_workspace() {
    let store = PermissionStore::in_memory();
    store.record_to(
        PermissionDestination::Workspace,
        PermissionRule::simple(
            "Write",
            PermissionScope::PathGlob("/tmp/ws/**".into()),
            PolicyDecision::AlwaysDeny,
            PermissionSource::Workspace,
        ),
    );
    store.record_to(
        PermissionDestination::Session,
        PermissionRule::simple(
            "Write",
            PermissionScope::PathGlob("/tmp/ws/**".into()),
            PolicyDecision::Allow,
            PermissionSource::Session,
        ),
    );
    let result = store.get_for_path("Write", "/tmp/ws/out.csv");
    assert_eq!(result, Some(PolicyDecision::Allow));
}
