use app_lib::runtime::agent::{AgentRuntime, SpawnChildRunRequest};
use app_lib::runtime::ids::RunId;

#[tokio::test]
async fn cancelling_parent_run_marks_child_run_cancelled() {
    let runtime = AgentRuntime::for_test();
    let request = SpawnChildRunRequest::for_test(RunId::new("run-parent"));
    let handle = runtime.spawn_child_run(request).await.unwrap();
    runtime
        .cancel_run(handle.child_run_id().clone())
        .await
        .unwrap();
    assert_eq!(
        runtime.status(handle.child_run_id()).await.unwrap(),
        "cancelled"
    );
}
