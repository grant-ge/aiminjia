use std::fs;

#[test]
fn u5_worker_runtime_contract_exists_and_sub_agent_delegates_to_it() {
    let worker_runtime = fs::read_to_string("src/runtime/agent/worker_runtime.rs")
        .expect("read src/runtime/agent/worker_runtime.rs");
    assert!(
        worker_runtime.contains("pub struct WorkerTurnRequest"),
        "worker runtime must define WorkerTurnRequest"
    );
    assert!(
        worker_runtime.contains("pub struct WorkerRunConfig"),
        "worker runtime must define WorkerRunConfig"
    );
    assert!(
        worker_runtime.contains("pub struct SubagentWorkerRuntime"),
        "worker runtime must define SubagentWorkerRuntime"
    );

    let sub_agent = fs::read_to_string("src/llm/sub_agent.rs").expect("read src/llm/sub_agent.rs");
    assert!(
        sub_agent.contains("SubagentWorkerRuntime"),
        "sub_agent must delegate execution to SubagentWorkerRuntime"
    );
}

#[test]
fn u5_sub_agent_no_longer_owns_worker_loop_or_completion_pipeline() {
    let sub_agent = fs::read_to_string("src/llm/sub_agent.rs").expect("read src/llm/sub_agent.rs");

    for forbidden in [
        "stream.next().await",
        "dispatch_batch(",
        "store_transcript(",
        "complete_background_run(",
    ] {
        assert!(
            !sub_agent.contains(forbidden),
            "sub_agent.rs must stop owning worker loop/completion detail: {forbidden}"
        );
    }
}

#[test]
fn u5_worker_runtime_owns_tool_round_and_completion_paths() {
    let worker_runtime = fs::read_to_string("src/runtime/agent/worker_runtime.rs")
        .expect("read src/runtime/agent/worker_runtime.rs");

    for required in [
        "stream.next().await",
        "store_transcript(",
        "complete_background_run(",
    ] {
        assert!(
            worker_runtime.contains(required),
            "worker runtime must own worker execution detail: {required}"
        );
    }

    assert!(
        worker_runtime.contains("ToolRoundDriver"),
        "worker runtime must own tool round dispatch through ToolRoundDriver"
    );
}
