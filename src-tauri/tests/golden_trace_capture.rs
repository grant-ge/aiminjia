use app_lib::runtime_audit::trace_capture::{capture_legacy_trace, LegacyTraceScenario};

#[tokio::test]
async fn captures_real_legacy_trace_for_basic_chat_flow() {
    let trace = capture_legacy_trace(LegacyTraceScenario::BasicChat)
        .await
        .unwrap();
    assert_eq!(
        trace.event_names(),
        vec!["streaming:delta", "message:updated", "streaming:done"]
    );
}

#[tokio::test]
async fn captures_real_legacy_trace_for_single_tool_flow() {
    let trace = capture_legacy_trace(LegacyTraceScenario::SingleTool)
        .await
        .unwrap();
    assert_eq!(
        trace.event_names(),
        vec!["tool:executing", "tool:completed", "streaming:done"]
    );
}
