use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::events::RuntimeEventKind;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;

#[tokio::test]
async fn review_cancelled_turn_should_stop_before_emitting_stream_events() {
    let engine = QueryEngine::new();
    let bus = RuntimeEventBus::new();
    let mapping = IdentityMapping::from_legacy_conversation_id("conv-cancel".to_string());
    let mut turn = TurnState::new(mapping, RunId::new("run-cancel"), "hello".to_string());

    turn.cancellation().cancel();

    let result = engine.run(&mut turn, &bus).await;
    assert!(
        result.is_err(),
        "cancelled turn should fail fast instead of streaming content"
    );

    let events = bus.recorded();
    assert!(
        events
            .iter()
            .any(|event| matches!(event.kind, RuntimeEventKind::RunCancelled)),
        "cancelled turn should emit RunCancelled, got: {events:?}"
    );
    assert!(
        events
            .iter()
            .all(|event| !matches!(event.kind, RuntimeEventKind::StreamDelta { .. })),
        "cancelled turn must not emit stream deltas, got: {events:?}"
    );
}
