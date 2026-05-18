//! Review test for the turn-stage event chain.
//!
//! Spec: lotus/docs/desktop/superpowers/specs/2026-05-17-turn-stages.md.
//!
//! Asserts:
//! 1. Every `TurnStage` variant the emitter exposes maps to a well-formed
//!    `turn:stage` legacy event with camelCase fields.
//! 2. The emitter respects the `AIJIA_TURN_STAGES` feature flag — when off
//!    no stage / heartbeat events land on the bus, and the heartbeat guard is
//!    inert.
//! 3. The `emit_oneshot` helper (used from the compaction closure) follows
//!    the same gating rule as the emitter.
//!
//! End-to-end driver wiring is covered indirectly by the existing
//! `s4_driver_loop_test` suite — re-running it with `AIJIA_TURN_STAGES=1`
//! exercises the chat_turn_driver emission points.  This file focuses on the
//! contract of the emitter + adapter pair so future refactors of those two
//! cannot silently drift the event protocol.

use app_lib::runtime::chat::turn_stage::{emit_oneshot, TurnStageEmitter};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::events::{RunningTool, RuntimeEventKind, TurnStage};
use app_lib::runtime::ids::{RunId, SessionId};
use app_lib::transport::tauri_event_adapter::map_runtime_event;

/// Pull every `TurnStageChanged` from the bus's recorded log, return the
/// stage values in emission order.
fn recorded_stages(bus: &RuntimeEventBus) -> Vec<TurnStage> {
    bus.recorded()
        .into_iter()
        .filter_map(|e| match e.kind {
            RuntimeEventKind::TurnStageChanged { stage, .. } => Some(stage),
            _ => None,
        })
        .collect()
}

/// Post-PR5 the flag defaults ON, so review tests construct the emitter with
/// explicit `with_enabled(...)` to stay hermetic against the ambient env var.
fn make_disabled_emitter() -> (TurnStageEmitter, RuntimeEventBus) {
    let bus = RuntimeEventBus::new();
    let emitter = TurnStageEmitter::new(
        bus.clone(),
        SessionId::new("conv-X"),
        RunId::new("run-Y"),
    )
    .with_enabled(false);
    (emitter, bus)
}

#[tokio::test]
async fn flag_off_emitter_drops_every_transition_silently() {
    let (emitter, bus) = make_disabled_emitter();

    emitter.submitted().await;
    emitter.waiting_llm(0).await;
    emitter.tools_started(0, vec![running("Bash", "tc-1")]).await;
    emitter
        .waiting_permission("Write".into(), "tc-2".into())
        .await;
    emitter
        .waiting_interaction("AskUserQuestion".into(), "int-3".into())
        .await;
    emitter.compacting().await;
    emitter.completing().await;

    assert!(
        recorded_stages(&bus).is_empty(),
        "flag-off emitter must produce zero stage events"
    );
    assert!(
        bus.recorded()
            .iter()
            .all(|e| !matches!(e.kind, RuntimeEventKind::TurnHeartbeat { .. })),
        "flag-off emitter must produce zero heartbeats"
    );
}

#[tokio::test]
async fn flag_on_emitter_records_every_transition_in_order() {
    let bus = RuntimeEventBus::new();
    let emitter = TurnStageEmitter::new(
        bus.clone(),
        SessionId::new("conv-X"),
        RunId::new("run-Y"),
    )
    .with_enabled(true);

    emitter.submitted().await;
    emitter.waiting_llm(0).await;
    emitter
        .tools_started(0, vec![running("Bash", "tc-1"), running("Read", "tc-2")])
        .await;
    emitter
        .waiting_permission("Write".into(), "tc-3".into())
        .await;
    emitter
        .waiting_interaction("AskUserQuestion".into(), "int-9".into())
        .await;
    emitter.compacting().await;
    emitter.completing().await;

    let stages = recorded_stages(&bus);
    assert_eq!(stages.len(), 7, "expected 7 transitions, got {}", stages.len());

    assert!(matches!(stages[0], TurnStage::Submitted));
    assert!(matches!(stages[1], TurnStage::WaitingLlm { iteration: 0 }));
    match &stages[2] {
        TurnStage::Tools { iteration, running, .. } => {
            assert_eq!(*iteration, 0);
            assert_eq!(running.len(), 2);
            assert_eq!(running[0].tool_name, "Bash");
            assert_eq!(running[1].tool_name, "Read");
        }
        other => panic!("expected Tools, got {other:?}"),
    }
    match &stages[3] {
        TurnStage::WaitingPermission { tool_name, tool_call_id } => {
            assert_eq!(tool_name, "Write");
            assert_eq!(tool_call_id, "tc-3");
        }
        other => panic!("expected WaitingPermission, got {other:?}"),
    }
    match &stages[4] {
        TurnStage::WaitingInteraction { interaction_kind, interaction_id } => {
            assert_eq!(interaction_kind, "AskUserQuestion");
            assert_eq!(interaction_id, "int-9");
        }
        other => panic!("expected WaitingInteraction, got {other:?}"),
    }
    assert!(matches!(stages[5], TurnStage::Compacting));
    assert!(matches!(stages[6], TurnStage::Completing));
}

