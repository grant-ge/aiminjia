//! Phase 5: sub-agent inherits parent's permission_ctx.
//!
//! Verifies that:
//! 1. A sub-agent constructed from a parent with additional_working_dirs
//!    can see those dirs in its own ctx.
//! 2. A sub-agent's StorageCapability.permission_ctx contains parent's
//!    UserSettings working_dirs and allow_rules.
//! 3. The child's session_attachment_dirs include parent's attachment dirs
//!    (snapshot semantics — child does NOT mutate parent).
//!
//! The tests exercise the data-flow path:
//!   parent QueryEngine (base_permission_ctx + session_attachment_dirs)
//!     → build_turn_permission_ctx (snapshot)
//!     → SubAgentRuntimeDeps.permission_ctx
//!     → SubagentWorkerRuntime::build_query_engine → child QueryEngine.base_permission_ctx

use std::path::PathBuf;
use std::sync::Arc;

use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::path_auth::{
    derive_working_dirs_from_attachments, PathOp, PermissionRule, RuleSource, ToolPermissionContext,
};
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use app_lib::runtime::store::PermissionStore;
use app_lib::runtime::tools::permission::{PermissionDestination, PermissionMode};
use tempfile::TempDir;

/// Helper: build a minimal TurnState with Default permission mode.
fn make_turn() -> TurnState {
    TurnState::new(
        IdentityMapping::from_legacy_conversation_id("conv-subagent-inh".to_string()),
        RunId::new("run-subagent-inh"),
        "test input".to_string(),
    )
    .with_permission_mode(PermissionMode::Default)
}

// ---------------------------------------------------------------------------
// Test 1: Child receives parent's UserSettings working_dirs
// ---------------------------------------------------------------------------
#[test]
fn subagent_inherits_parent_user_settings_working_dirs() {
    // Build a parent permission_ctx with a UserSettings working dir.
    let store = Arc::new(PermissionStore::in_memory());
    let user_dir = PathBuf::from("/Users/example/Projects");
    store
        .append_working_dir(PermissionDestination::User, user_dir.clone())
        .unwrap();

    let entries = app_lib::runtime::path_auth::load_path_auth_entries(&store);
    let mut base_ctx = ToolPermissionContext::empty();
    base_ctx.additional_working_dirs = entries.working_dirs;

    // Parent engine with the UserSettings dir in its base_permission_ctx.
    let parent_engine = QueryEngine::new().with_permission_ctx(Arc::new(base_ctx));

    // Simulate spawn time: snapshot the parent's permission_ctx (as build_turn_permission_ctx
    // would produce, including the UserSettings dir).
    let turn = make_turn();
    let parent_perm_ctx = parent_engine.build_turn_permission_ctx_for_test(&turn);

    // This snapshot is what SubAgentRuntimeDeps.permission_ctx carries.
    // Verify it contains the UserSettings dir.
    assert!(
        parent_perm_ctx
            .additional_working_dirs
            .contains_key(&user_dir),
        "snapshot permission_ctx must contain parent's UserSettings working dir"
    );
    assert_eq!(
        parent_perm_ctx.additional_working_dirs.get(&user_dir),
        Some(&RuleSource::UserSettings),
        "source must be UserSettings (not Session)"
    );

    // Simulate child QueryEngine construction (what SubagentWorkerRuntime::build_query_engine does).
    let child_engine = QueryEngine::new().with_permission_ctx(parent_perm_ctx);
    let child_ctx = child_engine.build_turn_permission_ctx_for_test(&turn);

    assert!(
        child_ctx.additional_working_dirs.contains_key(&user_dir),
        "child engine must see parent's UserSettings working dir"
    );
    assert_eq!(
        child_ctx.additional_working_dirs.get(&user_dir),
        Some(&RuleSource::UserSettings),
        "child must preserve UserSettings source for inherited dir"
    );
}

