use app_lib::runtime::agent::invocation::AgentInvocation;
use app_lib::runtime::ids::{AgentId, RunId};

#[test]
fn creates_agent_invocation_with_child_run() {
    let invocation = AgentInvocation::new(
        AgentId::new("agent-1"),
        RunId::new("run-parent"),
        RunId::new("run-child"),
    );
    assert_eq!(invocation.child_run_id().as_str(), "run-child");
}