#[tokio::test]
async fn every_stage_variant_maps_to_camelcase_legacy_event() {
    // Roundtrip each variant through the adapter and verify the externally
    // visible JSON shape that the frontend deserializes.  This is the
    // protocol-stability check: any future refactor that changes field
    // names or casing must update this test.
    let bus = RuntimeEventBus::new();
    let emitter = TurnStageEmitter::new(
        bus.clone(),
        SessionId::new("conv-X"),
        RunId::new("run-Y"),
    )
    .with_enabled(true);

    emitter.submitted().await;
    emitter.waiting_llm(2).await;
    emitter.tools_started(2, vec![running("Grep", "tc-1")]).await;
    emitter
        .waiting_permission("Write".into(), "tc-9".into())
        .await;
    emitter
        .waiting_interaction("AskUserQuestion".into(), "int-4".into())
        .await;
    emitter.compacting().await;
    emitter.completing().await;

    let mapped: Vec<_> = bus
        .recorded()
        .iter()
        .filter_map(map_runtime_event)
        .filter(|legacy| legacy.name == "turn:stage")
        .collect();
    assert_eq!(mapped.len(), 7);

    // All payloads must carry conversationId / runId / stageStartedAtMs.
    for legacy in &mapped {
        assert_eq!(legacy.payload["conversationId"], "conv-X");
        assert_eq!(legacy.payload["runId"], "run-Y");
        assert!(
            legacy.payload["stageStartedAtMs"].is_u64(),
            "stageStartedAtMs must be a u64"
        );
        assert!(legacy.payload["stage"].is_object(), "stage must be a JSON object");
        assert!(
            legacy.payload["stage"]["kind"].is_string(),
            "stage.kind must be the discriminator"
        );
    }

    // Spot-check the camelCase discriminators.
    let kinds: Vec<&str> = mapped
        .iter()
        .map(|m| m.payload["stage"]["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        vec![
            "submitted",
            "waitingLlm",
            "tools",
            "waitingPermission",
            "waitingInteraction",
            "compacting",
            "completing",
        ]
    );

    // Tools variant must use camelCase for both the running list entries and
    // the completedInBatch counter (rename_all on the enum + struct).
    let tools_stage = &mapped[2].payload["stage"];
    assert_eq!(tools_stage["iteration"], 2);
    assert_eq!(tools_stage["completedInBatch"], 0);
    assert_eq!(tools_stage["running"][0]["toolName"], "Grep");
    assert_eq!(tools_stage["running"][0]["toolCallId"], "tc-1");
    assert!(tools_stage["running"][0]["startedAtMs"].is_u64());

    // WaitingPermission must use camelCase tool_name → toolName.
    let perm_stage = &mapped[3].payload["stage"];
    assert_eq!(perm_stage["toolName"], "Write");
    assert_eq!(perm_stage["toolCallId"], "tc-9");

    // WaitingInteraction must use camelCase interaction_kind → interactionKind.
    let interaction_stage = &mapped[4].payload["stage"];
    assert_eq!(interaction_stage["interactionKind"], "AskUserQuestion");
    assert_eq!(interaction_stage["interactionId"], "int-4");
}

#[tokio::test]
async fn emit_oneshot_helper_emits_when_default_on() {
    // Post-PR5: default is on, so a bare emit_oneshot should land on the bus.
    // The flag-off path is covered by the per-variant lib tests that use
    // `with_enabled(false)`; emit_oneshot itself is a thin wrapper around the
    // same gate so we don't separately test its disabled branch (would
    // require mutating process env, which races against parallel tests).
    let bus = RuntimeEventBus::new();
    emit_oneshot(
        &bus,
        SessionId::new("conv-X"),
        RunId::new("run-Y"),
        TurnStage::Compacting,
    )
    .await;
    assert_eq!(recorded_stages(&bus).len(), 1);
}

#[tokio::test]
async fn waiting_permission_in_camelcase_does_not_leak_snake_case() {
    // Regression guard: if someone removes the per-variant rename_all on
    // TurnStage::WaitingPermission, this test catches the leak.
    let bus = RuntimeEventBus::new();
    let emitter = TurnStageEmitter::new(bus.clone(), SessionId::new("c"), RunId::new("r"))
        .with_enabled(true);
    emitter
        .waiting_permission("Edit".into(), "tc-42".into())
        .await;

    let legacy = map_runtime_event(&bus.recorded()[0]).expect("mapped");
    assert!(
        legacy.payload["stage"].get("tool_name").is_none(),
        "snake_case `tool_name` leaked into legacy payload"
    );
    assert!(
        legacy.payload["stage"].get("tool_call_id").is_none(),
        "snake_case `tool_call_id` leaked into legacy payload"
    );
    assert_eq!(legacy.payload["stage"]["toolName"], "Edit");
    assert_eq!(legacy.payload["stage"]["toolCallId"], "tc-42");
}

fn running(name: &str, id: &str) -> RunningTool {
    RunningTool {
        tool_name: name.to_string(),
        tool_call_id: id.to_string(),
        started_at_ms: 1_700_000_000_000,
    }
}
