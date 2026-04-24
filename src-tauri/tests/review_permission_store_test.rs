use std::sync::Arc;

use app_lib::runtime::ids::{RunId, SessionId, ToolCallId};
use app_lib::runtime::store::permission_store::{
    PermissionScope, PermissionSource, PermissionStore, PermissionStoreSnapshot, PolicyDecision,
};
use app_lib::runtime::tools::definition::ToolDefinition;
use app_lib::runtime::tools::permission::{
    persist_permission_decision, PermissionDecision, PermissionDestination, PermissionPipeline,
    StorePolicyPipeline,
};
use app_lib::runtime::tools::context::ToolExecutionContext;
use tempfile::TempDir;

const TOOL: &str = "mcp__demo__tool";
const SCOPE: &str = "mcp";

fn definition(tool_name: &str, scopes: &[&str]) -> ToolDefinition {
    ToolDefinition::new(tool_name, "demo tool").with_capability_scope(scopes.iter().copied())
}

fn ctx() -> ToolExecutionContext {
    ToolExecutionContext::new(
        SessionId::new("conv-permission-store"),
        RunId::new("run-permission-store"),
        None,
        ToolCallId::new("tc-permission-store"),
        app_lib::runtime::cancellation::CancellationToken::new(),
    )
}

fn authorize(store: Arc<PermissionStore>, tool_name: &str, scopes: &[&str]) -> PermissionDecision {
    StorePolicyPipeline::new(store).authorize(&definition(tool_name, scopes), &serde_json::json!({}), &ctx())
}

fn read_snapshot(path: &std::path::Path) -> PermissionStoreSnapshot {
    serde_json::from_str(&std::fs::read_to_string(path).expect("permission file should exist"))
        .expect("permission file should contain snapshot json")
}

fn assert_allow(decision: PermissionDecision) {
    assert!(matches!(decision, PermissionDecision::Allow { .. }), "expected Allow");
}

fn assert_deny(decision: PermissionDecision) {
    assert!(matches!(decision, PermissionDecision::Deny { .. }), "expected Deny");
}

#[test]
fn session_remembered_allow_applies_in_session_without_writing_workspace_or_user_files() {
    let dir = TempDir::new().expect("tempdir");
    let workspace_file = dir.path().join("workspace-permissions.json");
    let user_file = dir.path().join("user-permissions.json");
    let store = Arc::new(PermissionStore::with_layer_files(
        Some(workspace_file.clone()),
        Some(user_file.clone()),
    ));

    persist_permission_decision(
        &store,
        TOOL,
        &[SCOPE.to_string()],
        PolicyDecision::Allow,
        PermissionDestination::Session,
    );

    assert_allow(authorize(store, TOOL, &[SCOPE]));
    assert!(
        !workspace_file.exists() || !std::fs::read_to_string(&workspace_file).unwrap().contains(TOOL),
        "session rule must not be persisted to workspace file"
    );
    assert!(
        !user_file.exists() || !std::fs::read_to_string(&user_file).unwrap().contains(TOOL),
        "session rule must not be persisted to user file"
    );
}

#[test]
fn workspace_remembered_allow_records_workspace_rule() {
    let dir = TempDir::new().expect("tempdir");
    let workspace_file = dir.path().join("workspace-permissions.json");
    let store = PermissionStore::with_layer_files(Some(workspace_file.clone()), None);

    persist_permission_decision(
        &store,
        TOOL,
        &[SCOPE.to_string()],
        PolicyDecision::AlwaysAllow,
        PermissionDestination::Workspace,
    );

    assert_eq!(store.get_for_scope(TOOL, SCOPE), Some(PolicyDecision::AlwaysAllow));
    let snapshot = read_snapshot(&workspace_file);
    let rule = snapshot
        .rules
        .iter()
        .find(|rule| rule.tool_name == TOOL && rule.scope == PermissionScope::Scope(SCOPE.into()))
        .expect("workspace rule should exist in snapshot");
    assert_eq!(rule.source, PermissionSource::Workspace);
}

#[test]
fn user_remembered_allow_records_user_rule() {
    let dir = TempDir::new().expect("tempdir");
    let user_file = dir.path().join("user-permissions.json");
    let store = PermissionStore::with_layer_files(None, Some(user_file.clone()));

    persist_permission_decision(
        &store,
        TOOL,
        &[SCOPE.to_string()],
        PolicyDecision::AlwaysAllow,
        PermissionDestination::User,
    );

    assert_eq!(store.get_for_scope(TOOL, SCOPE), Some(PolicyDecision::AlwaysAllow));
    let snapshot = read_snapshot(&user_file);
    let rule = snapshot
        .rules
        .iter()
        .find(|rule| rule.tool_name == TOOL && rule.scope == PermissionScope::Scope(SCOPE.into()))
        .expect("user rule should exist in snapshot");
    assert_eq!(rule.source, PermissionSource::User);
}

