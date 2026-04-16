// src-tauri/tests/s4_driver_loop_test.rs

use app_lib::runtime::chat::turn_config::*;

#[test]
fn turn_iteration_state_initializes_cleanly() {
    let state = TurnIterationState::new(vec![]);
    assert_eq!(state.iteration_count, 0);
    assert!(!state.stream_cancelled);
    assert!(state.full_content.is_empty());
    assert!(!state.force_no_tools);
}
