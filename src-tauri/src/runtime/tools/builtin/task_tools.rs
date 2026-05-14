//! Task V2 RuntimeTools — TaskCreate, TaskUpdate, TaskList, TaskGet, TaskClaim.
//!
//! Tasks are stored per-conversation under
//! `<home>/conversations/<conv_id>/tasks/<task_list_id>/`.
//! See P1.5 path-migration for rationale.

use std::collections::HashMap;
use std::collections::HashSet;
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
use crate::telemetry::{record_diagnostic, DiagnosticEvent, DiagnosticSource};

/// Best-effort task-notification emitter for Team mode (P2.5).  Resolves the
/// actor name via `AgentNameRegistry::name_for(ctx.agent_id)`; falls back to
/// `"unknown-actor"` when nothing is registered.  Silently no-ops when LTR
/// registries are not wired into the ctx (legacy / test paths).  Failures
/// are logged at debug level — the task operation has already succeeded and
/// must not be undone by a notification glitch.
async fn try_notify_lead(
    ctx: &ToolExecutionContext,
    task_id: &str,
    action: crate::runtime::agent::task_notification_lead::TaskAction,
    subject: &str,
    status: &str,
) {
    use crate::runtime::agent::task_notification_lead::{emit_to_lead, TaskNotificationDeps};

    let (Some(team_reg), Some(name_reg), Some(inbox_reg)) = (
        ctx.team_registry.clone(),
        ctx.agent_names.clone(),
        ctx.inbox_registry.clone(),
    ) else {
        return;
    };
    let actor_name = if let Some(aid) = ctx.agent_id.as_ref() {
        name_reg
            .name_for(&ctx.session_id, aid)
            .await
            .unwrap_or_else(|| "unknown-actor".into())
    } else {
        "unknown-actor".into()
    };
    let deps = TaskNotificationDeps {
        team_registry: team_reg,
        agent_names: name_reg,
        inbox_registry: inbox_reg,
        lead_idle: ctx.lead_idle.clone(),
    };
    let outcome = emit_to_lead(
        &deps,
        &ctx.session_id,
        &actor_name,
        task_id,
        action,
        subject,
        status,
    )
    .await;
    log::debug!(
        "[task_tools] notify_lead task={task_id} action={:?} outcome={:?}",
        action,
        outcome
    );
}

pub struct TaskCreateRuntimeTool;
pub struct TaskUpdateRuntimeTool;
pub struct TaskListRuntimeTool;
pub struct TaskGetRuntimeTool;
pub struct TaskClaimRuntimeTool;

fn task_list_id(_ctx: &ToolExecutionContext) -> String {
    // P1.5 follow-up: store is already rooted at the per-conversation
    // `tasks/` directory, so we pass an empty list id and let the store
    // keep task files flat at `<root>/<id>.json`.  Previously this
    // returned the session id, producing a redundant
    // `<root>/<conv_id>/<id>.json` second level.
    String::new()
}

fn store_for(ctx: &ToolExecutionContext) -> Result<FileTaskV2Store, ToolError> {
    // 生产路径：用 TeamPaths 派生 tasks 目录。
    // 若有 active_team_name，写 teams/{name}/tasks/；否则写 conv 根 tasks/。
    if let Some(conv_dir) = ctx.conv_dir.as_ref() {
        use crate::runtime::agent::team_paths::TeamPaths;
        let paths = match ctx.active_team_name.as_deref() {
            Some(name) => TeamPaths::for_team(conv_dir, name),
            None => TeamPaths::for_conv(conv_dir),
        };
        return Ok(FileTaskV2Store::new(paths.tasks_dir()));
    }

    // Fallback：单测或老代码路径未注入 conv_dir 时，沿用原始拼接逻辑。
    let home = ctx
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
    // P1.5: scope task storage to the current conversation directory so that
    // tasks are session-local artifacts rather than global state.
    let conv_id = ctx.session_id.as_str();
    let tasks_root = home.join("conversations").join(conv_id).join("tasks");
    Ok(FileTaskV2Store::new(tasks_root))
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
    fn id(&self) -> &str { "TaskCreate" }
    
    async fn definition(&self, _ctx: &crate::runtime::tools::ToolDescriptionContext) -> ToolDefinition {
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
        try_notify_lead(
            &ctx,
            &id,
            crate::runtime::agent::task_notification_lead::TaskAction::Created,
            &subject,
            "pending",
        )
        .await;
        Ok(ToolResult::new(
            "TaskCreate",
            format!("Task #{} created successfully: {}", id, subject),
            Some(json!({ "success": true, "taskId": id, "task": task_to_json(&task) })),
        ))
    }
}

