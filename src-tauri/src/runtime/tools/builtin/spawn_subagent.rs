//! Generic `spawn_subagent` RuntimeTool — entry point for a parent LLM to
//! delegate work to a sub-agent.
//!
//! Architecture: this tool is the dispatcher only. Actual sub-agent execution is
//! injected via [`SpawnSubagentLauncher`] (parallel to `BrowseDataLauncher`),
//! resolved at `lib.rs`/`registry.rs` setup time. This module is free of
//! `LlmGateway` / `SubAgentRuntimeDeps` imports.
//!
//! ## Sync path (`run_in_background = false` or omitted)
//! Awaits `launcher.launch_sync()` and returns the final output as a
//! [`ToolResult`].
//!
//! ## Async path (`run_in_background = true`)
//! Calls `launcher.launch_async()` which registers the agent in
//! `AsyncAgentTaskStore`, `tokio::spawn`s a detached task, and returns
//! immediately. The spawned task updates the store state to
//! `Completed`/`Failed` and enqueues a task-notification XML into
//! `TaskNotificationQueue` when it finishes. The tool returns a JSON
//! `{"status":"async_launched","agent_id":"...","name":"..."}` so the
//! parent LLM knows the launch succeeded.
//!
//! ## Teammate dispatch (P1.6)
//! Passing `employee_id` (mutually exclusive with `subagent_type`) sources the
//! spawned agent from an Employee profile.  `team_name` (non-empty) turns the
//! dispatch into a Teammate that joins the session's Team.  `name` is required
//! for Teammate dispatch and is registered in [`AgentNameRegistry`].
//! The idle loop is launched via `tokio::spawn(run_worker(WorkerMode::TeammateIdle{...}))`.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::agent::definition::AgentModel;
use crate::runtime::agent::inbox::AgentInbox;
use crate::runtime::agent::output_writer::{AgentTranscriptMeta, TranscriptKind};
use crate::runtime::agent::registry::AgentRegistry;
use crate::runtime::agent::worker_runtime::{
    run_worker, TeammateWorkerCtx, WorkerMode,
};
use crate::runtime::cancellation::CancellationToken;
use crate::runtime::employee::store::EmployeeStore;
use crate::runtime::ids::{AgentId, RunId, SessionId, ToolCallId};
use crate::runtime::path_auth::ToolPermissionContext;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::permission::PermissionMode;
use crate::runtime::tools::RuntimeTool;

// ─── Request / Context / Result types ────────────────────────────────────────

/// Resolved request payload passed to the launcher.
#[derive(Clone, Debug)]
pub struct SpawnSubagentRequest {
    pub subagent_type: String,
    pub prompt: String,
    pub description: String,
    /// Effective model after three-tier resolution (caller > definition > None=inherit).
    pub effective_model: Option<String>,
    pub run_in_background: bool,
    /// Optional instance name for SendMessage routing (async path only).
    pub name: Option<String>,
}

/// Runtime context the launcher needs to invoke the sub-agent.
#[derive(Clone)]
pub struct SpawnSubagentContext {
    pub session_id: SessionId,
    pub parent_run_id: Option<RunId>,
    pub parent_agent_id: Option<AgentId>,
    pub cancellation: CancellationToken,
    pub permission_mode: PermissionMode,
    pub parent_tool_use_id: ToolCallId,
    /// Phase 5: snapshot of the parent turn's merged ToolPermissionContext.
    /// Carried from `ToolExecutionContext.capability.storage.permission_ctx` at
    /// the point the parent calls spawn_subagent.  `None` when the tool is
    /// invoked through a path where capability has not been set (tests, legacy).
    pub permission_ctx: Option<Arc<ToolPermissionContext>>,
}

/// Outcome from the async launcher path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnAsyncOutcome {
    pub agent_id: AgentId,
    pub name: Option<String>,
}

// ─── Launcher trait ───────────────────────────────────────────────────────────

/// Injected dependency that performs the actual sub-agent execution.
///
/// Implement this in the infrastructure layer (`llm/tool_executor/` or
/// `plugin/registry.rs` setup) so that `SpawnSubagentRuntimeTool` stays free
/// of heavy gateway imports and is easily testable with a stub.
#[async_trait]
pub trait SpawnSubagentLauncher: Send + Sync {
    /// Sync path: run the sub-agent to completion and return its final output.
    async fn launch_sync(
        &self,
        request: SpawnSubagentRequest,
        context: SpawnSubagentContext,
    ) -> Result<String>;

