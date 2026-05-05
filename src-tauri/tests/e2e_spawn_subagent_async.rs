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
use tempfile::TempDir;

use app_lib::runtime::agent::async_task_store::{
    AsyncAgentTaskStore, AsyncTaskHandle, AsyncTaskState,
};
use app_lib::runtime::agent::output_writer;
use app_lib::runtime::agent::registry::AgentRegistry;
use app_lib::runtime::agent::task_notification::TaskNotificationQueue;
use app_lib::runtime::ids::{AgentId, RunId, SessionId};
use app_lib::runtime::tools::builtin::spawn_subagent::{
    SpawnAsyncOutcome, SpawnSubagentContext, SpawnSubagentLauncher, SpawnSubagentRequest,
    SpawnSubagentRuntimeTool,
};
use app_lib::runtime::tools::builtin::task_output::TaskOutputRuntimeTool;
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::RuntimeTool;
use app_lib::storage::user_scoped_paths::{UserScopedPathResolver, UserScopedPaths};

// ─── StubLauncher ─────────────────────────────────────────────────────────────

/// Synchronous-completion stub that mirrors what `DefaultSpawnSubagentLauncher`
/// does but without real `tokio::spawn`.  State transitions happen in-line so
/// tests can assert on `Completed` without any timing concerns.
struct StubLauncher {
    task_store: Arc<AsyncAgentTaskStore>,
    notif_queue: Arc<TaskNotificationQueue>,
    transcripts_dir: Option<std::path::PathBuf>,
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
        ctx: SpawnSubagentContext,
    ) -> anyhow::Result<SpawnAsyncOutcome> {
        // Deterministic agent_id — no real tokio::spawn, no UUIDs that vary.
        let agent_id = AgentId::new(format!("stub-{}", uuid::Uuid::new_v4()));
        let transcript_path = self.transcripts_dir.as_ref().map(|root| {
            output_writer::transcript_path(
                &UserScopedPaths::new(root, "t_test__u_test").subagent_transcripts_dir(),
                agent_id.as_str(),
            )
        });

        // Register named tasks (mirrors DefaultSpawnSubagentLauncher behavior).
        if let Some(name) = &req.name {
            self.task_store.register(
                name,
                AsyncTaskHandle {
                    agent_id: agent_id.clone(),
                    state: AsyncTaskState::Running,
                    output_file: transcript_path.clone().unwrap_or_default(),
                    description: req.description.clone(),
                },
            );
        }

        // Synchronously transition to Completed (avoids tokio::spawn timing flakes).
        self.task_store
            .update_state(&agent_id, AsyncTaskState::Completed);

        // Write a deterministic transcript line so task_output can read it
        // immediately after Completed becomes visible.
        if let Some(path) = transcript_path.as_ref() {
            let _ = output_writer::append_line(
                path,
                &output_writer::TranscriptLine::assistant(&format!(
                    "done: {}",
                    agent_id.as_str()
                )),
            );
        }

        // Enqueue task-notification XML so parent LLM can observe completion.
        let xml = format!(
            "<task-notification><task-id>{}</task-id><output-file>{}</output-file><status>completed</status></task-notification>",
            agent_id.as_str(),
            transcript_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default()
        );
        self.notif_queue.enqueue(
            agent_id.as_str(),
            xml,
            ctx.session_id.clone(),
            ctx.parent_run_id.clone(),
        );

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
    TempDir,
) {
    let task_store = Arc::new(AsyncAgentTaskStore::new());
    let notif_queue = Arc::new(TaskNotificationQueue::new());
    let registry = Arc::new(AgentRegistry::with_builtins());
    let tmp = TempDir::new().expect("tempdir");
    let launcher = Arc::new(StubLauncher {
        task_store: task_store.clone(),
        notif_queue: notif_queue.clone(),
        transcripts_dir: Some(tmp.path().to_path_buf()),
    });
    let tool = SpawnSubagentRuntimeTool::new(launcher, registry);
    (tool, task_store, notif_queue, tmp)
}

const TEST_SESSION_ID: &str = "sess-async";
const TEST_RUN_ID: &str = "run-async";

