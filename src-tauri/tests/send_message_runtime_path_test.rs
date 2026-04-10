use app_lib::commands::chat::testsupport::run_send_message_through_runtime;

#[tokio::test]
async fn send_message_emits_legacy_events_via_runtime_adapter() {
    let trace = run_send_message_through_runtime("conv-1", "hello")
        .await
        .unwrap();
    assert_eq!(
        trace.event_names(),
        vec!["streaming:delta", "message:updated", "streaming:done"]
    );
}