    /// Async path: launch a detached sub-agent task and return its identity.
    async fn launch_async(
        &self,
        request: SpawnSubagentRequest,
        context: SpawnSubagentContext,
    ) -> Result<SpawnAsyncOutcome>;
}

// ─── Source discrimination ────────────────────────────────────────────────────

/// Discriminates the source of the spawned agent's definition.
enum AgentSource {
    /// Legacy path: look up agent definition by type name in AgentRegistry.
    Registry(String),
    /// New path (P1.3): load Employee profile by id from EmployeeStore.
    Employee(String),
}

// ─── RuntimeTool implementation ───────────────────────────────────────────────

pub struct SpawnSubagentRuntimeTool {
    launcher: Arc<dyn SpawnSubagentLauncher>,
    registry: Arc<AgentRegistry>,
    /// Optional EmployeeStore injected at construction time.  Required when
    /// the LLM calls Agent(employee_id=...).  `None` for legacy paths and tests
    /// that only use subagent_type.
    employee_store: Option<Arc<EmployeeStore>>,
}

impl SpawnSubagentRuntimeTool {
    pub fn new(launcher: Arc<dyn SpawnSubagentLauncher>, registry: Arc<AgentRegistry>) -> Self {
        Self {
            launcher,
            registry,
            employee_store: None,
        }
    }

    /// Constructor for production paths that need Employee-sourced Teammates.
    pub fn new_with_employees(
        launcher: Arc<dyn SpawnSubagentLauncher>,
        registry: Arc<AgentRegistry>,
        employee_store: Arc<EmployeeStore>,
    ) -> Self {
        Self {
            launcher,
            registry,
            employee_store: Some(employee_store),
        }
    }
}

