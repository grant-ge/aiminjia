use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tempfile::TempDir;

use app_lib::runtime::agent::async_task_store::{
    AsyncAgentTaskStore, AsyncTaskHandle, AsyncTaskState,
};
use app_lib::runtime::agent::output_writer::{self, TranscriptLine};
use app_lib::runtime::agent::registry::AgentRegistry;
use app_lib::runtime::agent::task_notification::{
    build_task_notification_xml, TaskNotificationQueue,
};
use app_lib::runtime::cancellation::{wait_for_cancellation, CancellationToken};
use app_lib::runtime::ids::{AgentId, SessionId};
use app_lib::runtime::tools::builtin::spawn_subagent::{
    SpawnAsyncOutcome, SpawnForegroundAutoOutcome, SpawnSubagentContext, SpawnSubagentLauncher,
    SpawnSubagentRequest, SpawnSubagentRuntimeTool,
};
use app_lib::runtime::tools::builtin::task_output::TaskOutputRuntimeTool;
use app_lib::runtime::tools::builtin::task_stop::TaskStopRuntimeTool;
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::RuntimeTool;
use app_lib::storage::user_scoped_paths::{UserScopedPathResolver, UserScopedPaths};

const TEST_SESSION_ID: &str = "sess-auto-bg";
const TEST_RUN_ID: &str = "run-auto-bg";
const TEST_SCOPE: &str = "t_test__u_test";

struct TestResolver {
    paths: UserScopedPaths,
}

impl UserScopedPathResolver for TestResolver {
    fn resolve_paths(&self) -> Option<UserScopedPaths> {
        Some(self.paths.clone())
    }
}

struct AutoBackgroundHarnessLauncher {
    task_store: Arc<AsyncAgentTaskStore>,
    notif_queue: Arc<TaskNotificationQueue>,
    paths: UserScopedPaths,
    subagent_delay_ms: u64,
    subagent_output: String,
}

#[async_trait]
impl SpawnSubagentLauncher for AutoBackgroundHarnessLauncher {
    async fn launch_sync(
        &self,
        _request: SpawnSubagentRequest,
        _context: SpawnSubagentContext,
    ) -> Result<String> {
        unreachable!("tests exercise foreground auto-background path")
    }

    async fn launch_async(
        &self,
        _request: SpawnSubagentRequest,
        _context: SpawnSubagentContext,
    ) -> Result<SpawnAsyncOutcome> {
        unreachable!("tests exercise foreground auto-background path")
    }

    async fn launch_foreground_auto_background(
        &self,
        request: SpawnSubagentRequest,
        context: SpawnSubagentContext,
    ) -> Result<SpawnForegroundAutoOutcome> {
        let auto_background_after_ms = request.auto_background_after_ms.unwrap_or(15_000);
        let child_cancel = CancellationToken::new();

        if self.subagent_delay_ms <= auto_background_after_ms {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(self.subagent_delay_ms)) => {
                    return Ok(SpawnForegroundAutoOutcome::Completed(self.subagent_output.clone()));
                }
                _ = wait_for_cancellation(context.cancellation.clone()) => {
                    child_cancel.cancel();
                    return Err(anyhow!("cancelled before auto-background promotion"));
                }
            }
        }

        #[allow(deprecated)]
        let agent_id = AgentId::new(format!("auto-{}", uuid::Uuid::new_v4()));
        #[allow(deprecated)]
        let transcript_path = output_writer::transcript_path(
            &self.paths.subagent_transcripts_dir(),
            agent_id.as_str(),
        );

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(auto_background_after_ms)) => {
                let handle = AsyncTaskHandle {
                    agent_id: agent_id.clone(),
                    state: AsyncTaskState::Running,
                    output_file: transcript_path.clone(),
                    description: request.description.clone(),
                    cancel_token: child_cancel.clone(),
                };
                if let Some(ref name) = request.name {
                    self.task_store.register(name, handle);
                } else {
                    self.task_store.register_anonymous(handle);
                }

                let store = self.task_store.clone();
                let queue = self.notif_queue.clone();
                let parent_session_id = context.session_id.clone();
                let parent_run_id = context.parent_run_id.clone();
                let parent_tool_use_id = context.parent_tool_use_id.as_str().to_string();
                let subagent_type = request.subagent_type.clone();
                let output = self.subagent_output.clone();
                let remaining_ms = self
                    .subagent_delay_ms
                    .saturating_sub(auto_background_after_ms);
                let task_id = agent_id.clone();
                let task_transcript_path = transcript_path.clone();
                let task_cancel = child_cancel.clone();
                tokio::spawn(async move {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(remaining_ms.max(1))) => {
                            let _ = output_writer::append_line(
                                &task_transcript_path,
                                &TranscriptLine::assistant(output.clone()),
                            );
                            let xml = build_task_notification_xml(
                                task_id.as_str(),
                                Some(&parent_tool_use_id),
                                &task_transcript_path.to_string_lossy(),
                                "completed",
                                &format!("Agent '{}' completed", subagent_type),
                                Some(&output),
                                None,
                            );
                            queue.enqueue(
                                task_id.as_str(),
                                xml,
                                parent_session_id,
                                parent_run_id,
                            );
                            store.update_state(&task_id, AsyncTaskState::Completed);
                        }
                        _ = wait_for_cancellation(task_cancel) => {}
                    }
                });

                Ok(SpawnForegroundAutoOutcome::Backgrounded {
                    agent_id,
                    name: request.name.clone(),
                    auto_background_after_ms,
                })
            }
            _ = wait_for_cancellation(context.cancellation.clone()) => {
                child_cancel.cancel();
                Err(anyhow!("cancelled before auto-background promotion"))
            }
        }
    }
}

