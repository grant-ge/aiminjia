use app_lib::runtime::agent::{AgentRuntime, ResumeChildRunRequest};
use app_lib::runtime::store::InMemoryAgentInvocationStore;

#[tokio::test]
async fn restores_child_run_from_agent_invocation_store() {
    let store = InMemoryAgentInvocationStore::with_child_run("agent-1", "run-parent", "run-child");
    let runtime = AgentRuntime::for_resume_test(store);
    let restored = runtime
        .resume_child_run(ResumeChildRunRequest::new("agent-1"))
        .await
        .unwrap();
    assert_eq!(restored.child_run_id().as_str(), "run-child");
}
