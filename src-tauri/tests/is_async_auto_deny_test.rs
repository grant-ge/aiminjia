//! P2.8: `apply_async_auto_deny` pure-function contract.
//!
//! The function transforms a `PermissionDecision::Ask` into a `Deny` only
//! when `is_async = true`.  All other variants pass through unchanged, and
//! `is_async = false` is a complete pass-through.

use app_lib::runtime::tools::permission::{
    apply_async_auto_deny, PermissionDecision, PermissionReason,
};

fn ask() -> PermissionDecision {
    PermissionDecision::Ask {
        message: "may I read X?".into(),
        suggestions: vec![],
        remember_options: vec![],
        default_destination: None,
        reason: PermissionReason::Other("explicit".into()),
        path_auth_scope: None,
    }
}

fn allow() -> PermissionDecision {
    PermissionDecision::Allow {
        updated_input: None,
        reason: PermissionReason::Other("workspace".into()),
    }
}

fn deny() -> PermissionDecision {
    PermissionDecision::Deny {
        message: "no".into(),
        reason: PermissionReason::Other("deny-rule".into()),
    }
}

#[test]
fn async_runner_auto_denies_ask() {
    let out = apply_async_auto_deny(ask(), "Read", true);
    match out {
        PermissionDecision::Deny { message, reason } => {
            assert!(message.contains("auto-denied"), "message: {message}");
            assert!(matches!(reason, PermissionReason::Mode(ref m) if m == "async"));
        }
        other => panic!("expected Deny, got {other:?}"),
    }
}

#[test]
fn sync_runner_keeps_ask_untouched() {
    let out = apply_async_auto_deny(ask(), "Read", false);
    assert!(matches!(out, PermissionDecision::Ask { .. }));
}

#[test]
fn allow_is_pass_through_in_both_modes() {
    let async_out = apply_async_auto_deny(allow(), "Read", true);
    assert!(matches!(async_out, PermissionDecision::Allow { .. }));
    let sync_out = apply_async_auto_deny(allow(), "Read", false);
    assert!(matches!(sync_out, PermissionDecision::Allow { .. }));
}

#[test]
fn deny_is_pass_through_in_both_modes() {
    let async_out = apply_async_auto_deny(deny(), "Read", true);
    assert!(matches!(async_out, PermissionDecision::Deny { .. }));
    let sync_out = apply_async_auto_deny(deny(), "Read", false);
    assert!(matches!(sync_out, PermissionDecision::Deny { .. }));
}
