use app_lib::runtime::chat::{ChatTurnRequest, TurnConfig};
use app_lib::runtime::ids::{RunId, SessionId};

#[test]
fn review_chat_turn_request_uses_session_id_type() {
    let request = ChatTurnRequest::new("conv-1", "hello", vec![]);
    let _: &SessionId = &request.conversation_id;
    assert!(
        request.agent_name.is_none(),
        "new ChatTurnRequest should default agent_name to None"
    );
}

#[test]
fn review_turn_config_uses_session_id_and_run_id() {
    fn assert_session_id(_: &SessionId) {}
    fn assert_run_id(_: &RunId) {}

    let _: fn(&TurnConfig) = |config| {
        assert_session_id(&config.conversation_id);
        assert_run_id(&config.run_id);
    };
}