#[async_trait]
impl RuntimeTool for TaskListRuntimeTool {
    fn id(&self) -> &str { "TaskList" }
    
    async fn definition(&self, _ctx: &crate::runtime::tools::ToolDescriptionContext) -> ToolDefinition {
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

// ── Cycle detection for addBlocks / addBlockedBy ──────────────────────────────

/// Check whether adding `new_edges` (each `(blocker, blocked)` in the `blocks`
/// direction, i.e. `blocker → blocked`) would introduce a cycle.
///
/// Returns `Err(String)` with the cycle path if a cycle is detected, `Ok(())`
/// otherwise.
///
/// `existing_tasks` is the full task list for the conversation (before the
/// proposed edges are applied).
fn check_no_cycle(
    existing_tasks: &[TaskRecord],
    new_edges: &[(&str, &str)], // (blocker_id, blocked_id) — "A blocks B" = A→B
) -> Result<(), String> {
    // Build adjacency map: node → set of nodes it blocks (A → {B, C, ...})
    let mut adj: HashMap<&str, HashSet<&str>> = HashMap::new();
    for task in existing_tasks {
        let entry = adj.entry(task.id.as_str()).or_default();
        for blocked in &task.blocks {
            entry.insert(blocked.as_str());
        }
        // Also reconstruct forward edges from blocked_by (B.blocked_by contains A → A→B)
        for blocker in &task.blocked_by {
            adj.entry(blocker.as_str())
                .or_default()
                .insert(task.id.as_str());
        }
    }

    // Apply proposed edges to the temporary graph.
    for (blocker, blocked) in new_edges {
        adj.entry(blocker).or_default().insert(blocked);
    }

    // For each new edge (A→B): check whether B can reach A (which would form A→B→...→A).
    // Also handle self-loop: A→A is immediately a cycle.
    for (blocker, blocked) in new_edges {
        if blocker == blocked {
            return Err(format!(
                "cyclic blocking dependency: {} → {} (self-block)",
                blocker, blocked
            ));
        }
        // DFS from `blocked` looking for `blocker`.  The returned path starts at
        // `blocked` and ends at `blocker`, so prepending `blocker →` gives the
        // full cycle: blocker → blocked → ... → blocker.
        if let Some(cycle_path) = dfs_find_cycle_path(&adj, blocked, blocker) {
            let full_path = format!("{} → {}", blocker, cycle_path);
            return Err(format!("cyclic blocking dependency: {}", full_path));
        }
    }

    Ok(())
}

/// DFS from `start` looking for `target`.  Returns the path from `start` to
/// `target` (inclusive of both endpoints) as a `" → "` separated string, or
/// `None` if no path exists.
fn dfs_find_cycle_path<'a>(
    adj: &HashMap<&'a str, HashSet<&'a str>>,
    start: &'a str,
    target: &'a str,
) -> Option<String> {
    let mut visited: HashSet<&str> = HashSet::new();
    let mut path: Vec<&str> = Vec::new();
    if dfs_inner(adj, start, target, &mut visited, &mut path) {
        Some(path.iter().copied().collect::<Vec<_>>().join(" → "))
    } else {
        None
    }
}

fn dfs_inner<'a>(
    adj: &HashMap<&'a str, HashSet<&'a str>>,
    current: &'a str,
    target: &'a str,
    visited: &mut HashSet<&'a str>,
    path: &mut Vec<&'a str>,
) -> bool {
    if current == target {
        path.push(current);
        return true;
    }
    if visited.contains(current) {
        return false;
    }
    visited.insert(current);
    if let Some(neighbors) = adj.get(current) {
        for &next in neighbors {
            if dfs_inner(adj, next, target, visited, path) {
                path.insert(0, current);
                return true;
            }
        }
    }
    false
}

