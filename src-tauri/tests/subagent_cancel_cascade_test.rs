use app_lib::runtime::cancellation::{CancellationReason, CancellationToken};

#[test]
fn test_h2_1_parent_cancel_propagates_to_child() {
    let parent = CancellationToken::new();
    let child = parent.child_token();

    assert!(!parent.is_cancelled());
    assert!(!child.is_cancelled());

    parent.cancel_with_reason(CancellationReason::Interrupt);

    assert!(parent.is_cancelled());
    assert!(child.is_cancelled());
    assert_eq!(parent.reason(), Some(CancellationReason::Interrupt));
    assert_eq!(child.reason(), Some(CancellationReason::Interrupt));
}

#[test]
fn test_h2_2_child_cancel_does_not_reverse_propagate() {
    let parent = CancellationToken::new();
    let child = parent.child_token();

    child.cancel_with_reason(CancellationReason::SiblingError);

    assert!(child.is_cancelled());
    assert_eq!(child.reason(), Some(CancellationReason::SiblingError));
    assert!(!parent.is_cancelled());
    assert_eq!(parent.reason(), None);
}

#[test]
fn test_h2_3_parent_cancel_propagates_to_grandchild() {
    let parent = CancellationToken::new();
    let child = parent.child_token();
    let grandchild = child.child_token();

    parent.cancel_with_reason(CancellationReason::BackgroundStop);

    assert!(child.is_cancelled());
    assert!(grandchild.is_cancelled());
    assert_eq!(child.reason(), Some(CancellationReason::BackgroundStop));
    assert_eq!(grandchild.reason(), Some(CancellationReason::BackgroundStop));
}
