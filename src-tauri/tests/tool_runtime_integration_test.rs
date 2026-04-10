use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::tools::testing::single_legacy_tool_dispatcher;

#[tokio::test]
async fn query_engine_routes_tool_calls_through_dispatcher_and_permission_pipeline() {
    let dispatcher = single_legacy_tool_dispatcher("python_exec");
    let engine = QueryEngine::for_test(dispatcher);
    let trace = engine
        .run_single_tool_turn("conv-1", "run-1", "python_exec")
        .await
        .unwrap();
    assert_eq!(
        trace,
        vec!["tool:executing", "tool:completed", "streaming:done"]
    );
}
