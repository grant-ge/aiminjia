use app_lib::runtime::chat::compaction::{AutoCompactConfig, AutoCompactState};

#[test]
fn k4_auto_compact_state_initial_values() {
    let state = AutoCompactState::new();
    assert!(!state.compacted);
    assert_eq!(state.turn_counter, 0);
    assert_eq!(state.consecutive_failures, 0);
}

#[test]
fn k4_auto_compact_state_circuit_breaker() {
    let mut state = AutoCompactState::new();
    let config = AutoCompactConfig {
        threshold_chars: 1,
        max_output_chars: 80_000,
        consecutive_failure_limit: 3,
        custom_context_window: None,
    };
    state.consecutive_failures = 2;
    assert!(!state.is_circuit_broken(&config));

    state.consecutive_failures = 3;
    assert!(state.is_circuit_broken(&config));
}

#[test]
fn k4_auto_compact_state_reset_on_success() {
    let mut state = AutoCompactState::new();
    state.consecutive_failures = 2;
    state.compacted = true;
    state.turn_counter = 5;

    state.record_success();
    assert_eq!(state.consecutive_failures, 0);
    assert!(state.compacted);
    assert_eq!(state.turn_counter, 0);
}

#[test]
fn k4_auto_compact_state_increment_failure() {
    let mut state = AutoCompactState::new();
    state.record_failure();
    assert_eq!(state.consecutive_failures, 1);
    state.record_failure();
    assert_eq!(state.consecutive_failures, 2);
}

#[test]
fn k4_auto_compact_state_increment_turn() {
    let mut state = AutoCompactState::new();
    state.increment_turn();
    assert_eq!(state.turn_counter, 1);
    state.increment_turn();
    assert_eq!(state.turn_counter, 2);
}
