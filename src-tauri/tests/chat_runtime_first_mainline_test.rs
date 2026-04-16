/// chat_runtime_first_mainline_test.rs
///
/// Architecture constraint tests for the chat-runtime-first plan.
///
/// These tests document (and enforce) the target state where:
///  - SessionRuntime is the single turn orchestrator
///  - transport/chat_runtime_impl is only a thin adapter, not a full orchestrator
///  - The runtime event bus carries the complete turn lifecycle
///
/// S4 status: the executor-backed path (legacy RuntimeTurnExecutor) has been
/// removed. All tests here use the pure runtime or RuntimeLlmExecutor path.

mod common;

use app_lib::commands::chat::testsupport::run_send_message_through_runtime;

// ─── Test 3: fixed event observation surface (no-tool scenario) ───────────────

/// GREEN (non-executor path): on the pure runtime path (no executor) the event
/// sequence must be: streaming:delta -> message:updated -> streaming:done.
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
