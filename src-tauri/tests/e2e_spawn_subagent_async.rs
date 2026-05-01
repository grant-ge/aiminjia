//! P10.2 e2e: async spawn_subagent path — StubLauncher (synchronous completion).
//!
//! Tests the dispatcher contract for `run_in_background: true`:
//! - Tool returns immediately with `{"status":"async_launched","agent_id":"...","name":"..."}`
//! - Named tasks are registered in `AsyncAgentTaskStore`
//! - `TaskNotificationQueue` receives exactly one notification per launch
//! - Un-named tasks skip the store but still enqueue a notification
//! - State is `Completed` after stub finishes (synchronous model avoids timing flakes)
//!
//! Does NOT exercise `DefaultSpawnSubagentLauncher` (slice 3 covered real wiring).
//! No real LLM, no real tokio::spawn.
//!
//! Plan reference: docs/superpowers/plans/2026-04-30-mode-b-progress-handoff.md §P10.2

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use app_lib::runtime::agent::async_task_store::{
    AsyncAgentTaskStore, AsyncTaskHandle, AsyncTaskState,
};
use app_lib::runtime::agent::registry::AgentRegistry;
use app_lib::runtime::agent::task_notification::TaskNotificationQueue;
use app_lib::runtime::ids::AgentId;
use app_lib::runtime::tools::builtin::spawn_subagent::{
    SpawnAsyncOutcome, SpawnSubagentContext, SpawnSubagentLauncher, SpawnSubagentRequest,
    SpawnSubagentRuntimeTool,
};
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::RuntimeTool;

// ─── StubLauncher ─────────────────────────────────────────────────────────────

/// Synchronous-completion stub that mirrors what `DefaultSpawnSubagentLauncher`
/// does but without real `tokio::spawn`.  State transitions happen in-line so
/// tests can assert on `Completed` without any timing concerns.
struct StubLauncher {
    task_store: Arc<AsyncAgentTaskStore>,
    notif_queue: Arc<TaskNotificationQueue>,
}

#[async_trait]
impl SpawnSubagentLauncher for StubLauncher {
    async fn launch_sync(
        &self,
        _req: SpawnSubagentRequest,
        _ctx: SpawnSubagentContext,
    ) -> anyhow::Result<String> {
        unreachable!("test only exercises async path")
    }

    async fn launch_async(
        &self,
        req: SpawnSubagentRequest,
        _ctx: SpawnSubagentContext,
    ) -> anyhow::Result<SpawnAsyncOutcome> {
        // Deterministic agent_id — no real tokio::spawn, no UUIDs that vary.
        let agent_id = AgentId::new(format!("stub-{}", uuid::Uuid::new_v4()));

        // Register named tasks (mirrors DefaultSpawnSubagentLauncher behavior).
        if let Some(name) = &req.name {
            self.task_store.register(
                name,
                AsyncTaskHandle {
                    agent_id: agent_id.clone(),
                    state: AsyncTaskState::Running,
                    output_file: std::path::PathBuf::new(),
                    description: req.description.clone(),
                },
            );
        }

        // Synchronously transition to Completed (avoids tokio::spawn timing flakes).
        self.task_store
            .update_state(&agent_id, AsyncTaskState::Completed);

        // Enqueue task-notification XML so parent LLM can observe completion.
        let xml = format!(
            "<task-notification><task-id>{}</task-id></task-notification>",
            agent_id.as_str()
        );
        self.notif_queue.enqueue(agent_id.as_str(), xml);

        Ok(SpawnAsyncOutcome {
            agent_id,
            name: req.name.clone(),
        })
    }
}

// ─── Helper ───────────────────────────────────────────────────────────────────

fn build_tool() -> (
    SpawnSubagentRuntimeTool,
    Arc<AsyncAgentTaskStore>,
    Arc<TaskNotificationQueue>,
) {
    let task_store = Arc::new(AsyncAgentTaskStore::new());
    let notif_queue = Arc::new(TaskNotificationQueue::new());
    let registry = Arc::new(AgentRegistry::with_builtins());
    let launcher = Arc::new(StubLauncher {
        task_store: task_store.clone(),
        notif_queue: notif_queue.clone(),
    });
    let tool = SpawnSubagentRuntimeTool::new(launcher, registry);
    (tool, task_store, notif_queue)
}

/// Execute the tool with `run_in_background: true` and return the parsed JSON
/// content from `ToolResult`.
async fn execute_async(
    tool: &SpawnSubagentRuntimeTool,
    input: Value,
) -> Value {
    let ctx = ToolExecutionContext::for_test("sess-async", "run-async", "tc-async");
    let result = tool
        .execute(input, ctx)
        .await
        .expect("async execute must not return Err");
    serde_json::from_str(&result.content)
        .expect("async ToolResult.content must be valid JSON")
}

// ─── Test 1 ───────────────────────────────────────────────────────────────────

