//! Todo handlers — list tasks, create task, mark done.
//!
//! Response shape: { result: { todoCards: [{ subject, taskId, dueTime(ms), priority, ... }] } }

use anyhow::Result;
use serde_json::Value;

use super::super::{optional_i64, optional_str, require_str};
use super::get_bridge;
use crate::plugin::context::PluginContext;

/// List todos. Response: result.todoCards[]
pub async fn handle_dingtalk_list_todos(ctx: &PluginContext, args: &Value) -> Result<String> {
    let bridge = get_bridge(ctx).await?;
    let limit = optional_i64(args, "limit", 20);
    let status = optional_str(args, "status");

    let size_str = limit.to_string();
    let mut cmd_args = vec!["todo", "task", "list", "--size", &size_str];

    if let Some(s) = status {
        let dws_status = match s {
            "done" | "completed" | "true" => "true",
            "open" | "pending" | "false" => "false",
            _ => s,
        };
        cmd_args.extend(["--status", dws_status]);
    }

    let result = bridge.query(&cmd_args).await?;

    // result.todoCards[]
    let tasks = result
        .get("result")
        .and_then(|r| r.get("todoCards"))
        .and_then(|t| t.as_array());

    if let Some(tasks) = tasks {
        if tasks.is_empty() {
            return Ok("No todo tasks found.".into());
        }
        let mut output = format!("Found {} task(s):\n\n", tasks.len());
        for t in tasks {
            let subject = t
                .get("subject")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled");
            let tid = t.get("taskId").and_then(|v| v.as_str()).unwrap_or("?");
            // dueTime is milliseconds timestamp
            let due_ms = t.get("dueTime").and_then(|v| v.as_i64()).unwrap_or(0);
            let due_str = if due_ms > 0 {
                chrono::DateTime::from_timestamp_millis(due_ms)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_default()
            } else {
                String::new()
            };
            // finalStatusStage: 0=not started, 1=in progress, 2=completed
            let stage = t
                .get("finalStatusStage")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let status_icon = if stage == 2 { "[done]" } else { "[open]" };
            let priority = t.get("priority").and_then(|v| v.as_i64()).unwrap_or(0);
            let priority_str = match priority {
                40 => " [urgent]",
                30 => " [high]",
                20 => " [normal]",
                10 => " [low]",
                _ => "",
            };

            output.push_str(&format!(
                "{} **{}**{} (task_id: `{}`)",
                status_icon, subject, priority_str, tid
            ));
            if !due_str.is_empty() {
                output.push_str(&format!(" — due: {}", due_str));
            }
            output.push('\n');
        }
        Ok(output)
    } else {
        Ok(format!(
            "Tasks:\n```json\n{}\n```",
            serde_json::to_string_pretty(&result)?
        ))
    }
}

/// Create todo. dws: todo task create --title X --executors Y --due Z --priority 40
pub async fn handle_dingtalk_create_todo(ctx: &PluginContext, args: &Value) -> Result<String> {
    let bridge = get_bridge(ctx).await?;
    let subject = require_str(args, "subject")?;
    let due_time = optional_str(args, "due_time");
    let executors = optional_str(args, "executor_user_ids");
    let priority = optional_i64(args, "priority", 0);

    let mut cmd_args = vec!["todo", "task", "create", "--title", subject];

    if let Some(due) = due_time {
        cmd_args.extend(["--due", due]);
    }
    if let Some(exec) = executors {
        cmd_args.extend(["--executors", exec]);
    }
    let priority_str = priority.to_string();
    if priority > 0 {
        cmd_args.extend(["--priority", &priority_str]);
    }

    let result = bridge.mutate(&cmd_args).await?;

    let task_id = result
        .get("result")
        .and_then(|r| r.get("id").or_else(|| r.get("taskId")))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let priority_label = match priority {
        40 => " (urgent)",
        30 => " (high)",
        20 => " (normal)",
        10 => " (low)",
        _ => "",
    };

    Ok(format!(
        "Todo created (task_id: `{}`).\n\n**{}**{}\n{}{}",
        task_id,
        subject,
        priority_label,
        due_time
            .map(|d| format!("Due: {}\n", d))
            .unwrap_or_default(),
        executors
            .map(|e| format!("Executors: {}\n", e))
            .unwrap_or_default(),
    ))
}

/// Mark todo done. dws: todo task done --task-id X --status true
pub async fn handle_dingtalk_complete_todo(ctx: &PluginContext, args: &Value) -> Result<String> {
    let bridge = get_bridge(ctx).await?;
    let task_id = require_str(args, "task_id")?;

    bridge
        .mutate(&[
            "todo",
            "task",
            "done",
            "--task-id",
            task_id,
            "--status",
            "true",
        ])
        .await?;

    Ok(format!("Task `{}` marked as complete.", task_id))
}

#[cfg(test)]
mod tests {
    use crate::llm::tool_executor::require_str;
    use serde_json::json;

    #[test]
    fn test_require_subject() {
        let args = json!({"subject": "Review PR"});
        assert_eq!(require_str(&args, "subject").unwrap(), "Review PR");
    }

    #[test]
    fn test_require_task_id() {
        let args = json!({"task_id": "task_abc123"});
        assert_eq!(require_str(&args, "task_id").unwrap(), "task_abc123");
    }
}