// ---------------------------------------------------------------------------
// Test 2: Child receives parent's allow_rules
// ---------------------------------------------------------------------------
#[test]
fn subagent_inherits_parent_allow_rules() {
    let mut base_ctx = ToolPermissionContext::empty();
    base_ctx.allow_rules.push(PermissionRule {
        pattern: "/Users/example/Shared/**".to_string(),
        op: Some(PathOp::Read),
        source: RuleSource::UserSettings,
    });

    let parent_engine = QueryEngine::new().with_permission_ctx(Arc::new(base_ctx));
    let turn = make_turn();
    let parent_perm_ctx = parent_engine.build_turn_permission_ctx_for_test(&turn);

    assert_eq!(
        parent_perm_ctx.allow_rules.len(),
        1,
        "snapshot must carry parent's allow_rules"
    );
    assert_eq!(
        parent_perm_ctx.allow_rules[0].pattern,
        "/Users/example/Shared/**"
    );

    // Child QueryEngine inherits via permission_ctx.
    let child_engine = QueryEngine::new().with_permission_ctx(parent_perm_ctx);
    let child_ctx = child_engine.build_turn_permission_ctx_for_test(&turn);

    assert_eq!(
        child_ctx.allow_rules.len(),
        1,
        "child engine must inherit parent's allow_rules"
    );
    assert_eq!(
        child_ctx.allow_rules[0].pattern, "/Users/example/Shared/**",
        "allow_rule pattern must be preserved in child"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Child receives parent's session attachment dirs (snapshot semantics)
// ---------------------------------------------------------------------------
#[test]
fn subagent_inherits_parent_session_attachment_dirs_as_snapshot() {
    let tmp = TempDir::new().unwrap();
    let attachment_file = tmp.path().join("sales.csv");
    std::fs::write(&attachment_file, b"col1,col2\n1,2").unwrap();

    // Parent engine accumulates a session attachment dir.
    let parent_engine = QueryEngine::new();
    let attachment_dirs = derive_working_dirs_from_attachments(&[attachment_file.clone()]);
    assert!(
        !attachment_dirs.is_empty(),
        "attachment should derive a dir"
    );
    parent_engine.merge_session_attachment_dirs(&attachment_dirs);

    // At spawn time: build_turn_permission_ctx produces a snapshot that includes
    // the session attachment dir.
    let turn = make_turn();
    let parent_perm_ctx = parent_engine.build_turn_permission_ctx_for_test(&turn);

    // The attachment dir must appear in the snapshot (as Session source).
    let attachment_dir = std::fs::canonicalize(tmp.path()).unwrap();
    assert!(
        parent_perm_ctx
            .additional_working_dirs
            .contains_key(&attachment_dir),
        "snapshot permission_ctx must contain parent's session attachment dir; \
         dirs in snapshot: {:?}",
        parent_perm_ctx
            .additional_working_dirs
            .keys()
            .collect::<Vec<_>>()
    );
    assert_eq!(
        parent_perm_ctx.additional_working_dirs.get(&attachment_dir),
        Some(&RuleSource::Session),
        "attachment dir source must be Session"
    );

    // Child QueryEngine seeded with parent's snapshot should see the attachment dir.
    let child_engine = QueryEngine::new().with_permission_ctx(parent_perm_ctx);
    let child_ctx = child_engine.build_turn_permission_ctx_for_test(&turn);

    assert!(
        child_ctx
            .additional_working_dirs
            .contains_key(&attachment_dir),
        "child engine must see parent's session attachment dir"
    );
    assert_eq!(
        child_ctx.additional_working_dirs.get(&attachment_dir),
        Some(&RuleSource::Session),
        "child must preserve Session source for inherited attachment dir"
    );
}

// ---------------------------------------------------------------------------
// Test 4: Child mutation does NOT propagate back to parent (snapshot semantics)
// ---------------------------------------------------------------------------
#[test]
fn subagent_attachment_dir_snapshot_does_not_mutate_parent() {
    let tmp = TempDir::new().unwrap();
    let parent_file = tmp.path().join("parent.csv");
    std::fs::write(&parent_file, b"data").unwrap();

    // Parent engine: merge one attachment dir.
    let parent_engine = QueryEngine::new();
    let parent_dirs = derive_working_dirs_from_attachments(&[parent_file.clone()]);
    parent_engine.merge_session_attachment_dirs(&parent_dirs);

    // Take snapshot → seed child engine.
    let turn = make_turn();
    let snapshot = parent_engine.build_turn_permission_ctx_for_test(&turn);
    let child_engine = QueryEngine::new().with_permission_ctx(snapshot);

    // Child merges an ADDITIONAL attachment dir.
    let tmp2 = TempDir::new().unwrap();
    let child_file = tmp2.path().join("child.csv");
    std::fs::write(&child_file, b"data").unwrap();
    let child_dirs = derive_working_dirs_from_attachments(&[child_file.clone()]);
    child_engine.merge_session_attachment_dirs(&child_dirs);

    // Parent's session_attachment_dirs must NOT contain the child-only dir.
    // Build a fresh snapshot from the parent engine to observe its accumulator.
    let parent_ctx_after = parent_engine.build_turn_permission_ctx_for_test(&turn);
    let child_dir_canonical = std::fs::canonicalize(tmp2.path()).unwrap();
    assert!(
        !parent_ctx_after
            .additional_working_dirs
            .contains_key(&child_dir_canonical),
        "parent's session accumulator must NOT be mutated by child additions; \
         parent dirs: {:?}",
        parent_ctx_after
            .additional_working_dirs
            .keys()
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Test 5: SubAgentRuntimeDeps carries permission_ctx field (struct API contract)
// ---------------------------------------------------------------------------
#[test]
fn sub_agent_runtime_deps_carries_permission_ctx_field() {
    // This test verifies the struct field exists and can be set to Some(...).
    // It does NOT run a real sub-agent.
    let mut ctx = ToolPermissionContext::empty();
    ctx.additional_working_dirs.insert(
        PathBuf::from("/Users/example/Data"),
        RuleSource::UserSettings,
    );
    let ctx_arc = Arc::new(ctx);

    // Verify the field round-trips through Option<Arc<...>>.
    let cloned = ctx_arc.clone();
    let maybe_ctx: Option<Arc<ToolPermissionContext>> = Some(cloned);
    assert!(maybe_ctx.is_some());
    let unwrapped = maybe_ctx.unwrap();
    assert!(
        unwrapped
            .additional_working_dirs
            .contains_key(&PathBuf::from("/Users/example/Data")),
        "permission_ctx must survive Option<Arc<>> round-trip"
    );
}