/// The async path returns a JSON object immediately (no wait for sub-agent
/// completion) containing `status`, `agent_id`, and `name`.
#[tokio::test]
async fn async_path_returns_immediately_with_agent_id() {
    let (tool, _store, _queue) = build_tool();

    let parsed = execute_async(
        &tool,
        json!({
            "subagent_type":    "explore",
            "prompt":           "x",
            "description":      "x",
            "run_in_background": true,
            "name":             "w1",
        }),
    )
    .await;

    assert_eq!(
        parsed.get("status").and_then(Value::as_str),
        Some("async_launched"),
        "status must be 'async_launched', got: {}",
        parsed
    );

    let agent_id = parsed
        .get("agent_id")
        .and_then(Value::as_str)
        .expect("agent_id must be present and a string");
    assert!(!agent_id.is_empty(), "agent_id must be non-empty");

    assert_eq!(
        parsed.get("name").and_then(Value::as_str),
        Some("w1"),
        "name must echo back the supplied value"
    );
}

// ─── Test 2 ───────────────────────────────────────────────────────────────────

/// After a named async launch the task is findable in `AsyncAgentTaskStore` by
/// name, and the state is `Completed` (the stub transitions synchronously).
#[tokio::test]
async fn async_path_registers_in_task_store() {
    let (tool, store, _queue) = build_tool();

    execute_async(
        &tool,
        json!({
            "subagent_type":    "explore",
            "prompt":           "x",
            "description":      "x",
            "run_in_background": true,
            "name":             "w1",
        }),
    )
    .await;

    let handle = store
        .find_by_name("w1")
        .expect("task must be registered under name 'w1'");

    assert_eq!(
        handle.state,
        AsyncTaskState::Completed,
        "stub completes synchronously — state must be Completed, got: {:?}",
        handle.state
    );
}

// ─── Test 3 ───────────────────────────────────────────────────────────────────

/// After a named async launch exactly one notification is enqueued, and it
/// contains the `agent_id` returned in the tool result.
#[tokio::test]
async fn async_path_enqueues_notification() {
    let (tool, _store, queue) = build_tool();

    let parsed = execute_async(
        &tool,
        json!({
            "subagent_type":    "explore",
            "prompt":           "x",
            "description":      "x",
            "run_in_background": true,
            "name":             "w1",
        }),
    )
    .await;

    let agent_id = parsed
        .get("agent_id")
        .and_then(Value::as_str)
        .expect("agent_id must be present");

    let notifications = queue.drain_all();
    assert_eq!(
        notifications.len(),
        1,
        "exactly one notification must be enqueued, got {}",
        notifications.len()
    );

    assert!(
        notifications[0].xml.contains(agent_id),
        "notification XML must contain the agent_id '{}', got: {}",
        agent_id,
        notifications[0].xml
    );
}

// ─── Test 4 ───────────────────────────────────────────────────────────────────

/// When `name` is omitted the task is NOT registered in `AsyncAgentTaskStore`,
/// but the notification is still enqueued (the launcher always enqueues).
#[tokio::test]
async fn async_path_without_name_skips_register() {
    let (tool, store, queue) = build_tool();

    execute_async(
        &tool,
        json!({
            "subagent_type":    "explore",
            "prompt":           "x",
            "description":      "x",
            "run_in_background": true,
            // no "name" field
        }),
    )
    .await;

    assert!(
        store.list_active().is_empty(),
        "no named task registered → list_active must be empty (stub transitions \
         any registered task to Completed immediately, and unnamed tasks are not \
         registered at all)"
    );

    assert!(
        store.find_by_name("anything").is_none(),
        "find_by_name must return None when no task was registered"
    );

    let notifications = queue.drain_all();
    assert_eq!(
        notifications.len(),
        1,
        "notification must still be enqueued even for unnamed tasks, got {}",
        notifications.len()
    );
}

// ─── Test 5 ───────────────────────────────────────────────���───────────────────

/// After a named async launch, looking up the task by `agent_id` (from the
/// JSON response) returns a handle with state `Completed`.
#[tokio::test]
async fn async_path_state_is_completed_after_stub_finishes() {
    let (tool, store, _queue) = build_tool();

    let parsed = execute_async(
        &tool,
        json!({
            "subagent_type":    "explore",
            "prompt":           "x",
            "description":      "x",
            "run_in_background": true,
            "name":             "w1",
        }),
    )
    .await;

    let agent_id_str = parsed
        .get("agent_id")
        .and_then(Value::as_str)
        .expect("agent_id must be present in JSON response");

    let agent_id = AgentId::new(agent_id_str);
    let handle = store
        .find_by_id(&agent_id)
        .unwrap_or_else(|| panic!("find_by_id('{}') must return Some", agent_id_str));

    assert_eq!(
        handle.state,
        AsyncTaskState::Completed,
        "stub transitions to Completed synchronously; state must be Completed, got: {:?}",
        handle.state
    );
}
