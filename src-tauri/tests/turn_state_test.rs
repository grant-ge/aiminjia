use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::state::TurnState;

#[test]
fn turn_state_keeps_legacy_conversation_id_only_as_compatibility_field() {
    let mapping = IdentityMapping::from_legacy_conversation_id("conv-1".to_string());
    let turn = TurnState::new(mapping, RunId::new("run-1"), "hello".into());

    assert_eq!(turn.session_id().as_str(), "conv-1");
    assert_eq!(turn.legacy_conversation_id(), Some("conv-1"));
}
