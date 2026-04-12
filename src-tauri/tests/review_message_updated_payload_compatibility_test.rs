use app_lib::commands::chat::testsupport::run_send_message_through_runtime;

#[tokio::test]
async fn review_runtime_message_updated_payload_should_match_legacy_minimum_contract() {
    let trace = run_send_message_through_runtime("conv-compat", "hello runtime")
        .await
        .unwrap();

    let message_event = trace
        .events
        .iter()
        .find(|event| event.name == "message:updated")
        .expect("runtime adapter should emit message:updated");

    assert_eq!(
        message_event
            .payload
            .get("conversationId")
            .and_then(|value| value.as_str()),
        Some("conv-compat")
    );
    assert!(
        message_event
            .payload
            .get("id")
            .and_then(|value| value.as_str())
            .is_some(),
        "legacy message:updated payloads carry a concrete assistant message id so the UI can upsert the message"
    );
    assert_eq!(
        message_event
            .payload
            .get("role")
            .and_then(|value| value.as_str()),
        Some("assistant"),
        "legacy message:updated payloads identify the assistant role explicitly"
    );
    assert!(
        message_event.payload.get("content").is_some(),
        "legacy message:updated payloads carry the assistant content; without it, runtime-only transport cannot reproduce the old UI contract"
    );
}
