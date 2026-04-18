#[test]
fn review_run_registry_stays_stream_cancel_bridge_only() {
    let source = include_str!("../src/runtime/run_registry.rs");
    assert!(source.contains("watch::Sender<bool>"));
    assert!(source.contains("run_id"));
    assert!(!source.contains("CancellationToken"));
}

#[test]
fn review_session_runtime_no_longer_accepts_injected_cancel_template() {
    let source = include_str!("../src/runtime/session_runtime.rs");
    assert!(!source.contains("cancel_token: Option<CancellationToken>"));
    assert!(!source.contains("with_cancellation_token("));
    assert!(source.contains("session_cancel_roots"));
}
