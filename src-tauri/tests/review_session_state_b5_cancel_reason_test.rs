use app_lib::runtime::cancellation::{CancellationReason, CancellationToken};

#[test]
fn review_session_state_b5_cancel_with_reason_records_reason() {
    let token = CancellationToken::new();

    token.cancel_with_reason(CancellationReason::UserCancel);

    assert!(token.is_cancelled());
    assert_eq!(token.reason(), Some(CancellationReason::UserCancel));
}

#[test]
fn review_session_state_b5_parent_cancel_propagates_reason_to_child() {
    let parent = CancellationToken::new();
    let child = parent.child_token();

    parent.cancel_with_reason(CancellationReason::Interrupt);

    assert!(parent.is_cancelled());
    assert!(child.is_cancelled());
    assert_eq!(parent.reason(), Some(CancellationReason::Interrupt));
    assert_eq!(child.reason(), Some(CancellationReason::Interrupt));
}

#[test]
fn review_session_state_b5_child_cancel_does_not_override_parent_reason() {
    let parent = CancellationToken::new();
    let child = parent.child_token();

    child.cancel_with_reason(CancellationReason::SiblingError);

    assert!(child.is_cancelled());
    assert_eq!(child.reason(), Some(CancellationReason::SiblingError));
    assert!(!parent.is_cancelled());
    assert_eq!(parent.reason(), None);
}

#[test]
fn review_session_state_b5_cancel_defaults_to_user_cancel() {
    let token = CancellationToken::new();

    token.cancel();

    assert!(token.is_cancelled());
    assert_eq!(token.reason(), Some(CancellationReason::UserCancel));
}
