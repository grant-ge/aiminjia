//! Behavior tests for `EmptyResponseRecoveryState`.
//!
//! State machine semantics:
//!   - `had_content == true` OR `had_tool_calls == true`  → NoRecovery
//!   - `StopReason::MaxTokens` + empty:
//!       attempts_used < max_attempts → Retry (counter++)
//!       attempts_used >= max_attempts → Surface
//!   - any other StopReason + empty → Surface immediately (no retry)
//!
//! Plan: docs/superpowers/plans/2026-05-13-subagent-empty-response-handling.md §7.1

use app_lib::llm::streaming::StopReason;
use app_lib::runtime::agent::empty_response_recovery::{
    EmptyResponseRecoveryConfig, EmptyResponseRecoveryState, RecoveryDecision,
};

#[test]
fn no_recovery_when_content_present() {
    let mut s = EmptyResponseRecoveryState::new(EmptyResponseRecoveryConfig::default());
    let d = s.decide(StopReason::EndTurn, true, false, 64000, 1);
    assert!(matches!(d, RecoveryDecision::NoRecovery));
    assert_eq!(s.attempts_used(), 0);
}

#[test]
fn no_recovery_when_tool_calls_present() {
    let mut s = EmptyResponseRecoveryState::new(EmptyResponseRecoveryConfig::default());
    let d = s.decide(StopReason::ToolUse, false, true, 64000, 1);
    assert!(matches!(d, RecoveryDecision::NoRecovery));
    assert_eq!(s.attempts_used(), 0);
}

#[test]
fn max_tokens_first_two_attempts_retry() {
    let mut s = EmptyResponseRecoveryState::new(EmptyResponseRecoveryConfig::default());

    let d1 = s.decide(StopReason::MaxTokens, false, false, 64000, 1);
    assert!(
        matches!(d1, RecoveryDecision::Retry { .. }),
        "first MaxTokens hit should Retry"
    );
    assert_eq!(s.attempts_used(), 1);

    let d2 = s.decide(StopReason::MaxTokens, false, false, 64000, 2);
    assert!(
        matches!(d2, RecoveryDecision::Retry { .. }),
        "second MaxTokens hit should still Retry"
    );
    assert_eq!(s.attempts_used(), 2);
}

#[test]
fn max_tokens_third_attempt_surfaces() {
    let mut s = EmptyResponseRecoveryState::new(EmptyResponseRecoveryConfig::default());
    s.decide(StopReason::MaxTokens, false, false, 64000, 1);
    s.decide(StopReason::MaxTokens, false, false, 64000, 2);

    let d3 = s.decide(StopReason::MaxTokens, false, false, 64000, 3);
    match d3 {
        RecoveryDecision::Surface { fallback_output } => {
            assert!(
                fallback_output.contains("max_tokens"),
                "surface text must mention max_tokens for father agent visibility"
            );
            assert!(
                fallback_output.contains("内部重试"),
                "surface text must indicate internal retry attempts"
            );
        }
        other => panic!("expected Surface after exhausted attempts, got {:?}", other),
    }
    // counter 不该再 +1
    assert_eq!(s.attempts_used(), 2);
}

#[test]
fn end_turn_with_empty_surfaces_immediately() {
    let mut s = EmptyResponseRecoveryState::new(EmptyResponseRecoveryConfig::default());
    let d = s.decide(StopReason::EndTurn, false, false, 64000, 1);
    match d {
        RecoveryDecision::Surface { fallback_output } => {
            assert!(
                fallback_output.contains("没有产生任何输出"),
                "EndTurn empty surface text missing"
            );
            assert!(fallback_output.contains("EndTurn"));
        }
        other => panic!("EndTurn empty must Surface immediately, got {:?}", other),
    }
    assert_eq!(s.attempts_used(), 0);
}

#[test]
fn stop_sequence_with_empty_surfaces_immediately() {
    let mut s = EmptyResponseRecoveryState::new(EmptyResponseRecoveryConfig::default());
    let d = s.decide(StopReason::StopSequence, false, false, 64000, 1);
    match d {
        RecoveryDecision::Surface { fallback_output } => {
            assert!(fallback_output.contains("没有产生任何输出"));
            assert!(fallback_output.contains("StopSequence"));
        }
        other => panic!("StopSequence empty must Surface, got {:?}", other),
    }
    assert_eq!(s.attempts_used(), 0);
}

#[test]
fn custom_max_attempts_respected() {
    let mut s = EmptyResponseRecoveryState::new(EmptyResponseRecoveryConfig {
        max_attempts: 1,
    });
    let d1 = s.decide(StopReason::MaxTokens, false, false, 64000, 1);
    assert!(matches!(d1, RecoveryDecision::Retry { .. }));
    let d2 = s.decide(StopReason::MaxTokens, false, false, 64000, 2);
    assert!(matches!(d2, RecoveryDecision::Surface { .. }));
    assert_eq!(s.attempts_used(), 1);
}