#[test]
fn remembered_deny_denies_same_tool_and_scope_without_asking() {
    let store = Arc::new(PermissionStore::in_memory());

    persist_permission_decision(
        &store,
        TOOL,
        &[SCOPE.to_string()],
        PolicyDecision::AlwaysDeny,
        PermissionDestination::Workspace,
    );

    assert_deny(authorize(store.clone(), TOOL, &[SCOPE]));
    assert_eq!(store.get_for_scope(TOOL, SCOPE), Some(PolicyDecision::AlwaysDeny));
}

#[test]
fn remembered_rule_matches_exact_tool_name_and_scope_only() {
    let store = Arc::new(PermissionStore::in_memory());

    persist_permission_decision(
        &store,
        "mcp__demo__tool_a",
        &[SCOPE.to_string()],
        PolicyDecision::AlwaysAllow,
        PermissionDestination::Workspace,
    );

    assert_allow(authorize(store.clone(), "mcp__demo__tool_a", &[SCOPE]));
    assert!(!matches!(
        authorize(store.clone(), "mcp__demo__tool_a", &["custom:other"]),
        PermissionDecision::Allow { .. }
    ));
    assert!(!matches!(
        authorize(store, "mcp__demo__tool_b", &[SCOPE]),
        PermissionDecision::Allow { .. }
    ));
}

#[test]
fn remembering_multi_scope_tool_records_all_scopes() {
    let dir = TempDir::new().expect("tempdir");
    let workspace_file = dir.path().join("workspace-permissions.json");
    let store = PermissionStore::with_layer_files(Some(workspace_file.clone()), None);

    persist_permission_decision(
        &store,
        TOOL,
        &[SCOPE.to_string(), "custom:data".to_string()],
        PolicyDecision::AlwaysAllow,
        PermissionDestination::Workspace,
    );

    assert_eq!(store.get_for_scope(TOOL, SCOPE), Some(PolicyDecision::AlwaysAllow));
    assert_eq!(store.get_for_scope(TOOL, "custom:data"), Some(PolicyDecision::AlwaysAllow));
    let snapshot = read_snapshot(&workspace_file);
    let workspace_rules = snapshot
        .rules
        .iter()
        .filter(|rule| rule.tool_name == TOOL && rule.source == PermissionSource::Workspace)
        .count();
    assert_eq!(workspace_rules, 2);
}

#[test]
fn ask_decision_defaults_remember_destination_to_session() {
    let store = Arc::new(PermissionStore::in_memory());
    let decision = authorize(store, TOOL, &[SCOPE]);

    let PermissionDecision::Ask {
        remember_options,
        default_destination,
        ..
    } = decision else {
        panic!("unknown mcp scope without stored rule should ask");
    };

    assert_eq!(default_destination, Some(PermissionDestination::Session));
    assert!(remember_options.contains(&PermissionDestination::Session));
    assert!(remember_options.contains(&PermissionDestination::Workspace));
    assert!(remember_options.contains(&PermissionDestination::User));
}

#[test]
fn workspace_and_user_rules_are_read_after_reconstructing_permission_store() {
    let dir = TempDir::new().expect("tempdir");
    let workspace_file = dir.path().join("workspace-permissions.json");
    let user_file = dir.path().join("user-permissions.json");

    {
        let store = PermissionStore::with_layer_files(Some(workspace_file.clone()), Some(user_file.clone()));
        persist_permission_decision(
            &store,
            TOOL,
            &[SCOPE.to_string()],
            PolicyDecision::AlwaysAllow,
            PermissionDestination::Workspace,
        );
    }

    let reloaded = Arc::new(PermissionStore::with_layer_files(Some(workspace_file), Some(user_file)));
    assert_eq!(reloaded.get_for_scope(TOOL, SCOPE), Some(PolicyDecision::AlwaysAllow));
    assert_allow(authorize(reloaded, TOOL, &[SCOPE]));
}

#[test]
fn later_rule_for_same_tool_and_scope_overwrites_earlier_rule() {
    let store = Arc::new(PermissionStore::in_memory());

    persist_permission_decision(
        &store,
        TOOL,
        &[SCOPE.to_string()],
        PolicyDecision::AlwaysAllow,
        PermissionDestination::Workspace,
    );
    persist_permission_decision(
        &store,
        TOOL,
        &[SCOPE.to_string()],
        PolicyDecision::AlwaysDeny,
        PermissionDestination::Workspace,
    );

    assert_eq!(store.get_for_scope(TOOL, SCOPE), Some(PolicyDecision::AlwaysDeny));
    assert_deny(authorize(store, TOOL, &[SCOPE]));
}

#[test]
fn workspace_rule_takes_priority_over_conflicting_user_rule() {
    let store = Arc::new(PermissionStore::in_memory());

    persist_permission_decision(
        &store,
        TOOL,
        &[SCOPE.to_string()],
        PolicyDecision::AlwaysAllow,
        PermissionDestination::User,
    );
    persist_permission_decision(
        &store,
        TOOL,
        &[SCOPE.to_string()],
        PolicyDecision::AlwaysDeny,
        PermissionDestination::Workspace,
    );

    assert_eq!(store.get_for_scope(TOOL, SCOPE), Some(PolicyDecision::AlwaysDeny));
    assert_deny(authorize(store, TOOL, &[SCOPE]));
}
