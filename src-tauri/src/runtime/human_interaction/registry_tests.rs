use crate::runtime::human_interaction::{
    HumanInteractionId, HumanInteractionKind, HumanInteractionRef, HumanInteractionRegistry,
    HumanInteractionStatus, ImPlatform, InboundUserMessage, OutputBinding, TurnOrigin,
};
use crate::runtime::ids::{RunId, SessionId, ToolCallId};

fn interaction(id: &str, run: &str, kind: HumanInteractionKind) -> HumanInteractionRef {
    HumanInteractionRef {
        id: HumanInteractionId::new(id),
        session_id: SessionId::new("sess-1"),
        run_id: RunId::new(run),
        tool_call_id: ToolCallId::new(format!("tool-{id}")),
        kind,
        turn_origin: TurnOrigin::App,
        output_binding: OutputBinding::AppOnly,
        status: HumanInteractionStatus::Pending,
    }
}

#[test]
fn latest_live_interaction_owns_session_input() {
    let registry = HumanInteractionRegistry::default();
    registry.register(interaction(
        "permission-1",
        "run-1",
        HumanInteractionKind::PermissionAsk,
    ));
    registry.register(interaction(
        "ask-1",
        "run-1",
        HumanInteractionKind::AskUserQuestion,
    ));

    let live = registry
        .latest_live_for_session("sess-1")
        .expect("live interaction");

    assert_eq!(live.id.as_str(), "ask-1");
    assert_eq!(live.kind, HumanInteractionKind::AskUserQuestion);
}

#[test]
fn resolved_interaction_cannot_consume_later_input() {
    let registry = HumanInteractionRegistry::default();
    registry.register(interaction(
        "permission-1",
        "run-1",
        HumanInteractionKind::PermissionAsk,
    ));
    registry.mark_resolved(&HumanInteractionId::new("permission-1"));

    assert!(registry.latest_live_for_session("sess-1").is_none());
}

#[test]
fn early_messages_are_drained_when_interaction_registers() {
    let registry = HumanInteractionRegistry::default();
    registry.buffer_early_message(InboundUserMessage::im_text(
        "sess-1",
        ImPlatform::Dingtalk,
        "conv-1",
        "好了没啊",
    ));

    let drained = registry.register_and_drain(interaction(
        "ask-1",
        "run-1",
        HumanInteractionKind::AskUserQuestion,
    ));

    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].content, "好了没啊");
    assert!(registry.take_early_messages("sess-1").is_empty());
}
