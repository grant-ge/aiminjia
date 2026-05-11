//! Required tools a Teammate's Employee profile MUST whitelist for LTR dispatch.
//!
//! Validation is name-based and does not depend on the tools actually being
//! registered yet — `SendMessage` ships in P2.

pub const REQUIRED_TEAMMATE_TOOLS: &[&str] = &["SendMessage", "TaskList", "TaskGet"];

pub fn missing_required(whitelist: &[String]) -> Vec<&'static str> {
    REQUIRED_TEAMMATE_TOOLS
        .iter()
        .copied()
        .filter(|t| !whitelist.iter().any(|w| w == t))
        .collect()
}
