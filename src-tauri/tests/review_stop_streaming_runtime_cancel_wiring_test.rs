#[test]
fn review_stop_streaming_cancels_runtime_session_before_legacy_bridge() {
    let source = include_str!("../src/transport/tauri_commands/chat.rs");
    let start = source
        .find("pub async fn stop_streaming(&self, conversation_id: String)")
        .expect("stop_streaming should exist");
    let end = source[start..]
        .find("pub async fn approve_permission_request")
        .map(|offset| start + offset)
        .expect("approve_permission_request should follow stop_streaming");
    let stop_streaming_body = &source[start..end];

    let runtime_cancel = stop_streaming_body.find("cancel_session(").expect(
        "stop_streaming must cancel the SessionRuntime root before touching legacy bridges",
    );
    let legacy_bridge = stop_streaming_body
        .find("conversation_service::stop_streaming(")
        .expect("stop_streaming should still bridge to the legacy gateway/python interrupter");

    assert!(
        runtime_cancel < legacy_bridge,
        "stop_streaming must cancel the runtime-owned session root before calling the legacy gateway/python interrupt bridge"
    );
    assert!(
        stop_streaming_body.contains("CancellationReason::Interrupt"),
        "stop_streaming must use the Interrupt cancellation reason so downstream runtime/tool code can distinguish stop_streaming from user-cancelled tool output"
    );
    assert!(
        !stop_streaming_body.contains("cancel_pending_permission_requests_for_session("),
        "pending permission cleanup should be owned by SessionRuntime::cancel_session, not duplicated in the transport adapter"
    );
}