/// Execute the tool with `run_in_background: true` and return the parsed JSON
/// content from `ToolResult`.
async fn execute_async(
    tool: &SpawnSubagentRuntimeTool,
    input: Value,
) -> Value {
    let ctx = ToolExecutionContext::for_test(TEST_SESSION_ID, TEST_RUN_ID, "tc-async");
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
    let (tool, _store, _queue, _tmp) = build_tool();

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
    let (tool, store, _queue, _tmp) = build_tool();

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
    let (tool, _store, queue, _tmp) = build_tool();

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

    let notifications = queue.drain_for_session(&SessionId::new(TEST_SESSION_ID));
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
    let (tool, store, queue, _tmp) = build_tool();

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

    let notifications = queue.drain_for_session(&SessionId::new(TEST_SESSION_ID));
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
    let (tool, store, _queue, _tmp) = build_tool();

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

struct TestResolver {
    paths: UserScopedPaths,
}

impl UserScopedPathResolver for TestResolver {
    fn resolve_paths(&self) -> Option<UserScopedPaths> {
        Some(self.paths.clone())
    }
}

#[tokio::test]
async fn completed_state_can_be_read_by_task_output_immediately() {
    let (tool, store, queue, tmp) = build_tool();

    let parsed = execute_async(
        &tool,
        json!({
            "subagent_type": "explore",
            "prompt": "x",
            "description": "x",
            "run_in_background": true,
            "name": "w1",
        }),
    )
    .await;

    let agent_id = parsed
        .get("agent_id")
        .and_then(Value::as_str)
        .expect("agent_id must be present");
    let agent_id = AgentId::new(agent_id);

    let handle = store.find_by_id(&agent_id).expect("handle must exist");
    assert_eq!(handle.state, AsyncTaskState::Completed);
    assert_eq!(
        handle.output_file,
        output_writer::transcript_path(
            &UserScopedPaths::new(tmp.path(), "t_test__u_test").subagent_transcripts_dir(),
            agent_id.as_str()
        )
    );

    let xmls = queue.drain_for_session(&SessionId::new(TEST_SESSION_ID));
    assert_eq!(xmls.len(), 1);
    assert!(
        xmls[0].xml.contains(agent_id.as_str()),
        "notification must contain agent_id"
    );

    let resolver = TaskOutputRuntimeTool::new(Arc::new(TestResolver {
        paths: UserScopedPaths::new(tmp.path(), "t_test__u_test"),
    }));
    let ctx = ToolExecutionContext::for_test(TEST_SESSION_ID, TEST_RUN_ID, "tc-task-output");
    let result = resolver
        .execute(json!({"task_id": agent_id.as_str(), "offset": 0}), ctx)
        .await
        .expect("task_output must succeed immediately after Completed");
    let body: Value = serde_json::from_str(&result.content).expect("valid json body");
    assert_eq!(body["new_offset"].as_u64().unwrap(), 1);
    assert_eq!(body["lines"].as_array().unwrap().len(), 1);
    assert!(
        body["lines"][0]
            .as_str()
            .expect("line must be a string")
            .contains(agent_id.as_str()),
        "task_output should include the agent_id written by the stub"
    );
}

#[tokio::test]
async fn async_path_without_user_scope_still_launches_and_enqueues_notification() {
    let task_store = Arc::new(AsyncAgentTaskStore::new());
    let notif_queue = Arc::new(TaskNotificationQueue::new());
    let registry = Arc::new(AgentRegistry::with_builtins());
    let launcher = Arc::new(StubLauncher {
        task_store: task_store.clone(),
        notif_queue: notif_queue.clone(),
        transcripts_dir: None,
    });
    let tool = SpawnSubagentRuntimeTool::new(launcher, registry);

    let ctx = ToolExecutionContext::new(
        SessionId::new(TEST_SESSION_ID),
        RunId::new(TEST_RUN_ID),
        None,
        "tc-no-scope",
        app_lib::runtime::cancellation::CancellationToken::new(),
    );
    let result = tool
        .execute(
            json!({
                "subagent_type": "explore",
                "prompt": "x",
                "description": "x",
                "run_in_background": true,
                "name": "w1",
            }),
            ctx,
        )
        .await
        .expect("async launch without user scope should still succeed");
    let body: Value = serde_json::from_str(&result.content).expect("json body");
    let agent_id = body["agent_id"]
        .as_str()
        .expect("agent_id must be present");
    assert!(!agent_id.is_empty());

    let handle = task_store
        .find_by_name("w1")
        .expect("named task should still be registered");
    assert_eq!(handle.output_file, std::path::PathBuf::new());

    let notifications = notif_queue.drain_for_session(&SessionId::new(TEST_SESSION_ID));
    assert_eq!(notifications.len(), 1);
    assert!(
        notifications[0].xml.contains(agent_id),
        "notification must still be enqueued even without user scope"
    );
}