struct AutoBackgroundHarness {
    tool: SpawnSubagentRuntimeTool,
    task_store: Arc<AsyncAgentTaskStore>,
    notif_queue: Arc<TaskNotificationQueue>,
    task_output: TaskOutputRuntimeTool,
    task_stop: TaskStopRuntimeTool,
    _tmp: TempDir,
}

impl AutoBackgroundHarness {
    fn new(subagent_delay_ms: u64, subagent_output: impl Into<String>) -> Self {
        let task_store = Arc::new(AsyncAgentTaskStore::new());
        let notif_queue = Arc::new(TaskNotificationQueue::new());
        let tmp = TempDir::new().expect("tempdir");
        let paths = UserScopedPaths::new(tmp.path(), TEST_SCOPE);
        let launcher = Arc::new(AutoBackgroundHarnessLauncher {
            task_store: task_store.clone(),
            notif_queue: notif_queue.clone(),
            paths: paths.clone(),
            subagent_delay_ms,
            subagent_output: subagent_output.into(),
        });
        let tool =
            SpawnSubagentRuntimeTool::new(launcher, Arc::new(AgentRegistry::with_builtins()));
        let resolver = Arc::new(TestResolver { paths });
        Self {
            tool,
            task_store: task_store.clone(),
            notif_queue: notif_queue.clone(),
            task_output: TaskOutputRuntimeTool::new(resolver.clone()),
            task_stop: TaskStopRuntimeTool { store: task_store },
            _tmp: tmp,
        }
    }

    async fn execute(&self, input: Value) -> app_lib::runtime::tools::executor::ToolResult {
        self.tool
            .execute(
                input,
                ToolExecutionContext::for_test(TEST_SESSION_ID, TEST_RUN_ID, "tc-auto-bg"),
            )
            .await
            .expect("spawn_subagent execute should succeed")
    }

    async fn execute_with_cancel(
        &self,
        cancel: CancellationToken,
        input: Value,
    ) -> Result<app_lib::runtime::tools::executor::ToolResult> {
        self.tool
            .execute(
                input,
                ToolExecutionContext::new(
                    app_lib::runtime::ids::SessionId::new(TEST_SESSION_ID),
                    app_lib::runtime::ids::RunId::new(TEST_RUN_ID),
                    None,
                    "tc-auto-bg-cancel",
                    cancel,
                ),
            )
            .await
            .map_err(|err| anyhow!(err.to_string()))
    }

