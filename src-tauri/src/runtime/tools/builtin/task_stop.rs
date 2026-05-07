//! TaskStop runtime tool — terminates a running async sub-agent
//! by task_id (which equals the agent_id returned by Agent(run_in_background=true)).

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::runtime::agent::async_task_store::{AsyncAgentTaskStore, AsyncTaskState};
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
    fn definition(&self) -> ToolDefinition {
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

        let agent_id = AgentId::new(&task_id);
        let handle = self.store.find_by_id(&agent_id).ok_or_else(|| {
            ToolError::ExecutionFailed(format!("No task found with ID: {task_id}"))
        })?;

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
                "task_type": "local_agent",
                "command": description,
            })),
        ))
    }
}