#[async_trait]
impl RuntimeTool for TaskUpdateRuntimeTool {
    fn id(&self) -> &str { "TaskUpdate" }
    
    async fn definition(&self, _ctx: &crate::runtime::tools::ToolDescriptionContext) -> ToolDefinition {
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
            // Collect proposed new edges (skip duplicates already in task.blocks).
            let proposed_blocks: Vec<&str> = add_blocks
                .iter()
                .filter_map(|v| v.as_str())
                .filter(|&block_id| !task.blocks.iter().any(|id| id == block_id))
                .collect();

            // Cycle detection: A (task_id) blocks B → edge (task_id → B).
            // Load full task list for graph construction.
            if !proposed_blocks.is_empty() {
                let all_tasks = store
                    .list(&list_id)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                let new_edges: Vec<(&str, &str)> = proposed_blocks
                    .iter()
                    .map(|&b| (task_id, b))
                    .collect();
                check_no_cycle(&all_tasks, &new_edges)
                    .map_err(|msg| ToolError::ExecutionFailed(msg))?;
            }

            for block_id in proposed_blocks {
                task.blocks.push(block_id.to_string());
                updated_fields.push("blocks".into());
            }
        }
        if let Some(add_blocked_by) = input.get("addBlockedBy").and_then(|v| v.as_array()) {
            // Collect proposed new edges (skip duplicates already in task.blocked_by).
            let proposed_blockers: Vec<&str> = add_blocked_by
                .iter()
                .filter_map(|v| v.as_str())
                .filter(|&blocker_id| !task.blocked_by.iter().any(|id| id == blocker_id))
                .collect();

            // Cycle detection: B (blocker_id) blocks A (task_id) → edge (B → task_id).
            // Load full task list for graph construction (may already be loaded above; load
            // again if not — store reads are cheap in tests and rare in production).
            if !proposed_blockers.is_empty() {
                let all_tasks = store
                    .list(&list_id)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                let new_edges: Vec<(&str, &str)> = proposed_blockers
                    .iter()
                    .map(|&b| (b, task_id))
                    .collect();
                check_no_cycle(&all_tasks, &new_edges)
                    .map_err(|msg| ToolError::ExecutionFailed(msg))?;
            }

            for blocker_id in proposed_blockers {
                task.blocked_by.push(blocker_id.to_string());
                updated_fields.push("blockedBy".into());
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

        notify_after_update(&ctx, &task_id, &task.subject, task.status.as_str()).await;
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

async fn notify_after_update(
    ctx: &ToolExecutionContext,
    task_id: &str,
    subject: &str,
    status: &str,
) {
    try_notify_lead(
        ctx,
        task_id,
        crate::runtime::agent::task_notification_lead::TaskAction::Updated,
        subject,
        status,
    )
    .await;
}

#[async_trait]
impl RuntimeTool for TaskGetRuntimeTool {
    fn id(&self) -> &str { "TaskGet" }
    
    async fn definition(&self, _ctx: &crate::runtime::tools::ToolDescriptionContext) -> ToolDefinition {
        TOOL_CATALOG
            .get("TaskGet")
            .unwrap_or_else(|| ToolDefinition::new("TaskGet", "获取单条任务"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let task_id = required_str(&input, "taskId", "TaskGet")?.to_string();
        let store = store_for(&ctx)?;
        let list_id = task_list_id(&ctx);
        let task = store
            .get(&list_id, &task_id)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        match task {
            Some(task) => Ok(ToolResult::new(
                "TaskGet",
                format_task_line(&task),
                Some(json!({ "task": task_to_json(&task) })),
            )),
            None => Ok(ToolResult::new(
                "TaskGet",
                format!("Task #{} not found", task_id),
                Some(json!({ "task": null, "error": "Task not found" })),
            )),
        }
    }
}

// ── TaskClaim ──────────────────────────────────────────────────────────────────

#[async_trait]
impl RuntimeTool for TaskClaimRuntimeTool {
    fn id(&self) -> &str { "TaskClaim" }
    
    async fn definition(&self, _ctx: &crate::runtime::tools::ToolDescriptionContext) -> ToolDefinition {
        TOOL_CATALOG.get("TaskClaim").unwrap_or_else(|| {
            ToolDefinition::new(
                "TaskClaim",
                "Claim a task whose owner is None or \"*\". Sets owner to your agent name. \
                 Idempotent if you already own it. Fails if someone else owns it.",
            )
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let task_id = required_str(&input, "taskId", "TaskClaim")?.to_string();
        let store = store_for(&ctx)?;
        let list_id = task_list_id(&ctx);

        let ws = crate::telemetry::diagnostics_workspace();
        record_diagnostic(
            &ws,
            DiagnosticEvent::new("tool.task_claim.entry", DiagnosticSource::Backend)
                .conversation_id(ctx.session_id.as_str())
                .run_id(ctx.run_id.as_str())
                .tool_call_id(ctx.tool_call_id.as_str())
                .payload(serde_json::json!({ "task_id": task_id })),
        );

        let existing = store
            .get(&list_id, &task_id)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let Some(mut task) = existing else {
            return Err(ToolError::ExecutionFailed(format!(
                "Task #{} not found",
                task_id
            )));
        };

        // Resolve caller identity: prefer agent_id string representation.
        let caller = ctx
            .agent_id
            .as_ref()
            .map(|id| id.as_str().to_string())
            .unwrap_or_else(|| "unknown-agent".to_string());

        // Check ownership without holding a borrow into task.owner across the mutation.
        enum ClaimDecision {
            Claim,
            AlreadyOwned,
            Taken(String),
        }
        let decision = match &task.owner {
            None => ClaimDecision::Claim,
            Some(owner) if owner == "*" => ClaimDecision::Claim,
            Some(owner) if *owner == caller => ClaimDecision::AlreadyOwned,
            Some(owner) => ClaimDecision::Taken(owner.clone()),
        };

        match decision {
            ClaimDecision::Claim => {
                task.owner = Some(caller.clone());
            }
            ClaimDecision::AlreadyOwned => {
                record_diagnostic(
                    &ws,
                    DiagnosticEvent::new("tool.task_claim.already_owned", DiagnosticSource::Backend)
                        .conversation_id(ctx.session_id.as_str())
                        .run_id(ctx.run_id.as_str())
                        .tool_call_id(ctx.tool_call_id.as_str())
                        .ok(true)
                        .payload(serde_json::json!({ "task_id": task_id, "caller": caller })),
                );
                return Ok(ToolResult::new(
                    "TaskClaim",
                    format!("Task #{} already owned by you ({})", task_id, caller),
                    Some(json!({ "success": true, "taskId": task_id, "task": task_to_json(&task) })),
                ));
            }
            ClaimDecision::Taken(existing_owner) => {
                record_diagnostic(
                    &ws,
                    DiagnosticEvent::new("tool.task_claim.already_claimed", DiagnosticSource::Backend)
                        .conversation_id(ctx.session_id.as_str())
                        .run_id(ctx.run_id.as_str())
                        .tool_call_id(ctx.tool_call_id.as_str())
                        .ok(false)
                        .payload(serde_json::json!({ "task_id": task_id, "existing_owner": existing_owner })),
                );
                return Err(ToolError::ExecutionFailed(format!(
                    "task already claimed by '{}'",
                    existing_owner
                )));
            }
        }

        store
            .update(&list_id, &task)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        try_notify_lead(
            &ctx,
            &task_id,
            crate::runtime::agent::task_notification_lead::TaskAction::Claimed,
            &task.subject,
            task.status.as_str(),
        )
        .await;

        record_diagnostic(
            &ws,
            DiagnosticEvent::new("tool.task_claim.completed", DiagnosticSource::Backend)
                .conversation_id(ctx.session_id.as_str())
                .run_id(ctx.run_id.as_str())
                .tool_call_id(ctx.tool_call_id.as_str())
                .ok(true)
                .payload(serde_json::json!({ "task_id": task_id, "caller": caller })),
        );

        Ok(ToolResult::new(
            "TaskClaim",
            format!("Task #{} claimed by {}", task_id, caller),
            Some(json!({ "success": true, "taskId": task_id, "task": task_to_json(&task) })),
        ))
    }
}