    async fn wait_for_state(&self, task_id: &str, expected: AsyncTaskState) {
        for _ in 0..50 {
            if self
                .task_store
                .find_by_id(&AgentId::new(task_id))
                .map(|handle| handle.state == expected)
                .unwrap_or(false)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let current = self.task_store.find_by_id(&AgentId::new(task_id));
        panic!(
            "task {task_id} did not reach {:?}, current: {:?}",
            expected,
            current.map(|h| h.state)
        );
    }

    async fn read_task_output(&self, task_id: &str, offset: u64) -> Value {
        let result = self
            .task_output
            .execute(
                json!({
                    "task_id": task_id,
                    "offset": offset,
                    "task_type": "local_agent",
                }),
                ToolExecutionContext::for_test(TEST_SESSION_ID, TEST_RUN_ID, "tc-task-output"),
            )
            .await
            .expect("task_output must succeed");
        serde_json::from_str(&result.content).expect("task_output json")
    }

    async fn stop_task(&self, task_id: &str) -> app_lib::runtime::tools::executor::ToolResult {
        self.task_stop
            .execute(
                json!({
                    "task_id": task_id,
                    "task_type": "local_agent",
                }),
                ToolExecutionContext::for_test(TEST_SESSION_ID, TEST_RUN_ID, "tc-task-stop"),
            )
            .await
            .expect("task_stop must succeed")
    }
}

#[tokio::test]
async fn foreground_auto_completed_returns_sync_output() {
    let harness = AutoBackgroundHarness::new(1, "short-result");

    let result = harness
        .execute(json!({
            "subagent_type": "explore",
            "prompt": "short",
            "description": "short",
            "_auto_background_after_ms": 50,
        }))
        .await;

    assert_eq!(result.content, "short-result");
    assert!(
        harness.task_store.list_active().is_empty(),
        "short foreground task should not register a background handle"
    );
}

#[tokio::test]
async fn foreground_auto_promotes_to_local_agent_task() {
    let harness = AutoBackgroundHarness::new(80, "long-result");

    let result = harness
        .execute(json!({
            "subagent_type": "explore",
            "prompt": "long",
            "description": "long",
            "_auto_background_after_ms": 5,
            "name": "auto-agent-1",
        }))
        .await;
    let body: Value = serde_json::from_str(&result.content).expect("json body");
    let task_id = body["task_id"].as_str().expect("task_id");

    assert_eq!(body["status"], "async_launched");
    assert_eq!(body["task_type"], "local_agent");
    assert_eq!(body["assistant_auto_backgrounded"], true);
    assert_eq!(body["auto_background_after_ms"], 5);
    assert!(harness.task_store.find_by_name("auto-agent-1").is_some());

    harness
        .wait_for_state(task_id, AsyncTaskState::Completed)
        .await;
    let task_output = harness.read_task_output(task_id, 0).await;
    assert_eq!(task_output["task_type"], "local_agent");
    assert!(
        task_output["lines"].to_string().contains("long-result"),
        "task_output should include final subagent output"
    );

    let notifications = harness
        .notif_queue
        .drain_for_session(&SessionId::new(TEST_SESSION_ID));
    assert_eq!(notifications.len(), 1);
    assert!(notifications[0].xml.contains(task_id));
    assert!(notifications[0].xml.contains("<status>completed</status>"));
    assert_eq!(
        harness
            .task_store
            .find_by_id(&AgentId::new(task_id))
            .expect("handle")
            .state,
        AsyncTaskState::Completed
    );
}

#[tokio::test]
async fn task_stop_cancels_promoted_local_agent_task() {
    let harness = AutoBackgroundHarness::new(500, "never-finished");

    let result = harness
        .execute(json!({
            "subagent_type": "explore",
            "prompt": "long",
            "description": "long",
            "_auto_background_after_ms": 10,
        }))
        .await;
    let body: Value = serde_json::from_str(&result.content).expect("json body");
    let task_id = body["task_id"].as_str().expect("task_id");

    let stop_result = harness.stop_task(task_id).await;
    assert!(
        stop_result.content.contains("Successfully stopped task"),
        "TaskStop should acknowledge cancellation"
    );
    assert_eq!(
        harness
            .task_store
            .find_by_id(&AgentId::new(task_id))
            .expect("handle")
            .state,
        AsyncTaskState::Killed
    );
}

#[tokio::test]
async fn parent_cancel_before_promotion_cancels_child_without_registering_task() {
    let harness = AutoBackgroundHarness::new(80, "never-returned");

    let cancel = CancellationToken::new();
    let future = harness.execute_with_cancel(
        cancel.clone(),
        json!({
            "subagent_type": "explore",
            "prompt": "cancel",
            "description": "cancel",
            "_auto_background_after_ms": 200,
        }),
    );
    cancel.cancel();

    let err = future
        .await
        .expect_err("parent cancellation should fail foreground wait");
    assert!(
        err.to_string().contains("cancel"),
        "error should mention cancellation, got: {err}"
    );
    assert!(
        harness.task_store.list_active().is_empty(),
        "cancel before promotion must not leave a visible background task"
    );
}