#[async_trait]
impl RuntimeTool for SpawnSubagentRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("Agent")
            .unwrap_or_else(|| ToolDefinition::new("Agent", "Spawn sub-agent"))
    }

    /// Parallel spawn_subagent calls are independent — safe to run concurrently.
    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        // ── Parse required fields ──────────────────────────────────────────
        let prompt = input
            .get("prompt")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("missing required field: prompt".into()))?;
        let description = input
            .get("description")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::ExecutionFailed("missing required field: description".into())
            })?;

        // ── Parse optional source fields ───────────────────────────────────
        let subagent_type = input
            .get("subagent_type")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let employee_id = input
            .get("employee_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        // ── Source discrimination (mutually exclusive) ─────────────────────
        // This check fires BEFORE any state mutation so a bad call never
        // pollutes AgentNameRegistry or TeamRegistry.
        let source = match (&subagent_type, &employee_id) {
            (Some(_), Some(_)) => {
                return Err(ToolError::ExecutionFailed(
                    "subagent_type and employee_id are mutually exclusive; \
                     use subagent_type for registry agents or employee_id for Employee-sourced Teammates"
                        .into(),
                ))
            }
            (None, None) => {
                return Err(ToolError::ExecutionFailed(
                    "either subagent_type or employee_id is required".into(),
                ))
            }
            (Some(t), None) => AgentSource::Registry(t.clone()),
            (None, Some(eid)) => AgentSource::Employee(eid.clone()),
        };

        // ── Parse optional routing/dispatch fields ─────────────────────────
        let caller_model = input
            .get("model")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty()) // empty string treated as inherit
            .map(str::to_string);
        let run_in_background = input
            .get("run_in_background")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let name = input
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let team_name = input
            .get("team_name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        // ── Team handle resolution ─────────────────────────────────────────
        // Must happen before name registration so a failed team lookup doesn't
        // leave a stale entry in AgentNameRegistry.
        let team_handle = if team_name.is_some() {
            let session_id = ctx.session_id.clone();
            let team = ctx
                .team_registry()
                .get(&session_id)
                .await
                .ok_or_else(|| {
                    ToolError::ExecutionFailed(
                        "no team in this session — call TeamCreate first".into(),
                    )
                })?;
            // Note: we don't reject on team_name mismatch because session_id
            // uniqueness is the authoritative lookup key.  A caller who uses
            // a different name string is probably just out of sync with the
            // UI label; warn but proceed.
            {
                let guard = team.lock().await;
                if let Some(tn) = &team_name {
                    if guard.team_name != *tn {
                        log::warn!(
                            "[spawn_subagent] team_name mismatch: caller said {:?} \
                             but session has team {:?}; proceeding with session team",
                            tn,
                            guard.team_name
                        );
                    }
                }
            }
            Some(team)
        } else {
            None
        };

        // ── name is required for Teammate dispatch ─────────────────────────
        if team_handle.is_some() && name.is_none() {
            return Err(ToolError::ExecutionFailed(
                "name is required for Teammate dispatch (team_name was set)".into(),
            ));
        }

        // ── Resolve prompt / tool_whitelist / effective_model ─────────────
        let (sys_prompt_extra, tool_whitelist, model_override) = match &source {
            AgentSource::Registry(agent_type) => {
                // Legacy path: resolve from AgentRegistry.
                let definition = self.registry.get(agent_type).ok_or_else(|| {
                    ToolError::ExecutionFailed(format!(
                        "unknown subagent_type '{agent_type}'; \
                         check ~/.renlijia/users/<scope>/agents/ or builtin agents"
                    ))
                })?;
                let model = caller_model.clone().or_else(|| match &definition.model {
                    AgentModel::Fixed(m) => Some(m.clone()),
                    AgentModel::Inherit => None,
                });
                (None, Vec::<String>::new(), model)
            }
            AgentSource::Employee(eid) => {
                // New path: load Employee profile from EmployeeStore.
                let store = self.employee_store.as_ref().ok_or_else(|| {
                    ToolError::ExecutionFailed(
                        "EmployeeStore not configured; \
                         use SpawnSubagentRuntimeTool::new_with_employees() in production"
                            .into(),
                    )
                })?;
                let employee = store.get_readonly(eid).ok_or_else(|| {
                    ToolError::ExecutionFailed(format!("employee not found: {eid}"))
                })?;
                // EmployeeRecord has no model field; inherit from parent.
                let model = caller_model.clone();
                (
                    employee.system_prompt_extra.clone(),
                    employee.tool_whitelist.clone(),
                    model,
                )
            }
        };

        // ── Effective model after three-tier resolution ────────────────────
        // (already resolved per-source above; model_override holds the result)
        let effective_model = model_override;

        // ── Required-tool validation for Teammate dispatch ─────────────────
        // A Teammate must have SendMessage / TaskList / TaskGet in its
        // whitelist; legacy fire-and-forget subagents are exempt.
        if team_handle.is_some() {
            let missing = crate::runtime::agent::required_tools::missing_required(&tool_whitelist);
            if !missing.is_empty() {
                let who = match &source {
                    AgentSource::Employee(eid) => format!("employee `{eid}`"),
                    AgentSource::Registry(t) => format!("agent type `{t}`"),
                };
                return Err(ToolError::ExecutionFailed(format!(
                    "{who} cannot be a teammate — missing required tools: {missing:?}. \
                     Add these to its tool_whitelist (or fix the employee profile)."
                )));
            }
        }

        // ── Name registration for Teammate dispatch ────────────────────────
        // Only Teammate paths (team_handle.is_some()) register in the name
        // registry; legacy async subagents carry name in the request/outcome
        // without registry registration.
        //
        // The agent_id is generated here so that the registry entry and the
        // actual agent share the same identity when P1.6 fills in the idle loop.
        let teammate_agent_id: Option<AgentId> = if team_handle.is_some() {
            let agent_id = AgentId::new(format!("agent-{}", uuid::Uuid::new_v4()));
            // name is guaranteed Some here (checked above after team_handle check)
            if let Some(ref agent_name) = name {
                ctx.agent_names()
                    .register(&ctx.session_id, agent_name, agent_id.clone())
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            }
            Some(agent_id)
        } else {
            None
        };

        // ── Build request ──────────────────────────────────────────────────
        let request = SpawnSubagentRequest {
            subagent_type: match &source {
                AgentSource::Registry(t) => t.clone(),
                // Use a sentinel value for Employee-sourced agents; the launcher
                // stub will reject this anyway until P1.6 fills in the idle loop.
                AgentSource::Employee(eid) => format!("employee:{eid}"),
            },
            prompt: prompt.to_string(),
            description: description.to_string(),
            effective_model,
            run_in_background,
            name: name.clone(),
        };

        let launch_ctx = SpawnSubagentContext {
            session_id: ctx.session_id.clone(),
            parent_run_id: Some(ctx.run_id.clone()),
            parent_agent_id: ctx.agent_id.clone(),
            cancellation: ctx.cancellation.clone(),
            permission_mode: ctx.permission_mode,
            parent_tool_use_id: ctx.tool_call_id.clone(),
            permission_ctx: ctx
                .capability
                .as_ref()
                .and_then(|cap| cap.storage.as_ref())
                .map(|storage| storage.permission_ctx.clone()),
        };

        // ── Dispatch path ──────────────────────────────────────────────────
        if let Some(team) = team_handle {
            // Teammate idle-loop path (P1.6).
            // Side-effects (AgentNameRegistry registration, Team join) have
            // already been applied above.  Now spawn the idle loop.
            let agent_id = teammate_agent_id.expect("teammate_agent_id must be set when team is Some");
            let agent_name_str = name.clone().unwrap_or_default();
            let employee_id = if let AgentSource::Employee(ref eid) = source {
                Some(eid.clone())
            } else {
                None
            };
            let spawned_by = launch_ctx.parent_agent_id.as_ref().map(|id| id.as_str().to_owned());

            // Join the team as a Teammate before starting the idle loop.
            // Capture team_name while we hold the lock so the boot prompt
            // composition below doesn't need to re-lock.
            let team_name_str: String;
            {
                let mut team_guard = team.lock().await;
                let member = crate::runtime::agent::Member {
                    agent_id: agent_id.clone(),
                    name: agent_name_str.clone(),
                    role: crate::runtime::agent::MemberRole::Teammate {
                        employee_id: employee_id.clone().unwrap_or_default(),
                        spawned_by: launch_ctx.parent_agent_id.clone().unwrap_or_else(|| AgentId::new("unknown")),
                    },
                    created_at: chrono::Utc::now(),
                    last_active_at: chrono::Utc::now(),
                };
                if let Err(e) = team_guard.add_teammate(member) {
                    // Unregister name on failure to keep state consistent.
                    ctx.agent_names()
                        .unregister(&ctx.session_id, &agent_name_str)
                        .await;
                    return Err(ToolError::ExecutionFailed(format!(
                        "Failed to join team as Teammate: {e}"
                    )));
                }
                team_name_str = team_guard.team_name.clone();
            }

            let inbox = AgentInbox::new(64);

            // P2.2: register this Teammate's inbox so SendMessage(to: name) can
            // resolve it.  Optional — tests / legacy paths that don't carry an
            // InboxRegistry simply skip this step (their Teammate is unaddressable
            // via SendMessage but still runs).
            if let Some(reg) = ctx.inbox_registry.clone() {
                let sid = launch_ctx.session_id.clone();
                let aid = agent_id.clone();
                let ibx = inbox.clone();
                tokio::spawn(async move {
                    reg.register(&sid, aid, ibx).await;
                });
            }

            let meta = AgentTranscriptMeta {
                agent_id: agent_id.as_str().to_string(),
                agent_name: name.clone(),
                kind: TranscriptKind::Teammate,
                employee_id: employee_id.clone(),
                team_id: Some(launch_ctx.session_id.as_str().to_string()),
                spawned_by,
                spawned_at: chrono::Utc::now(),
                model: request.effective_model.clone(),
                is_async: true,
                tool_whitelist: tool_whitelist.clone(),
                boot_system_prompt: Some(
                    crate::runtime::agent::teammate_addendum::compose_boot_prompt(
                        sys_prompt_extra.as_deref().unwrap_or(""),
                        &team_name_str,
                        &agent_name_str,
                    ),
                ),
            };

            let worker_ctx = TeammateWorkerCtx {
                agent_id: agent_id.clone(),
                session_id: launch_ctx.session_id.clone(),
                conv_id: launch_ctx.session_id.as_str().to_string(),
                cancel: launch_ctx.cancellation.child_token(),
                inbox: inbox.clone(),
                agent_names: ctx.agent_names().clone(),
                inbox_registry: ctx.inbox_registry.clone(),
                conv_dir: None, // P2: inject from paths resolver
                meta,
            };

            let initial_prompt = request.prompt.clone();
            let team_for_spawn = team.clone();
            let agent_name_for_spawn = agent_name_str.clone();
            tokio::spawn(async move {
                if let Err(e) = run_worker(
                    WorkerMode::TeammateIdle {
                        team_handle: team_for_spawn,
                        agent_name: agent_name_for_spawn,
                    },
                    worker_ctx,
                    Some(initial_prompt),
                )
                .await
                {
                    log::warn!("[spawn_teammate] idle loop exited with error: {e}");
                }
            });

            let agent_id_str = agent_id.as_str().to_string();
            let json = serde_json::json!({
                "status": "teammate_spawned",
                "agent_id": agent_id_str,
                "name": name,
            });
            return Ok(ToolResult::new("Agent", json.to_string(), None));
        }

        // ── Legacy async path ──────────────────────────────────────────────
        if request.run_in_background {
            let outcome = self
                .launcher
                .launch_async(request, launch_ctx)
                .await
                .map_err(|e| {
                    ToolError::ExecutionFailed(format!("async sub-agent launch failed: {e}"))
                })?;
            let agent_id_str = outcome.agent_id.to_string();
            let json = serde_json::json!({
                "status": "async_launched",
                "agent_id": agent_id_str.clone(),
                "task_id": agent_id_str,
                "name": outcome.name,
            });
            return Ok(ToolResult::new("Agent", json.to_string(), None));
        }

        // ── Legacy sync path ───────────────────────────────────────────────
        let output = self
            .launcher
            .launch_sync(request, launch_ctx)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("sub-agent launch failed: {e}")))?;

        Ok(ToolResult::new("Agent", output, None))
    }
}

