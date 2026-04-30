//! Task V2 RuntimeTools — TaskCreate, TaskUpdate, TaskList.
//!
//! Mirrors claude-code-best Task V2 tools, backed by ~/.renlijia/tasks/<taskListId>/.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::runtime::task::task_models::{TaskRecord, TaskStatus};
use crate::runtime::task::FileTaskV2Store;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct TaskCreateRuntimeTool;
pub struct TaskUpdateRuntimeTool;
pub struct TaskListRuntimeTool;

fn task_list_id(ctx: &ToolExecutionContext) -> String {
    ctx.session_id.as_str().to_string()
}

fn store_for(ctx: &ToolExecutionContext) -> Result<FileTaskV2Store, ToolError> {
    let root = ctx
        .task_store_root
        .clone()
        .or_else(|| {
            ctx.capability
                .as_ref()
                .and_then(|c| c.storage.as_ref())
                .map(|s| s.workspace_path.clone())
        })
        .or_else(default_aijia_home)
        .ok_or_else(|| ToolError::ExecutionFailed("Task tools require a storage root".into()))?;
    Ok(FileTaskV2Store::new(root))
}

fn default_aijia_home() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".renlijia"))
}

fn required_str<'a>(input: &'a Value, key: &str, tool: &str) -> Result<&'a str, ToolError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ToolError::InputValidationError {
            tool_name: tool.to_string(),
            message: format!("missing required string field `{}`", key),
        })
}

fn parse_status(status: &str) -> Result<TaskStatus, ToolError> {
    match status {
        "pending" => Ok(TaskStatus::Pending),
        "in_progress" => Ok(TaskStatus::InProgress),
        "completed" => Ok(TaskStatus::Completed),
        other => Err(ToolError::InputValidationError {
            tool_name: "TaskUpdate".into(),
            message: format!("unsupported task status `{}`", other),
        }),
    }
}

fn task_to_json(task: &TaskRecord) -> Value {
    json!({
        "id": task.id,
        "subject": task.subject,
        "description": task.description,
        "activeForm": task.active_form,
        "owner": task.owner,
        "status": task.status.as_str(),
        "blocks": task.blocks,
        "blockedBy": task.blocked_by,
        "metadata": task.metadata,
    })
}

fn format_task_line(task: &TaskRecord) -> String {
    let mut line = format!("#{} [{}] {}", task.id, task.status.as_str(), task.subject);
    if let Some(owner) = &task.owner {
        line.push_str(&format!(" ({})", owner));
    }
    if !task.blocked_by.is_empty() {
        let blockers = task
            .blocked_by
            .iter()
            .map(|id| format!("#{}", id))
            .collect::<Vec<_>>()
            .join(", ");
        line.push_str(&format!(" [blocked by {}]", blockers));
    }
    line
}

#[async_trait]
impl RuntimeTool for TaskCreateRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("TaskCreate")
            .unwrap_or_else(|| ToolDefinition::new("TaskCreate", "创建任务"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let subject = required_str(&input, "subject", "TaskCreate")?.to_string();
        let description = required_str(&input, "description", "TaskCreate")?.to_string();
        let active_form = input
            .get("activeForm")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let metadata = input.get("metadata").and_then(|v| v.as_object()).map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<HashMap<_, _>>()
        });
        let store = store_for(&ctx)?;
        let list_id = task_list_id(&ctx);
        let id = store
            .next_id(&list_id)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let task = TaskRecord {
            id: id.clone(),
            subject: subject.clone(),
            description,
            active_form,
            owner: ctx.agent_id.as_ref().map(|id| id.as_str().to_string()),
            status: TaskStatus::Pending,
            blocks: vec![],
            blocked_by: vec![],
            metadata,
            session_id: ctx.session_id.clone(),
            parent_run_id: ctx.run_id.clone(),
            owner_agent_id: ctx.agent_id.clone(),
        };
        store
            .create(&list_id, &task)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ToolResult::new(
            "TaskCreate",
            format!("Task #{} created successfully: {}", id, subject),
            Some(json!({ "success": true, "taskId": id, "task": task_to_json(&task) })),
        ))
    }
}

#[async_trait]
impl RuntimeTool for TaskListRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("TaskList")
            .unwrap_or_else(|| ToolDefinition::new("TaskList", "列出任务"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }
    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        _input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let store = store_for(&ctx)?;
        let list_id = task_list_id(&ctx);
        let tasks = store
            .list(&list_id)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        if tasks.is_empty() {
            return Ok(ToolResult::new(
                "TaskList",
                "No tasks found",
                Some(json!({ "tasks": [] })),
            ));
        }
        let content = tasks
            .iter()
            .map(format_task_line)
            .collect::<Vec<_>>()
            .join("\n");
        let data = json!({ "tasks": tasks.iter().map(task_to_json).collect::<Vec<_>>() });
        Ok(ToolResult::new("TaskList", content, Some(data)))
    }
}

