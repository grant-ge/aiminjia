//! review: ALL_AGENT_DISALLOWED static contains expected tool names.
//!
//! Covers 3 membership assertions:
//! 1. task_stop_is_in_all_agent_disallowed
//! 2. agent_is_in_all_agent_disallowed
//! 3. ask_user_question_is_in_all_agent_disallowed

use app_lib::runtime::agent::tool_whitelist::ALL_AGENT_DISALLOWED;

// ─── Test 1 ──────────────────────────────────────────────────────────────────

#[test]
fn task_stop_is_in_all_agent_disallowed() {
    assert!(
        ALL_AGENT_DISALLOWED.contains(&"TaskStop"),
        "TaskStop must be in ALL_AGENT_DISALLOWED — child agents must not be able to cancel \
         sibling/parent tasks; got: {:?}",
        ALL_AGENT_DISALLOWED
    );
}

// ─── Test 2 ──────────────────────────────────────────────────────────────────

#[test]
fn agent_is_in_all_agent_disallowed() {
    assert!(
        ALL_AGENT_DISALLOWED.contains(&"Agent"),
        "Agent must be in ALL_AGENT_DISALLOWED — prevents recursive spawn by default; \
         got: {:?}",
        ALL_AGENT_DISALLOWED
    );
}

// ─── Test 3 ──────────────────────────────────────────────────────────────────

#[test]
fn ask_user_question_is_in_all_agent_disallowed() {
    assert!(
        ALL_AGENT_DISALLOWED.contains(&"AskUserQuestion"),
        "AskUserQuestion must be in ALL_AGENT_DISALLOWED — sub-agents must not \
         be able to directly prompt the user; got: {:?}",
        ALL_AGENT_DISALLOWED
    );
}
