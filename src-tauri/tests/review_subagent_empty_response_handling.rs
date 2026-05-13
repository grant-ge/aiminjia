//! Review-style source assertions: verify `worker_runtime.rs` integrates the
//! `EmptyResponseRecoveryState` helper and preserves transcript audit trail
//! when the LLM returns an empty turn.
//!
//! Plan: docs/superpowers/plans/2026-05-13-subagent-empty-response-handling.md §7.2

use std::fs;

fn read_worker_runtime() -> String {
    fs::read_to_string("src/runtime/agent/worker_runtime.rs")
        .expect("read src/runtime/agent/worker_runtime.rs")
}

fn read_envelope() -> String {
    fs::read_to_string("src/runtime/agent/subagent_result_envelope.rs")
        .expect("read src/runtime/agent/subagent_result_envelope.rs")
}

#[test]
fn worker_runtime_uses_empty_response_recovery_helper() {
    let src = read_worker_runtime();
    assert!(
        src.contains("EmptyResponseRecoveryState::new"),
        "worker_runtime must instantiate EmptyResponseRecoveryState"
    );
    assert!(
        src.contains("recovery.decide("),
        "worker_runtime must invoke recovery.decide(...)"
    );
    assert!(
        src.contains("RecoveryDecision::Retry"),
        "worker_runtime must handle Retry branch"
    );
    assert!(
        src.contains("RecoveryDecision::Surface"),
        "worker_runtime must handle Surface branch"
    );
    assert!(
        src.contains("continue 'agent_loop"),
        "Retry branch must continue outer loop instead of breaking"
    );
}

#[test]
fn worker_runtime_pushes_audit_assistant_turn_for_empty_content() {
    let src = read_worker_runtime();
    assert!(
        src.contains("[empty turn: stop_reason="),
        "empty-turn placeholder must be present to preserve transcript audit"
    );
    // The old guard wrapped the push so empty content turns silently disappeared.
    // The fix unconditionally pushes — verify the guard is gone.
    assert!(
        !src.contains("if !iter_content.is_empty() {\n                    request\n                        .messages\n                        .push(ChatMessage::text(\"assistant\", iter_content));"),
        "old `if !iter_content.is_empty()` guard around assistant push must be removed"
    );
}

#[test]
fn worker_runtime_stream_error_breaks_outer_loop() {
    let src = read_worker_runtime();
    let idx = src
        .find("StreamEvent::Error { error }")
        .expect("StreamEvent::Error branch must exist");
    let tail = &src[idx..(idx + 800).min(src.len())];
    assert!(
        tail.contains("break 'agent_loop"),
        "stream error branch must break 'agent_loop' (outer) so the error output \
         isn't overwritten by the empty iter_content path"
    );
}

#[test]
fn envelope_has_recovery_audit_fields() {
    let src = read_envelope();
    assert!(
        src.contains("pub terminal_stop_reason: Option<String>"),
        "envelope must expose terminal_stop_reason"
    );
    assert!(
        src.contains("pub max_tokens_recovery_attempts: u32"),
        "envelope must expose max_tokens_recovery_attempts"
    );
    let field_idx = src.find("pub terminal_stop_reason").unwrap();
    let preceding = &src[field_idx.saturating_sub(120)..field_idx];
    assert!(
        preceding.contains("#[serde(default"),
        "terminal_stop_reason must have #[serde(default)] for backward compat"
    );
}
