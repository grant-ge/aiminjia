use app_lib::runtime::chat::ChatTurnOutcome;
use app_lib::runtime::events::{AgentIdleScope, RuntimeEvent, RuntimeEventKind};
use app_lib::runtime::ids::{AgentId, RunId, SessionId};
use app_lib::transport::tauri_event_adapter::map_runtime_event;

#[test]
fn maps_runtime_stream_delta_to_legacy_streaming_delta() {
    let event =
        RuntimeEvent::stream_delta(SessionId::new("conv-1"), RunId::new("run-1"), "hi".into());
    let mapped = map_runtime_event(&event).expect("legacy adapter should expose stream delta");
    assert_eq!(mapped.name, "streaming:delta");
}

#[test]
fn maps_agent_idle_with_scope_metadata() {
    let event = RuntimeEvent::new(
        SessionId::new("conv-1"),
        RunId::new("run-parent"),
        RuntimeEventKind::AgentIdle {
            agent_id: AgentId::new("agent-child"),
            scope: AgentIdleScope::Child,
        },
    );
    let mapped = map_runtime_event(&event).unwrap();
    assert_eq!(
        mapped.payload.get("agentId").and_then(|v| v.as_str()),
        Some("agent-child")
    );
    assert_eq!(
        mapped.payload.get("scope").and_then(|v| v.as_str()),
        Some("child")
    );
}

#[test]
fn maps_permission_ask_runtime_event_to_legacy_permission_ask() {
    let event = RuntimeEvent::new(
        SessionId::new("conv-1"),
        RunId::new("run-1"),
        RuntimeEventKind::PermissionAskRequired {
            tool_call_id: "tc-ask-1".into(),
            tool_name: "bash".to_string(),
            message: "need approval".to_string(),
            suggestions: vec!["Allow once".to_string(), "Deny".to_string()],
        },
    );
    let mapped = map_runtime_event(&event).expect("legacy adapter should expose permission ask");
    assert_eq!(mapped.name, "permission:ask");
    assert_eq!(
        mapped
            .payload
            .get("conversationId")
            .and_then(|v| v.as_str()),
        Some("conv-1")
    );
    assert_eq!(
        mapped.payload.get("runId").and_then(|v| v.as_str()),
        Some("run-1")
    );
    assert_eq!(
        mapped.payload.get("toolCallId").and_then(|v| v.as_str()),
        Some("tc-ask-1")
    );
    assert_eq!(
        mapped.payload.get("toolName").and_then(|v| v.as_str()),
        Some("bash")
    );
    assert_eq!(
        mapped.payload.get("message").and_then(|v| v.as_str()),
        Some("need approval")
    );
    assert_eq!(
        mapped
            .payload
            .get("suggestions")
            .and_then(|v| v.as_array())
            .map(|v| v.len()),
        Some(2)
    );
}

#[test]
fn maps_turn_completed_runtime_event_to_legacy_turn_completed() {
    let event = RuntimeEvent::new(
        SessionId::new("conv-1"),
        RunId::new("run-1"),
        RuntimeEventKind::TurnCompleted {
            outcome: ChatTurnOutcome::BudgetExceeded {
                reason: "Reached maximum budget ($0.50)".to_string(),
                total_cost_usd: 0.75,
            },
            total_input_tokens: 100,
            total_output_tokens: 50,
            total_cost_usd: Some(0.75),
            permission_denial_count: 2,
        },
    );
    let mapped = map_runtime_event(&event).expect("legacy adapter should expose turn completion");
    assert_eq!(mapped.name, "turn:completed");
    assert_eq!(
        mapped
            .payload
            .get("conversationId")
            .and_then(|v| v.as_str()),
        Some("conv-1")
    );
    assert_eq!(
        mapped.payload.get("runId").and_then(|v| v.as_str()),
        Some("run-1")
    );
    assert_eq!(
        mapped.payload.get("outcome").and_then(|v| v.as_str()),
        Some("BudgetExceeded")
    );
    assert_eq!(
        mapped.payload.get("reason").and_then(|v| v.as_str()),
        Some("Reached maximum budget ($0.50)")
    );
    assert_eq!(mapped.payload["totalInputTokens"], 100);
    assert_eq!(mapped.payload["totalOutputTokens"], 50);
    assert_eq!(mapped.payload["totalCostUsd"], 0.75);
    assert_eq!(mapped.payload["permissionDenialCount"], 2);
}
