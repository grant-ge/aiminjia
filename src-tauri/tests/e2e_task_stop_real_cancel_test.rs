//! True e2e test for TaskStop -> AsyncAgentTaskStore -> CancellationToken cascade.
//!
//! Does not exercise the real LLM gateway (that would require mocking the
//! entire SDK chain). Instead:
//!   1. We register a handle in AsyncAgentTaskStore the same way launch_async
//!      does (via register_anonymous), with a real CancellationToken.
//!   2. We tokio::spawn a long-running task that polls cancel_token.
//!   3. We invoke TaskStopRuntimeTool::execute({task_id: <agent_id>}).
//!   4. We wait (with timeout) for the spawned task to exit due to cancellation.
//!   5. We assert state transitioned to Killed and token reason is BackgroundStop.
//!
//! This proves the TaskStop tool's cancellation pathway works end-to-end at
//! the runtime level. The launch_async wiring itself is locked in by
//! review_async_agent_cancel_token_wired_test.rs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::time::timeout;

use app_lib::runtime::agent::async_task_store::{
    AsyncAgentTaskStore, AsyncTaskHandle, AsyncTaskState,
};
use app_lib::runtime::cancellation::{CancellationReason, CancellationToken};
use app_lib::runtime::ids::AgentId;
use app_lib::runtime::tools::builtin::task_stop::TaskStopRuntimeTool;
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::RuntimeTool;

// ─── Test 1: main e2e ──────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn task_stop_actually_cancels_a_running_background_task() {
    // 1. Setup: store + token + handle
    let store = Arc::new(AsyncAgentTaskStore::new());
    let agent_id = AgentId::new("e2e-cancel-test-001");
    let token = CancellationToken::new();

    let handle = AsyncTaskHandle {
        agent_id: agent_id.clone(),
        state: AsyncTaskState::Running,
        output_file: std::path::PathBuf::from("/tmp/e2e_test.out"),
        description: "long-running e2e test agent".to_string(),
        cancel_token: token.clone(),
    };
    store.register_anonymous(handle);

    // 2. Spawn a real worker that polls the token
    let task_started = Arc::new(AtomicBool::new(false));
    let task_finished = Arc::new(AtomicBool::new(false));
    let started_clone = task_started.clone();
    let finished_clone = task_finished.clone();
    let token_clone = token.clone();

    let join = tokio::spawn(async move {
        started_clone.store(true, Ordering::SeqCst);
        // Loop and poll token. Exit when cancelled.
        for _ in 0..1000 {
            if token_clone.is_cancelled() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        finished_clone.store(true, Ordering::SeqCst);
    });

    // Wait briefly until the spawned task is actually running
    let start_wait = std::time::Instant::now();
    while !task_started.load(Ordering::SeqCst) {
        assert!(
            start_wait.elapsed() < Duration::from_secs(2),
            "spawned task did not start within 2s"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // Confirm not yet cancelled
    assert!(
        !token.is_cancelled(),
        "token should not be cancelled before TaskStop"
    );
    assert!(
        !task_finished.load(Ordering::SeqCst),
        "spawned task should still be running"
    );

    // 3. Invoke TaskStop via the real tool
    let tool = TaskStopRuntimeTool {
        store: store.clone(),
    };
    let ctx = ToolExecutionContext::for_test("e2e-session", "e2e-run", "e2e-tc-001");
    let input = json!({ "task_id": agent_id.as_str() });
    let result = tool.execute(input, ctx).await;
    assert!(result.is_ok(), "TaskStop should succeed: {:?}", result);

    // 4. Wait (bounded) for the spawned task to observe the cancel and exit
    let spawn_finish = timeout(Duration::from_secs(2), join).await;
    assert!(
        spawn_finish.is_ok(),
        "spawned task did not exit within 2s after TaskStop"
    );
    // JoinHandle result
    spawn_finish
        .unwrap()
        .expect("spawned task panicked unexpectedly");

    // 5. Assert the cascade
    assert!(token.is_cancelled(), "token should be cancelled");
    assert_eq!(
        token.reason(),
        Some(CancellationReason::BackgroundStop),
        "reason should be BackgroundStop"
    );
    assert!(
        task_finished.load(Ordering::SeqCst),
        "spawned task should have exited via the cancel path"
    );

    let state = store
        .find_by_id(&agent_id)
        .expect("handle should still exist after stop")
        .state;
    assert_eq!(state, AsyncTaskState::Killed, "state should be Killed");
}

// ─── Test 2: unknown id is a clean error ───────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn task_stop_unknown_id_is_a_clean_error() {
    let store = Arc::new(AsyncAgentTaskStore::new());
    let tool = TaskStopRuntimeTool {
        store: store.clone(),
    };
    let ctx = ToolExecutionContext::for_test("e2e-session", "e2e-run", "e2e-tc-002");
    let result = tool
        .execute(json!({"task_id": "ghost-id-does-not-exist"}), ctx)
        .await;
    assert!(
        result.is_err(),
        "TaskStop for an unknown id should return Err, got: {:?}",
        result
    );
}

// ─── Test 3: stopping one task does not cancel others ─────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn task_stop_does_not_cancel_unrelated_background_tasks() {
    // Two registered tasks; stopping one must NOT cancel the other.
    let store = Arc::new(AsyncAgentTaskStore::new());
    let id_a = AgentId::new("e2e-isolation-a");
    let id_b = AgentId::new("e2e-isolation-b");
    let token_a = CancellationToken::new();
    let token_b = CancellationToken::new();

    store.register_anonymous(AsyncTaskHandle {
        agent_id: id_a.clone(),
        state: AsyncTaskState::Running,
        output_file: "/tmp/e2e_a.out".into(),
        description: "task A".into(),
        cancel_token: token_a.clone(),
    });
    store.register_anonymous(AsyncTaskHandle {
        agent_id: id_b.clone(),
        state: AsyncTaskState::Running,
        output_file: "/tmp/e2e_b.out".into(),
        description: "task B".into(),
        cancel_token: token_b.clone(),
    });

    let tool = TaskStopRuntimeTool {
        store: store.clone(),
    };
    let ctx = ToolExecutionContext::for_test("e2e-session", "e2e-run", "e2e-tc-003");
    tool.execute(json!({"task_id": "e2e-isolation-a"}), ctx)
        .await
        .expect("stopping task A should succeed");

    assert!(token_a.is_cancelled(), "task A token should be cancelled");
    assert!(
        !token_b.is_cancelled(),
        "task B token should NOT be cancelled"
    );
    assert_eq!(
        store.find_by_id(&id_a).unwrap().state,
        AsyncTaskState::Killed,
        "task A should be Killed"
    );
    assert_eq!(
        store.find_by_id(&id_b).unwrap().state,
        AsyncTaskState::Running,
        "task B should still be Running"
    );
}
