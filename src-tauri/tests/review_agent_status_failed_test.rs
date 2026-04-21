//! AgentStatus 必须有 Failed 变体，区分 Cancelled（用户取消）与 Failed（内部错误）。

use app_lib::runtime::agent::invocation::AgentStatus;

#[test]
fn review_agent_status_has_failed_variant() {
    let status = AgentStatus::Failed;
    assert!(matches!(status, AgentStatus::Failed));
}

#[test]
fn review_agent_status_cancelled_is_not_failed() {
    assert!(!matches!(AgentStatus::Cancelled, AgentStatus::Failed));
    assert!(!matches!(AgentStatus::Failed, AgentStatus::Cancelled));
}

#[test]
fn review_agent_status_failed_serializes() {
    let serialized = serde_json::to_string(&AgentStatus::Failed).expect("serialize");
    assert!(serialized.contains("failed") || serialized.contains("Failed"));
}
