use std::sync::Arc;

use app_lib::runtime::session_runtime::SessionRuntime;
use app_lib::transport::testing::NoopRuntimeHost;

#[tokio::test]
async fn runtime_core_executes_without_tauri_app_handle() {
    let host = Arc::new(NoopRuntimeHost);
    let runtime = SessionRuntime::for_test(host);
    runtime
        .run_for_test("conv-1", "run-1", "hello")
        .await
        .unwrap();
    let events = runtime.recorded_events();
    assert_eq!(events.len(), 3);
}
