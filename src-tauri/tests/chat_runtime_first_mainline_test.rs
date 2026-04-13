/// chat_runtime_first_mainline_test.rs
///
/// Architecture constraint tests for the chat-runtime-first plan.
///
/// These tests document (and enforce) the target state where:
///  - SessionRuntime is the single turn orchestrator
///  - transport/chat_runtime_impl is only a thin adapter, not a full orchestrator
///  - The runtime event bus carries the complete turn lifecycle
///
/// Tests that are marked RED-LIGHT will FAIL on the current codebase because the
/// production path delegates full turn orchestration to the legacy executor.  That
/// is intentional: these tests are the acceptance gate for the plan.

mod common;

use std::sync::Arc;

use app_lib::commands::chat::testsupport::run_send_message_through_runtime;
use app_lib::runtime::{ChatTurnRequest, QueryEngine, RuntimeEventBus, SessionRuntime};

// ─── Test 1: runtime driver must own turn orchestration ───────────────────────

/// RED-LIGHT: SessionRuntime must own the complete turn lifecycle when called via
/// the production path (executor present).  Specifically the runtime event bus must
/// emit MessagePersisted and StreamDone — the same events it emits on the pure
/// (non-executor) path.
///
/// Currently FAILS: executor-backed path emits only RunStarted + StreamStarted from
/// runtime; MessagePersisted and StreamDone are silently lost inside the legacy executor.
#[tokio::test]
async fn runtime_chat_driver_should_own_turn_orchestration() {
    let executor = Arc::new(common::SilentExecutor::default());
    let runtime = SessionRuntime::with_executor(
        QueryEngine::new(),
        RuntimeEventBus::new(),
        executor.clone(),
    );

    runtime
        .run_chat_request(ChatTurnRequest::new(
            "conv-orchestration-check",
            "test turn orchestration ownership",
            vec![],
        ))
        .await
        .unwrap();

    let recorded = runtime.recorded_events();
    let labels = common::event_labels(&recorded);

    // The runtime must emit RunStarted to open a turn (already passing).
    assert!(
        labels.contains(&"RunStarted"),
        "runtime must emit RunStarted. Got: {:?}",
        labels
    );

    // RED-LIGHT: runtime must also persist the message and close the stream.
    assert!(
        labels.contains(&"MessagePersisted"),
        "runtime must own MessagePersisted — not legacy executor. Got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"StreamDone"),
        "runtime must own StreamDone — not legacy executor. Got: {:?}",
        labels
    );
}

// ─── Test 2: transport must not be the full orchestrator ──────────────────────

/// RED-LIGHT: On the production path the legacy executor currently acts as the full
/// turn orchestrator (it drives LLM calls, tool loops, and persistence).  The runtime
/// must reclaim these responsibilities so that the transport layer (`chat_runtime_impl`)
/// is only a thin adapter that forwards requests and subscribes to events.
///
/// We detect the anti-pattern by observing that the runtime bus does NOT carry
/// MessagePersisted or StreamDone when the executor-backed path is used — meaning
/// the transport / executor pair owns the terminal state outside the runtime boundary.
///
/// The assertion below states the desired invariant (runtime emits terminal events),
/// which currently FAILS.
#[tokio::test]
async fn transport_chat_runtime_impl_should_not_be_full_orchestrator() {
    let executor = Arc::new(common::SilentExecutor::default());
    let runtime = SessionRuntime::with_executor(
        QueryEngine::new(),
        RuntimeEventBus::new(),
        executor.clone(),
    );

    runtime
        .run_chat_request(ChatTurnRequest::new(
            "conv-transport-not-orchestrator",
            "verify transport is not full orchestrator",
            vec![],
        ))
        .await
        .unwrap();

    let recorded = runtime.recorded_events();
    let labels = common::event_labels(&recorded);

    // If transport were a thin adapter (desired state), the runtime bus would carry
    // both MessagePersisted and StreamDone.
    let transport_owns_terminal = !labels.contains(&"MessagePersisted")
        && !labels.contains(&"StreamDone");
    assert!(
        !transport_owns_terminal,
        "transport / legacy executor must NOT be the terminal event owner. \
         Runtime bus must carry MessagePersisted and StreamDone. \
         Currently runtime only emits: {:?}",
        labels
    );
}

