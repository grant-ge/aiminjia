//! Plan mode 下 Ask 应被转为 Deny，而非保留为 Ask（原来只改 reason）。

use app_lib::runtime::tools::permission::{
    apply_permission_mode, default_permission_ask, PermissionDecision, PermissionMode,
    PermissionReason,
};

fn make_ask() -> PermissionDecision {
    let (remember_options, default_destination) = default_permission_ask();
    PermissionDecision::Ask {
        message: "Run this tool?".into(),
        suggestions: vec!["Allow once".into()],
        remember_options,
        default_destination,
        reason: PermissionReason::UnknownScope,
    }
}

#[test]
fn review_plan_mode_ask_becomes_deny() {
    let result = apply_permission_mode(make_ask(), "Bash", PermissionMode::Plan);
    assert!(
        matches!(result, PermissionDecision::Deny { .. }),
        "Plan mode must convert Ask to Deny, got: {:?}",
        result
    );
    if let PermissionDecision::Deny { reason, .. } = result {
        assert!(
            matches!(reason, PermissionReason::Mode(ref m) if m == "plan"),
            "Deny reason should be Mode(plan)"
        );
    }
}

#[test]
fn review_plan_mode_allow_passes_through() {
    let allow = PermissionDecision::Allow {
        updated_input: None,
        reason: PermissionReason::StoredPolicy,
    };
    let result = apply_permission_mode(allow, "Bash", PermissionMode::Plan);
    assert!(matches!(result, PermissionDecision::Allow { .. }));
}

#[test]
fn review_plan_mode_deny_passes_through() {
    let deny = PermissionDecision::Deny {
        message: "blocked".into(),
        reason: PermissionReason::StoredPolicy,
    };
    let result = apply_permission_mode(deny, "Bash", PermissionMode::Plan);
    assert!(matches!(result, PermissionDecision::Deny { .. }));
}

#[test]
fn review_dont_ask_mode_ask_becomes_deny() {
    let result = apply_permission_mode(make_ask(), "Bash", PermissionMode::DontAsk);
    assert!(
        matches!(result, PermissionDecision::Deny { .. }),
        "DontAsk mode must also convert Ask to Deny"
    );
}
