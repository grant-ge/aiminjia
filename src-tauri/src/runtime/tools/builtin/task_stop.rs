//! TaskStop runtime tool — terminates a running async sub-agent
//! by task_id (which equals the agent_id returned by Agent(run_in_background=true)).

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::runtime::agent::async_task_store::{
    AsyncAgentTaskStore, AsyncTaskState, AsyncTaskType,
};
use crate::runtime::cancellation::CancellationReason;
use crate::runtime::ids::AgentId;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct TaskStopRuntimeTool {
    pub store: Arc<AsyncAgentTaskStore>,
}

#[async_trait]
impl RuntimeTool for TaskStopRuntimeTool {
    fn id(&self) -> &str {
        "TaskStop"
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        TOOL_CATALOG
            .get("TaskStop")
            .unwrap_or_else(|| ToolDefinition::new("TaskStop", "终止一个正在后台运行的 Agent 任务"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let task_id = input
            .get("task_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| ToolError::InputValidationError {
                tool_name: "TaskStop".into(),
                message: "missing required string field `task_id`".into(),
            })?
            .to_string();
        let requested_task_type = input
            .get("task_type")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|value| {
                AsyncTaskType::parse(value).ok_or_else(|| ToolError::InputValidationError {
                    tool_name: "TaskStop".into(),
                    message: format!(
                        "unsupported task_type `{value}`; expected local_agent or local_bash"
                    ),
                })
            })
            .transpose()?;

        let agent_id = AgentId::new(&task_id);
        let handle = self.store.find_by_id(&agent_id).ok_or_else(|| {
            ToolError::ExecutionFailed(format!("No task found with ID: {task_id}"))
        })?;
        let task_type = self
            .store
            .task_type_for_id(&agent_id)
            .unwrap_or(AsyncTaskType::LocalAgent);
        if let Some(requested) = requested_task_type {
            if requested != task_type {
                return Err(ToolError::ExecutionFailed(format!(
                    "Task {task_id} has type {}, not {}",
                    task_type.as_str(),
                    requested.as_str()
                )));
            }
        }

        if handle.state.is_terminal() {
            return Err(ToolError::ExecutionFailed(format!(
                "Task {task_id} is not running (status: {:?})",
                handle.state
            )));
        }

        handle
            .cancel_token
            .cancel_with_reason(CancellationReason::BackgroundStop);
        self.store.update_state(&agent_id, AsyncTaskState::Killed);

        let description = handle.description.clone();
        Ok(ToolResult::new(
            "TaskStop",
            format!("Successfully stopped task: {task_id} ({description})"),
            Some(json!({
                "message": format!("Successfully stopped task: {task_id}"),
                "task_id": task_id,
                "task_type": task_type.as_str(),
                "command": description,
            })),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::agent::async_task_store::AsyncTaskHandle;
    use crate::runtime::cancellation::CancellationToken;
    use serde_json::json;

    fn make_handle(task_id: &str) -> AsyncTaskHandle {
        AsyncTaskHandle {
            agent_id: AgentId::new(task_id),
            state: AsyncTaskState::Running,
            output_file: std::path::PathBuf::from(format!("/tmp/{task_id}.jsonl")),
            description: "background shell".to_string(),
            cancel_token: CancellationToken::new(),
        }
    }

    #[tokio::test]
    async fn stops_local_bash_task_and_reports_task_type() {
        let store = Arc::new(AsyncAgentTaskStore::new());
        store.register_anonymous_with_type(make_handle("btest123"), AsyncTaskType::LocalBash);
        let tool = TaskStopRuntimeTool {
            store: store.clone(),
        };

        let result = tool
            .execute(
                json!({"task_id": "btest123", "task_type": "local_shell"}),
                ToolExecutionContext::for_test("c", "r", "tc"),
            )
            .await
            .unwrap();
        let data = result.data.unwrap();
        assert_eq!(data["task_type"].as_str(), Some("local_bash"));
        assert_eq!(
            store
                .find_by_id(&AgentId::new("btest123"))
                .unwrap()
                .state,
            AsyncTaskState::Killed
        );
    }
}