#[async_trait]
impl RuntimeTool for TaskUpdateRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("TaskUpdate")
            .unwrap_or_else(|| ToolDefinition::new("TaskUpdate", "更新任务"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let store = store_for(&ctx)?;
        let list_id = task_list_id(&ctx);
        let task_id = required_str(&input, "taskId", "TaskUpdate")?;

        let existing = store
            .get(&list_id, task_id)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let Some(mut task) = existing else {
            return Ok(ToolResult::new(
                "TaskUpdate",
                format!("Task #{} not found", task_id),
                Some(json!({
                    "success": false,
                    "taskId": task_id,
                    "updatedFields": [],
                    "error": "Task not found"
                })),
            ));
        };

        let mut updated_fields = Vec::<String>::new();
        let old_status = task.status.clone();

        if let Some(status) = input.get("status").and_then(|v| v.as_str()) {
            if status == "deleted" {
                let deleted = store
                    .delete(&list_id, task_id)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                return Ok(ToolResult::new(
                    "TaskUpdate",
                    if deleted {
                        format!("Deleted task #{}", task_id)
                    } else {
                        format!("Task #{} not found", task_id)
                    },
                    Some(json!({
                        "success": deleted,
                        "taskId": task_id,
                        "updatedFields": if deleted { vec!["deleted"] } else { vec![] },
                        "statusChange": if deleted { json!({"from": old_status.as_str(), "to": "deleted"}) } else { Value::Null }
                    })),
                ));
            }
            let new_status = parse_status(status)?;
            if task.status != new_status {
                task.status = new_status;
                updated_fields.push("status".into());
            }
        }

        if let Some(subject) = input.get("subject").and_then(|v| v.as_str()) {
            if task.subject != subject {
                task.subject = subject.to_string();
                updated_fields.push("subject".into());
            }
        }
        if let Some(description) = input.get("description").and_then(|v| v.as_str()) {
            if task.description != description {
                task.description = description.to_string();
                updated_fields.push("description".into());
            }
        }
        if let Some(active_form) = input.get("activeForm").and_then(|v| v.as_str()) {
            if task.active_form.as_deref() != Some(active_form) {
                task.active_form = Some(active_form.to_string());
                updated_fields.push("activeForm".into());
            }
        }
        if let Some(owner) = input.get("owner").and_then(|v| v.as_str()) {
            if task.owner.as_deref() != Some(owner) {
                task.owner = Some(owner.to_string());
                updated_fields.push("owner".into());
            }
        }
        if let Some(add_blocks) = input.get("addBlocks").and_then(|v| v.as_array()) {
            for block_id in add_blocks.iter().filter_map(|v| v.as_str()) {
                if !task.blocks.iter().any(|id| id == block_id) {
                    task.blocks.push(block_id.to_string());
                    updated_fields.push("blocks".into());
                }
            }
        }
        if let Some(add_blocked_by) = input.get("addBlockedBy").and_then(|v| v.as_array()) {
            for blocker_id in add_blocked_by.iter().filter_map(|v| v.as_str()) {
                if !task.blocked_by.iter().any(|id| id == blocker_id) {
                    task.blocked_by.push(blocker_id.to_string());
                    updated_fields.push("blockedBy".into());
                }
            }
        }
        if let Some(metadata) = input.get("metadata").and_then(|v| v.as_object()) {
            let mut merged = task.metadata.take().unwrap_or_default();
            for (key, value) in metadata {
                if value.is_null() {
                    merged.remove(key);
                } else {
                    merged.insert(key.clone(), value.clone());
                }
            }
            task.metadata = Some(merged);
            updated_fields.push("metadata".into());
        }

        store
            .update(&list_id, &task)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let status_change = if old_status != task.status {
            Some(json!({ "from": old_status.as_str(), "to": task.status.as_str() }))
        } else {
            None
        };

        Ok(ToolResult::new(
            "TaskUpdate",
            format!("Updated task #{} {}", task_id, updated_fields.join(", ")),
            Some(json!({
                "success": true,
                "taskId": task_id,
                "updatedFields": updated_fields,
                "statusChange": status_change,
                "task": task_to_json(&task),
            })),
        ))
    }
}