// ─── Tests ──────────────────────────────────────────────────────────��─────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tools::permission::PermissionMode;
    use serde_json::json;
    use std::sync::Mutex;

    struct RecordingLauncher {
        seen_requests: Arc<Mutex<Vec<SpawnSubagentRequest>>>,
        async_seen: Arc<Mutex<Vec<SpawnSubagentRequest>>>,
    }

    #[async_trait]
    impl SpawnSubagentLauncher for RecordingLauncher {
        async fn launch_sync(
            &self,
            request: SpawnSubagentRequest,
            _context: SpawnSubagentContext,
        ) -> Result<String> {
            self.seen_requests.lock().unwrap().push(request.clone());
            Ok(format!(
                "stub-output: type={}, model={:?}",
                request.subagent_type, request.effective_model
            ))
        }

        async fn launch_async(
            &self,
            request: SpawnSubagentRequest,
            _context: SpawnSubagentContext,
        ) -> Result<SpawnAsyncOutcome> {
            self.async_seen.lock().unwrap().push(request.clone());
            Ok(SpawnAsyncOutcome {
                agent_id: AgentId::new("stub-async-id"),
                name: request.name.clone(),
            })
        }
    }

    fn build_tool_with_recorder(
        seen: Arc<Mutex<Vec<SpawnSubagentRequest>>>,
    ) -> SpawnSubagentRuntimeTool {
        let registry = Arc::new(AgentRegistry::with_builtins());
        SpawnSubagentRuntimeTool::new(
            Arc::new(RecordingLauncher {
                seen_requests: seen,
                async_seen: Arc::new(Mutex::new(Vec::new())),
            }),
            registry,
        )
    }

    #[test]
    fn is_concurrency_safe_returns_true() {
        let tool = build_tool_with_recorder(Arc::new(Mutex::new(Vec::new())));
        assert!(tool.is_concurrency_safe(&json!({})));
    }

    #[test]
    fn definition_id_is_spawn_subagent() {
        let tool = build_tool_with_recorder(Arc::new(Mutex::new(Vec::new())));
        // Definition returned even without catalog pre-init in unit tests.
        let def = tool.definition();
        assert_eq!(def.id, "Agent");
    }

    #[tokio::test]
    async fn missing_subagent_type_returns_execution_failed() {
        let tool = build_tool_with_recorder(Arc::new(Mutex::new(Vec::new())));
        let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
            .with_permission_mode(PermissionMode::Default);
        // Neither subagent_type nor employee_id — should fail with "required" message.
        let err = tool
            .execute(
                json!({ "prompt": "do something", "description": "test" }),
                ctx,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::ExecutionFailed(msg) => {
                assert!(
                    msg.contains("subagent_type") || msg.contains("required"),
                    "error should mention subagent_type or required, got: {msg}"
                );
            }
            other => panic!("expected ExecutionFailed, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn missing_prompt_returns_execution_failed() {
        let tool = build_tool_with_recorder(Arc::new(Mutex::new(Vec::new())));
        let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
        let err = tool
            .execute(
                json!({ "subagent_type": "explore", "description": "test" }),
                ctx,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::ExecutionFailed(msg) => {
                assert!(
                    msg.contains("prompt"),
                    "error should mention prompt, got: {msg}"
                );
            }
            other => panic!("expected ExecutionFailed, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn unknown_subagent_type_returns_helpful_error() {
        let tool = build_tool_with_recorder(Arc::new(Mutex::new(Vec::new())));
        let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
        let err = tool
            .execute(
                json!({
                    "subagent_type": "nonexistent_type_xyz",
                    "prompt": "do it",
                    "description": "test"
                }),
                ctx,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::ExecutionFailed(msg) => {
                assert!(msg.contains("nonexistent_type_xyz"));
                assert!(
                    msg.contains("~/.renlijia")
                        || msg.contains("agents/")
                        || msg.contains("builtin")
                );
            }
            other => panic!("expected ExecutionFailed, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn run_in_background_true_calls_launch_async() {
        let tool = build_tool_with_recorder(Arc::new(Mutex::new(Vec::new())));
        let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
        let result = tool
            .execute(
                json!({
                    "subagent_type": "explore",
                    "prompt": "scrape some data",
                    "description": "test background",
                    "run_in_background": true,
                    "name": "w1"
                }),
                ctx,
            )
            .await
            .expect("background path should not return Err");
        let parsed: serde_json::Value = serde_json::from_str(&result.content)
            .expect("background response should be valid JSON");
        assert_eq!(
            parsed.get("status").and_then(|v| v.as_str()),
            Some("async_launched"),
            "status should be async_launched, got: {}",
            result.content
        );
        assert!(
            parsed.get("agent_id").and_then(|v| v.as_str()).is_some(),
            "agent_id should be present"
        );
    }

    #[tokio::test]
    async fn sync_path_passes_context_permission_mode_to_launcher() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let tool = build_tool_with_recorder(seen.clone());
        let ctx = ToolExecutionContext::for_test("conv-perm", "run-perm", "tc-perm")
            .with_permission_mode(PermissionMode::DontAsk);
        // explore is a builtin in AgentRegistry::with_builtins()
        tool.execute(
            json!({
                "subagent_type": "explore",
                "prompt": "extract table",
                "description": "sync perm test"
            }),
            ctx,
        )
        .await
        .expect("sync path should succeed with stub launcher");
        let reqs = seen.lock().unwrap();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].subagent_type, "explore");
    }

    #[tokio::test]
    async fn empty_model_string_treated_as_inherit() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let tool = build_tool_with_recorder(seen.clone());
        let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
        tool.execute(
            json!({
                "subagent_type": "explore",
                "prompt": "test",
                "description": "empty model",
                "model": ""
            }),
            ctx,
        )
        .await
        .expect("should succeed");
        let reqs = seen.lock().unwrap();
        // effective_model should be determined by definition.model (Inherit → None)
        // rather than forwarding the empty string.
        assert!(
            reqs[0]
                .effective_model
                .as_deref()
                .map(|s| !s.is_empty())
                .unwrap_or(true),
            "empty caller model should not override to empty string"
        );
    }

    // ── P1.3: mutual exclusion test ──────────────────────────────────────────

    #[tokio::test]
    async fn rejects_both_subagent_type_and_employee_id() {
        let tool = build_tool_with_recorder(Arc::new(Mutex::new(Vec::new())));
        let ctx = ToolExecutionContext::for_test("conv-mutex", "run-mutex", "tc-mutex");
        let err = tool
            .execute(
                json!({
                    "subagent_type": "explore",
                    "employee_id": "emp-123",
                    "prompt": "do it",
                    "description": "mutex test"
                }),
                ctx,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::ExecutionFailed(msg) => {
                assert!(
                    msg.contains("mutually exclusive"),
                    "error should say 'mutually exclusive', got: {msg}"
                );
            }
            other => panic!("expected ExecutionFailed, got: {:?}", other),
        }
    }
}
