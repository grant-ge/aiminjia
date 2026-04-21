//! Plan-P Permission Alignment 综合约束回归测试。
//! 验证整体权限模型不变量，任何重构后必须继续通过。

use std::sync::Arc;

use app_lib::runtime::store::permission_store::{
    PermissionRule, PermissionScope, PermissionSource, PermissionStore, PolicyDecision,
};
use app_lib::runtime::tools::permission::{
    apply_permission_mode, default_permission_ask, PermissionDecision, PermissionDestination,
    PermissionMode, PermissionReason,
};

// P-1: 三层规则优先级

#[test]
fn review_permission_session_overrides_all_layers() {
    let store = PermissionStore::in_memory();
    store.record_to(
        PermissionDestination::User,
        PermissionRule::simple(
            "bash",
            PermissionScope::Scope("workspace:write".into()),
            PolicyDecision::AlwaysDeny,
            PermissionSource::User,
        ),
    );
    store.record_to(
        PermissionDestination::Workspace,
        PermissionRule::simple(
            "bash",
            PermissionScope::Scope("workspace:write".into()),
            PolicyDecision::AlwaysDeny,
            PermissionSource::Workspace,
        ),
    );
    store.record_to(
        PermissionDestination::Session,
        PermissionRule::simple(
            "bash",
            PermissionScope::Scope("workspace:write".into()),
            PolicyDecision::Allow,
            PermissionSource::Session,
        ),
    );
    assert_eq!(
        store.get_for_scope("bash", "workspace:write"),
        Some(PolicyDecision::Allow),
        "session layer must override workspace and user"
    );
}

#[test]
fn review_permission_workspace_overrides_user_layer() {
    let store = PermissionStore::in_memory();
    store.record_to(
        PermissionDestination::User,
        PermissionRule::simple(
            "execute_python",
            PermissionScope::Scope("python:exec".into()),
            PolicyDecision::AlwaysAllow,
            PermissionSource::User,
        ),
    );
    store.record_to(
        PermissionDestination::Workspace,
        PermissionRule::simple(
            "execute_python",
            PermissionScope::Scope("python:exec".into()),
            PolicyDecision::AlwaysDeny,
            PermissionSource::Workspace,
        ),
    );
    assert_eq!(
        store.get_for_scope("execute_python", "python:exec"),
        Some(PolicyDecision::AlwaysDeny),
        "workspace layer must override user layer"
    );
}

// P-2: PermissionMode 语义

#[test]
fn review_permission_mode_dont_ask_converts_ask_to_deny() {
    let (ro, dd) = default_permission_ask();
    let ask = PermissionDecision::Ask {
        message: "run?".into(),
        suggestions: vec![],
        remember_options: ro,
        default_destination: dd,
        reason: PermissionReason::UnknownScope,
    };
    assert!(matches!(
        apply_permission_mode(ask, "tool", PermissionMode::DontAsk),
        PermissionDecision::Deny { .. }
    ));
}

#[test]
fn review_permission_mode_plan_converts_ask_to_deny() {
    let (ro, dd) = default_permission_ask();
    let ask = PermissionDecision::Ask {
        message: "run?".into(),
        suggestions: vec![],
        remember_options: ro,
        default_destination: dd,
        reason: PermissionReason::UnknownScope,
    };
    assert!(
        matches!(
            apply_permission_mode(ask, "tool", PermissionMode::Plan),
            PermissionDecision::Deny { .. }
        ),
        "Plan mode must also deny Ask"
    );
}

#[test]
fn review_permission_mode_default_preserves_ask() {
    let (ro, dd) = default_permission_ask();
    let ask = PermissionDecision::Ask {
        message: "run?".into(),
        suggestions: vec![],
        remember_options: ro,
        default_destination: dd,
        reason: PermissionReason::UnknownScope,
    };
    assert!(matches!(
        apply_permission_mode(ask, "tool", PermissionMode::Default),
        PermissionDecision::Ask { .. }
    ));
}

// P-3: PathGlob 匹配

#[test]
fn review_path_glob_wildcard_matching() {
    let store = PermissionStore::in_memory();
    store.record_to(
        PermissionDestination::Session,
        PermissionRule::simple(
            "write_file",
            PermissionScope::PathGlob("/workspace/**".into()),
            PolicyDecision::AlwaysAllow,
            PermissionSource::Session,
        ),
    );
    assert_eq!(
        store.get_for_path("write_file", "/workspace/reports/2026/q1.csv"),
        Some(PolicyDecision::AlwaysAllow)
    );
    assert_eq!(store.get_for_path("write_file", "/etc/shadow"), None);
}

// P-4: CommandPattern 匹配

#[test]
fn review_command_pattern_prefix_matching() {
    let store = PermissionStore::in_memory();
    store.record_to(
        PermissionDestination::Workspace,
        PermissionRule::simple(
            "bash",
            PermissionScope::CommandPattern("npm ".into()),
            PolicyDecision::AlwaysAllow,
            PermissionSource::Workspace,
        ),
    );
    assert_eq!(
        store.get_for_command("bash", "npm install --save-dev"),
        Some(PolicyDecision::AlwaysAllow)
    );
    assert_eq!(store.get_for_command("bash", "pip install requests"), None);
}

// P-5: MCP scope 走 Ask 路径

#[test]
fn review_mcp_scope_triggers_ask_via_store_pipeline() {
    use app_lib::runtime::tools::definition::ToolDefinition;
    use app_lib::runtime::tools::permission::{PermissionPipeline, StorePolicyPipeline};
    use app_lib::runtime::tools::ToolExecutionContext;
    use serde_json::json;

    let store = Arc::new(PermissionStore::in_memory());
    let pipeline = StorePolicyPipeline::new(store);
    let def = ToolDefinition::new("mcp__srv__my_tool", "mcp tool").with_capability_scope(["mcp"]);
    let ctx = ToolExecutionContext::for_test("conv", "run", "tc");
    let decision = pipeline.authorize(&def, &json!({}), &ctx);
    assert!(
        matches!(decision, PermissionDecision::Ask { .. }),
        "MCP tool with no stored policy must trigger Ask"
    );
}
