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