// ─── Test 3: fixed event observation surface (no-tool scenario) ───────────────

/// GREEN (non-executor path): on the pure runtime path (no executor) the event
/// sequence must be: RunStarted is not emitted by run_for_test, but the three
/// observable legacy events are: streaming:delta -> message:updated -> streaming:done.
///
/// This test acts as a fixture that documents and locks the no-tool event surface
/// for the pure runtime path.  It must stay GREEN across refactors.
#[tokio::test]
async fn chat_runtime_event_trace_no_tool() {
    let trace = run_send_message_through_runtime("conv-event-trace", "hello no-tool")
        .await
        .unwrap();

    let names = trace.event_names();

    // The exact order of legacy events on the no-tool path.
    assert_eq!(
        names,
        vec!["streaming:delta", "message:updated", "streaming:done"],
        "no-tool runtime path must produce exactly [streaming:delta, message:updated, streaming:done]. Got: {:?}",
        names
    );
}

// ─── Test 4: production path event ordering (RED-LIGHT) ───────────────────────

/// RED-LIGHT: on the production (executor-backed) path the runtime event bus must
/// carry the ordered sequence RunStarted → StreamStarted → MessagePersisted → StreamDone.
/// Currently only RunStarted and StreamStarted are emitted by the runtime; the rest
/// are owned by the legacy executor outside the runtime boundary.
#[tokio::test]
async fn chat_runtime_event_trace_production_path_ordered() {
    let executor = Arc::new(common::SilentExecutor::default());
    let runtime = SessionRuntime::with_executor(
        QueryEngine::new(),
        RuntimeEventBus::new(),
        executor,
    );

    runtime
        .run_chat_request(ChatTurnRequest::new(
            "conv-event-trace-ordered",
            "hello production ordered",
            vec![],
        ))
        .await
        .unwrap();

    let recorded = runtime.recorded_events();
    let labels = common::event_labels(&recorded);

    // Desired ordering on the production path.
    // We do not require these to be contiguous, but all four must be present
    // and RunStarted must precede StreamStarted, which must precede MessagePersisted,
    // which must precede StreamDone.
    let pos = |name: &'static str| labels.iter().position(|&l| l == name);

    let run_started = pos("RunStarted");
    let stream_started = pos("StreamStarted");
    let message_persisted = pos("MessagePersisted");
    let stream_done = pos("StreamDone");

    assert!(
        run_started.is_some(),
        "RunStarted must be present in runtime events. Got: {:?}", labels
    );
    assert!(
        stream_started.is_some(),
        "StreamStarted must be present in runtime events. Got: {:?}", labels
    );
    // RED-LIGHT assertions:
    assert!(
        message_persisted.is_some(),
        "MessagePersisted must be present in runtime events on production path. Got: {:?}", labels
    );
    assert!(
        stream_done.is_some(),
        "StreamDone must be present in runtime events on production path. Got: {:?}", labels
    );

    // Verify ordering when all events are present.
    if let (Some(rs), Some(ss), Some(mp), Some(sd)) =
        (run_started, stream_started, message_persisted, stream_done)
    {
        assert!(
            rs < ss,
            "RunStarted ({}) must come before StreamStarted ({}). Labels: {:?}", rs, ss, labels
        );
        assert!(
            ss < mp,
            "StreamStarted ({}) must come before MessagePersisted ({}). Labels: {:?}", ss, mp, labels
        );
        assert!(
            mp < sd,
            "MessagePersisted ({}) must come before StreamDone ({}). Labels: {:?}", mp, sd, labels
        );
    }
}
