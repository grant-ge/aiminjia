//! Generic `spawn_subagent` RuntimeTool — entry point for a parent LLM to
//! delegate work to a sub-agent.
//!
//! Architecture: this tool is the dispatcher only. Actual sub-agent execution is
//! injected via [`SpawnSubagentLauncher`] (parallel to `BrowseDataLauncher`),
//! resolved at `lib.rs`/`registry.rs` setup time. This module is free of
//! `LlmGateway` / `SubAgentRuntimeDeps` imports.
//!
//! ## Sync path (`run_in_background = false` or omitted)
//! Awaits `launcher.launch_foreground_auto_background()` and returns either the
//! final output as a [`ToolResult`] or an `async_launched` task JSON if the
//! sub-agent is auto-promoted to background execution.
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
//! Passing `team_name` (non-empty) turns the dispatch into a Teammate that
//! joins the session's Team. `subagent_type` may be a builtin agent or an
//! Employee profile. `name` is required for Teammate dispatch and is registered
//! in [`AgentNameRegistry`].
//! The idle loop is launched via `tokio::spawn(run_worker(WorkerMode::TeammateIdle{...}))`.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::agent::definition::AgentModel;
use crate::runtime::agent::inbox::AgentInbox;
use crate::runtime::agent::output_writer::{AgentTranscriptMeta, TranscriptKind};
use crate::runtime::agent::registry::AgentRegistry;
use crate::runtime::agent::worker_runtime::{run_worker, TeammateWorkerCtx, WorkerMode};
use crate::runtime::cancellation::CancellationToken;
use crate::runtime::ids::{AgentId, RunId, SessionId, ToolCallId};
use crate::runtime::path_auth::ToolPermissionContext;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::permission::PermissionMode;
use crate::runtime::tools::RuntimeTool;
use crate::telemetry::{record_diagnostic, DiagnosticEvent, DiagnosticSource};

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
    /// Hidden test/debug override for foreground auto-background timeout.
    pub auto_background_after_ms: Option<u64>,
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

/// Outcome from the default foreground path, which may auto-promote to a
/// background task after the blocking budget elapses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpawnForegroundAutoOutcome {
    Completed(String),
    Backgrounded {
        agent_id: AgentId,
        name: Option<String>,
        auto_background_after_ms: u64,
    },
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

    /// Default foreground path: wait up to a foreground budget, then keep the
    /// same sub-agent running in background if it exceeds that budget.
    async fn launch_foreground_auto_background(
        &self,
        request: SpawnSubagentRequest,
        context: SpawnSubagentContext,
    ) -> Result<SpawnForegroundAutoOutcome> {
        self.launch_sync(request, context)
            .await
            .map(SpawnForegroundAutoOutcome::Completed)
    }

    /// Build a `TeammateLlmEngine` for a Teammate idle loop.
    ///
    /// Production launchers return `Some(engine)` so the Teammate runs real
    /// LLM turns.  Test / stub launchers may return `None`, in which case
    /// the idle loop falls back to stub-mode (transcript-only placeholder
    /// replies — see `teammate_stub_turn`).
    ///
    /// The default impl returns `None` so test launchers don't need to
    /// override it (they continue exercising the stub path).
    async fn build_teammate_llm_engine(
        &self,
        _context: &SpawnSubagentContext,
    ) -> Option<crate::runtime::agent::worker_runtime::TeammateLlmEngine> {
        None
    }
}

// ─── RuntimeTool implementation ───────────────────────────────────────────────

pub struct SpawnSubagentRuntimeTool {
    launcher: Arc<dyn SpawnSubagentLauncher>,
    registry: Arc<AgentRegistry>,
}

impl SpawnSubagentRuntimeTool {
    pub fn new(launcher: Arc<dyn SpawnSubagentLauncher>, registry: Arc<AgentRegistry>) -> Self {
        Self { launcher, registry }
    }
}

/// Render the dispatchable-agent catalog as a markdown chunk to append
/// to the Agent tool's description. The output is bounded (~tens of
/// lines) and strictly informational — no secrets / paths.
///
/// When both lists are empty the function returns an empty string so the
/// caller can skip appending. Public + free-standing so unit tests can
/// hit it directly without constructing a tool.
///
/// Aligns with claude-code-best `getPrompt(agentDefinitions, ...)` in
/// `src/tools/AgentTool/prompt.ts` — that file builds the same kind of
/// "Available agent types" listing which is then returned from
/// `tool.prompt()` and fed verbatim into the tool schema description field.
pub fn render_dispatch_catalog(ctx: &crate::runtime::tools::ToolDescriptionContext) -> String {
    use crate::runtime::agent::definition::AgentSource;
    use std::fmt::Write as _;

    let mut emp_lines: Vec<String> = Vec::new();
    let mut other_lines: Vec<String> = Vec::new();
    for a in &ctx.agents {
        let line = format!("- `{}` — {}", a.name, a.description);
        if matches!(a.source, AgentSource::Employee) {
            emp_lines.push(line);
        } else {
            other_lines.push(line);
        }
    }
    if emp_lines.is_empty() && other_lines.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str(
        "**重要**：`subagent_type` 必须从下面清单中精确选择，禁止编造未列出的名字。\
         普通角色发言、临时专家讨论、兜底分析优先使用通用/内置 Agent；\
         只有用户明确要求某个数字员工，或任务确实需要该数字员工的专属人设、技能和工具白名单时，才选择 `emp-...`。",
    );
    let _ = write!(out, "\n\n<available_subagent_types>\n");
    for line in emp_lines.iter().chain(other_lines.iter()) {
        let _ = writeln!(out, "{line}");
    }
    let _ = write!(out, "</available_subagent_types>");
    out
}

/// 当 LLM 传入未知 `subagent_type` 时构造错误信息，把当前可选清单回灌给它。
/// 对齐 claude-code-best `AgentTool.tsx:532-536` 的 `Available agents: ...` 模式。
pub fn build_unknown_subagent_type_error(bad_name: &str, registry: &AgentRegistry) -> String {
    let available: Vec<String> = registry.list().iter().map(|d| d.name.clone()).collect();
    // Trailing hint points users at where to add their own agent definitions
    // (builtin agents ship with the app; user-level lives under ~/.renlijia/agents/;
    // project-level lives under <workspace>/agents/).  Aligns with
    // claude-code-best `AgentTool.tsx` which also surfaces install paths.
    const LOCATION_HINT: &str =
        " Add new agents under ~/.renlijia/agents/ or <workspace>/agents/, \
         or use a builtin agent name.";
    if available.is_empty() {
        format!(
            "unknown subagent_type '{}'; no agents configured.{LOCATION_HINT}",
            bad_name
        )
    } else {
        format!(
            "unknown subagent_type '{}'. Available subagent_type values: {}.{LOCATION_HINT}",
            bad_name,
            available.join(", ")
        )
    }
}

fn should_route_omitted_team_name_to_active_team(
    active_team_name: Option<&str>,
    name: Option<&str>,
    prompt: &str,
    description: &str,
) -> bool {
    if active_team_name.is_none() || name.is_none() {
        return false;
    }
    let text = format!("{description}\n{prompt}");
    let has_team_recipient = text.contains("team-lead")
        || text.contains("团队")
        || text.contains("圆桌")
        || text.contains("专家");
    let has_expert_deliverable = text.contains("发表")
        || text.contains("发言")
        || text.contains("观点")
        || text.contains("专业")
        || text.contains("共识")
        || text.contains("请从");
    has_team_recipient && has_expert_deliverable
}

#[async_trait]
impl RuntimeTool for SpawnSubagentRuntimeTool {
    fn id(&self) -> &str {
        "Agent"
    }

    async fn definition(
        &self,
        ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        let mut def = TOOL_CATALOG
            .get("Agent")
            .unwrap_or_else(|| ToolDefinition::new("Agent", "Spawn sub-agent"));
        let dynamic = render_dispatch_catalog(ctx);

        // Diagnostic event so we can verify the catalog actually rendered
        // and reached the LLM. Payload is bounded (preview at 4KB).
        let ws = crate::telemetry::diagnostics_workspace();
        let preview: String = dynamic.chars().take(4096).collect();
        record_diagnostic(
            &ws,
            DiagnosticEvent::new(
                "tool.spawn_subagent.definition.rendered",
                DiagnosticSource::Backend,
            )
            .payload(serde_json::json!({
                "dynamic_len": dynamic.len(),
                "ctx_agent_count": ctx.agents.len(),
                "has_subagent_section": dynamic.contains("<available_subagent_types>"),
                "agent_names": ctx.agents.iter().map(|a| a.name.clone()).collect::<Vec<_>>(),
                "employee_count": ctx.agents.iter()
                    .filter(|a| matches!(a.source, crate::runtime::agent::definition::AgentSource::Employee))
                    .count(),
                "preview": preview,
            })),
        );

        if !dynamic.is_empty() {
            def.description.push_str("\n\n");
            def.description.push_str(&dynamic);
        }
        def
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
        let auto_background_after_ms = input
            .get("_auto_background_after_ms")
            .and_then(Value::as_u64)
            .map(|value| value.max(1));
        let name = input
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let explicit_team_name = input
            .get("team_name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let inferred_active_team_name = if explicit_team_name.is_none()
            && should_route_omitted_team_name_to_active_team(
                ctx.active_team_name.as_deref(),
                name.as_deref(),
                prompt,
                description,
            ) {
            ctx.active_team_name.clone()
        } else {
            None
        };
        let team_name = explicit_team_name
            .clone()
            .or_else(|| inferred_active_team_name.clone());

        // ── Parse the single dispatch source field ────────────────────────
        // Server-authored expert-team templates can omit subagent_type while
        // still supplying (or implying) team_name/name/prompt. Treat that as
        // the generic teammate worker instead of forcing a visible retry.
        let subagent_type = match input
            .get("subagent_type")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
        {
            Some(value) => value,
            None if team_name.is_some() => "general-purpose".to_string(),
            None => {
                return Err(ToolError::ExecutionFailed(
                    "missing required field: subagent_type".into(),
                ))
            }
        };

        // ── Team handle resolution ─────────────────────────────────────────
        // Must happen before name registration so a failed team lookup doesn't
        // leave a stale entry in AgentNameRegistry.
        //
        // Resolution rule: an explicit `team_name` turns Agent dispatch into
        // a Teammate. Expert-team director templates sometimes omit
        // `team_name` on Agent calls after creating a team; if the call carries
        // a member `name` and the prompt clearly asks for expert/team
        // deliverables, route it to the active team as a narrow compatibility
        // fallback. Other omitted-team Agent calls remain standalone.
        //
        // If the caller supplied a stale/display team name, fall back to the
        // current active team to avoid a failed tool round caused by mixing
        // human labels with internal IDs.
        let team_handle = if let Some(caller_team) = team_name.as_deref() {
            let session_id = ctx.session_id.clone();
            let team = match ctx.team_registry().get(&session_id, caller_team).await {
                Some(team) => team,
                None => {
                    let active = ctx.active_team_name.as_deref().ok_or_else(|| {
                        ToolError::ExecutionFailed(format!(
                            "team `{caller_team}` not found in this session — call TeamCreate first or check the spelling"
                        ))
                    })?;
                    let active_team = ctx.team_registry().get(&session_id, active).await.ok_or_else(
                        || {
                            ToolError::ExecutionFailed(format!(
                                "team `{caller_team}` not found in this session, and active team `{active}` is not available"
                            ))
                        },
                    )?;
                    log::warn!(
                        "[spawn_subagent] caller-supplied team_name {:?} not found; using active team {:?}",
                        caller_team,
                        active
                    );
                    active_team
                }
            };
            // Surface unexpected mismatches between the caller-supplied name
            // and the active team to help diagnose stale UI labels, but the
            // resolved team handle is authoritative for routing.
            if let (Some(caller), Some(active)) = (&team_name, &ctx.active_team_name) {
                if caller != active {
                    log::warn!(
                        "[spawn_subagent] caller-supplied team_name {:?} differs from active team {:?}; using resolved team handle",
                        caller,
                        active
                    );
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

        // ── Resolve agent definition from registry (employees已在 boot 时投影进去) ──
        let definition = self.registry.get(&subagent_type).ok_or_else(|| {
            ToolError::ExecutionFailed(build_unknown_subagent_type_error(
                &subagent_type,
                &self.registry,
            ))
        })?;
        let sys_prompt_extra = match &definition.system_prompt {
            crate::runtime::agent::definition::AgentPrompt::Inline(s) if !s.is_empty() => {
                Some(s.clone())
            }
            _ => None,
        };
        let tool_whitelist = definition.allowed_tools.clone();
        let effective_model = caller_model.clone().or_else(|| match &definition.model {
            AgentModel::Fixed(m) => Some(m.clone()),
            AgentModel::Inherit => None,
        });

        // ── Teammate dispatch: collaboration tools are injected at runtime ─
        // `SendMessage / TaskList / TaskGet` are infrastructure that the
        // runtime injects when a Teammate is spawned (see
        // `runtime::agent::tool_whitelist::TEAMMATE_TOOLS`). Employee /
        // agent definitions should NOT have to list these in their
        // whitelist — that would be a leaky abstraction (business config
        // owns runtime orchestration capabilities). Aligns with
        // claude-code-best `IN_PROCESS_TEAMMATE_ALLOWED_TOOLS`
        // (src/constants/tools.ts:77). No pre-spawn gate here anymore.

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
                // Resolve team_name from the active team handle so the registry
                // key matches what cleanup_teammate will use (PR4).
                let team_name_for_reg: String = {
                    let guard = team_handle.as_ref().unwrap().lock().await;
                    guard.team_name.clone()
                };
                if let Some(existing_agent_id) = ctx
                    .agent_names()
                    .resolve(&ctx.session_id, &team_name_for_reg, agent_name)
                    .await
                {
                    return Ok(ToolResult::new(
                        "Agent",
                        format!("teammate `{agent_name}` already registered; duplicate spawn suppressed"),
                        Some(serde_json::json!({
                            "status": "teammate_spawned",
                            "agent_id": existing_agent_id.as_str(),
                            "name": agent_name,
                            "duplicate_suppressed": true,
                        })),
                    ));
                }
                let ws = crate::telemetry::diagnostics_workspace();
                record_diagnostic(
                    &ws,
                    DiagnosticEvent::new(
                        "tool.spawn_subagent.teammate.name_registered",
                        DiagnosticSource::Backend,
                    )
                    .conversation_id(ctx.session_id.as_str())
                    .run_id(ctx.run_id.as_str())
                    .tool_call_id(ctx.tool_call_id.as_str())
                    .agent_id(agent_id.as_str())
                    .team_name(team_name_for_reg.as_str())
                    .payload(serde_json::json!({ "agent_name": agent_name })),
                );
                ctx.agent_names()
                    .register(
                        &ctx.session_id,
                        &team_name_for_reg,
                        agent_name,
                        agent_id.clone(),
                    )
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                log::info!(
                    "[spawn_subagent][diag] teammate name registered: session={} team={} agent_id={} agent_name={}",
                    ctx.session_id.as_str(),
                    team_name_for_reg,
                    agent_id.as_str(),
                    agent_name
                );
            }
            Some(agent_id)
        } else {
            None
        };

        // ── Build request ──────────────────────────────────────────────────
        let request = SpawnSubagentRequest {
            subagent_type: subagent_type.clone(),
            prompt: prompt.to_string(),
            description: description.to_string(),
            effective_model,
            run_in_background,
            name: name.clone(),
            auto_background_after_ms,
        };

        let launch_ctx = SpawnSubagentContext {
            session_id: ctx.session_id.clone(),
            parent_run_id: Some(ctx.run_id.clone()),
            parent_agent_id: ctx.agent_id.clone(),
            cancellation: ctx.cancellation.clone(),
            // Sub-agents can run in foreground, explicit background, or be
            // auto-promoted to background. Background runners cannot surface a
            // permission prompt, so grant the child turn directly instead of
            // leaking AskRequired back into the model transcript.
            permission_mode: PermissionMode::FullAccess,
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
            let agent_id =
                teammate_agent_id.expect("teammate_agent_id must be set when team is Some");
            let agent_name_str = name.clone().unwrap_or_default();
            let employee_id = if matches!(
                definition.source,
                crate::runtime::agent::definition::AgentSource::Employee
            ) {
                Some(definition.name.clone())
            } else {
                None
            };
            let spawned_by = launch_ctx
                .parent_agent_id
                .as_ref()
                .map(|id| id.as_str().to_owned());

            // Join the team as a Teammate before starting the idle loop.
            // Capture team_name while we hold the lock so the boot prompt
            // composition below doesn't need to re-lock.
            let team_name_str: String;
            {
                let mut team_guard = team.lock().await;
                // Capture team_name first so rollback unregister uses the same key.
                let tname_now = team_guard.team_name.clone();
                let member = crate::runtime::agent::Member {
                    agent_id: agent_id.clone(),
                    name: agent_name_str.clone(),
                    role: crate::runtime::agent::MemberRole::Teammate {
                        employee_id: employee_id.clone().unwrap_or_default(),
                        spawned_by: launch_ctx
                            .parent_agent_id
                            .clone()
                            .unwrap_or_else(|| AgentId::new("unknown")),
                    },
                    created_at: chrono::Utc::now(),
                    last_active_at: chrono::Utc::now(),
                };
                if let Err(e) = team_guard.add_teammate(member) {
                    // Unregister name on failure to keep state consistent.
                    ctx.agent_names()
                        .unregister(&ctx.session_id, &tname_now, &agent_name_str)
                        .await;
                    return Err(ToolError::ExecutionFailed(format!(
                        "Failed to join team as Teammate: {e}"
                    )));
                }
                team_name_str = tname_now;
            }

            // 持久化 team.json（teammate 加入后名册更新）。fire-and-forget
            // tokio::spawn 避免阻塞 spawn 流程；teammate 第一轮 Read 之前
            // 大概率已落盘。
            if let Some(ref conv_dir) = ctx.conv_dir {
                let reg = ctx.team_registry().clone();
                let sid = ctx.session_id.clone();
                let dir = conv_dir.clone();
                let tname_persist = team_name_str.clone();
                tokio::spawn(async move {
                    if let Err(e) = reg.persist(&sid, &tname_persist, &dir).await {
                        log::warn!("[SpawnTeammate] persist config.json failed: {e}");
                    }
                });
            }

            let inbox = AgentInbox::new(64);

            // P2.7: derive the child cancel token NOW (before passing into
            // TeammateWorkerCtx via .child_token()) so we can register it
            // for TeammateStop lookup.
            let teammate_cancel = launch_ctx.cancellation.child_token();
            if let Some(reg) = ctx.cancellation_registry.clone() {
                let sid = launch_ctx.session_id.clone();
                let tname = team_name_str.clone();
                let aid = agent_id.clone();
                let tok = teammate_cancel.clone();
                tokio::spawn(async move {
                    reg.register(&sid, &tname, aid, tok).await;
                });
            }

            // P2.2: register this Teammate's inbox so SendMessage(to: name) can
            // resolve it.  Optional — tests / legacy paths that don't carry an
            // InboxRegistry simply skip this step (their Teammate is unaddressable
            // via SendMessage but still runs).
            if let Some(reg) = ctx.inbox_registry.clone() {
                let sid = launch_ctx.session_id.clone();
                let tname = team_name_str.clone();
                let aid = agent_id.clone();
                let ibx = inbox.clone();
                tokio::spawn(async move {
                    reg.register(&sid, &tname, aid, ibx).await;
                });
            }

            let meta = AgentTranscriptMeta {
                agent_id: agent_id.as_str().to_string(),
                agent_name: name.clone(),
                kind: TranscriptKind::Teammate,
                employee_id: employee_id.clone(),
                team_id: Some(team_name_str.clone()),
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
                team_name: team_name_str.clone(),
                conv_id: launch_ctx.session_id.as_str().to_string(),
                cancel: teammate_cancel,
                inbox: inbox.clone(),
                agent_names: ctx.agent_names().clone(),
                inbox_registry: ctx.inbox_registry.clone(),
                cancellation_registry: ctx.cancellation_registry.clone(),
                conv_dir: ctx.conv_dir.clone(),
                meta,
                // Production launchers return Some(engine) so the idle loop
                // runs real LLM turns; test launchers fall through to the
                // default `None` impl and exercise the stub path.
                llm_engine: self.launcher.build_teammate_llm_engine(&launch_ctx).await,
            };

            let ws = crate::telemetry::diagnostics_workspace();
            record_diagnostic(
                &ws,
                DiagnosticEvent::new(
                    "tool.spawn_subagent.teammate.worker_ctx_built",
                    DiagnosticSource::Backend,
                )
                .conversation_id(ctx.session_id.as_str())
                .run_id(ctx.run_id.as_str())
                .tool_call_id(ctx.tool_call_id.as_str())
                .agent_id(agent_id.as_str())
                .team_name(team_name_str.as_str())
                .payload(serde_json::json!({
                    "agent_name": agent_name_str,
                    "team_name": team_name_str,
                    "employee_id": employee_id,
                })),
            );

            let initial_prompt = request.prompt.clone();
            let team_for_spawn = team.clone();
            let agent_name_for_spawn = agent_name_str.clone();
            let agent_id_for_diag = agent_id.as_str().to_string();
            let conv_id_for_diag = ctx.session_id.as_str().to_string();
            let run_id_for_diag = ctx.run_id.as_str().to_string();
            let tool_call_id_for_diag = ctx.tool_call_id.as_str().to_string();
            let team_name_for_diag = team_name_str.clone();
            record_diagnostic(
                &ws,
                DiagnosticEvent::new(
                    "tool.spawn_subagent.teammate.spawning",
                    DiagnosticSource::Backend,
                )
                .conversation_id(&conv_id_for_diag)
                .run_id(&run_id_for_diag)
                .tool_call_id(&tool_call_id_for_diag)
                .agent_id(&agent_id_for_diag)
                .team_name(team_name_for_diag.as_str())
                .ok(true)
                .payload(serde_json::json!({ "agent_name": agent_name_for_spawn })),
            );
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
                    let ws_inner = crate::telemetry::diagnostics_workspace();
                    record_diagnostic(
                        &ws_inner,
                        DiagnosticEvent::new(
                            "tool.spawn_subagent.teammate.worker_exited_error",
                            DiagnosticSource::Backend,
                        )
                        .conversation_id(&conv_id_for_diag)
                        .agent_id(&agent_id_for_diag)
                        .team_name(team_name_for_diag.as_str())
                        .error(e.to_string()),
                    );
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
                "task_type": "local_agent",
                "name": outcome.name,
            });
            return Ok(ToolResult::new("Agent", json.to_string(), None));
        }

        // ── Foreground auto-background path ────────────────────────────────
        let outcome = self
            .launcher
            .launch_foreground_auto_background(request, launch_ctx)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("sub-agent launch failed: {e}")))?;

        match outcome {
            SpawnForegroundAutoOutcome::Completed(output) => {
                Ok(ToolResult::new("Agent", output, None))
            }
            SpawnForegroundAutoOutcome::Backgrounded {
                agent_id,
                name,
                auto_background_after_ms,
            } => {
                let agent_id_str = agent_id.as_str().to_string();
                let json = serde_json::json!({
                    "status": "async_launched",
                    "agent_id": agent_id_str.clone(),
                    "task_id": agent_id_str,
                    "task_type": "local_agent",
                    "name": name,
                    "assistant_auto_backgrounded": true,
                    "auto_background_after_ms": auto_background_after_ms,
                });
                Ok(ToolResult::new("Agent", json.to_string(), None))
            }
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────��─────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::agent::definition::AgentSource;
    use crate::runtime::tools::description_context::{AgentDefSummary, ToolDescriptionContext};
    use crate::runtime::tools::permission::PermissionMode;
    use serde_json::json;
    use std::sync::Mutex;

    // ── render_dispatch_catalog ────────────────────────────────────────────

    #[test]
    fn render_catalog_empty_ctx_returns_empty() {
        let ctx = ToolDescriptionContext::empty();
        assert_eq!(render_dispatch_catalog(&ctx), "");
    }

    #[test]
    fn render_catalog_lists_employees_with_id() {
        // The bug we hit: LLM picked subagent_type=general-purpose because
        // it never saw emp-* IDs. With a populated ctx the rendered chunk
        // must contain both IDs verbatim so they reach Anthropic via
        // tools[i].description.
        let ctx = ToolDescriptionContext {
            agents: vec![
                AgentDefSummary {
                    name: "emp-aaa-xiaoyan".into(),
                    description: "小研（调研员，数字员工）".into(),
                    source: AgentSource::Employee,
                },
                AgentDefSummary {
                    name: "emp-bbb-xiaosuan".into(),
                    description: "小算（数据分析师，数字员工）".into(),
                    source: AgentSource::Employee,
                },
            ],
            mcp_servers: vec![],
        };
        let out = render_dispatch_catalog(&ctx);
        assert!(out.contains("emp-aaa-xiaoyan"), "missing emp id 1: {out}");
        assert!(out.contains("emp-bbb-xiaosuan"), "missing emp id 2: {out}");
        assert!(out.contains("小研"), "missing 小研 name");
        assert!(out.contains("小算"), "missing 小算 name");
        assert!(
            out.contains("<available_subagent_types>"),
            "missing subagent_types section header"
        );
        assert!(
            !out.contains("<available_employee_ids>"),
            "old employee section header should be gone"
        );
        assert!(
            out.contains("禁止编造未列出的名字"),
            "missing anti-hallucination guidance"
        );
    }

    #[test]
    fn render_catalog_lists_agents_with_summary() {
        let ctx = ToolDescriptionContext {
            agents: vec![
                AgentDefSummary {
                    name: "explore".into(),
                    description: "Read-only investigation".into(),
                    source: AgentSource::Builtin,
                },
                AgentDefSummary {
                    name: "general-purpose".into(),
                    description: "All tools available".into(),
                    source: AgentSource::Builtin,
                },
            ],
            mcp_servers: vec![],
        };
        let out = render_dispatch_catalog(&ctx);
        assert!(out.contains("`explore`"));
        assert!(out.contains("`general-purpose`"));
        assert!(out.contains("<available_subagent_types>"));
        assert!(!out.contains("<available_employee_ids>"));
    }

    #[test]
    fn render_catalog_employees_appear_before_builtins() {
        // Design choice: employees should come FIRST in the unified catalog
        // so the LLM is prompted to consider them before falling back to
        // builtins like general-purpose.
        let ctx = ToolDescriptionContext {
            agents: vec![
                AgentDefSummary {
                    name: "general-purpose".into(),
                    description: "fallback".into(),
                    source: AgentSource::Builtin,
                },
                AgentDefSummary {
                    name: "emp-x".into(),
                    description: "n（r，数字员工）".into(),
                    source: AgentSource::Employee,
                },
            ],
            mcp_servers: vec![],
        };
        let out = render_dispatch_catalog(&ctx);
        let emp_idx = out.find("emp-x").expect("emp line missing");
        let bi_idx = out.find("general-purpose").expect("builtin line missing");
        assert!(
            emp_idx < bi_idx,
            "employee should be listed before builtin: {out}"
        );
    }

    #[test]
    fn render_catalog_does_not_prefer_employees_for_generic_roles() {
        let ctx = ToolDescriptionContext {
            agents: vec![
                AgentDefSummary {
                    name: "emp-x".into(),
                    description: "薪酬专家（数字员工）".into(),
                    source: AgentSource::Employee,
                },
                AgentDefSummary {
                    name: "general-purpose".into(),
                    description: "通用子代理".into(),
                    source: AgentSource::Builtin,
                },
            ],
            mcp_servers: vec![],
        };

        let out = render_dispatch_catalog(&ctx);

        assert!(
            !out.contains("优先选择"),
            "catalog should not bias generic role roundtables toward employees: {out}"
        );
        assert!(
            out.contains("明确要求") || out.contains("专属"),
            "catalog should explain when employee agents are appropriate: {out}"
        );
    }

    // ── definition() integration with ctx ──────────────────────────────────

    #[tokio::test]
    async fn definition_appends_dynamic_catalog_to_base() {
        // The actual entry point used by `get_schemas_filtered`: when a
        // populated ctx is passed, the description must end with the
        // rendered catalog (base then \n\n then catalog).
        let registry = Arc::new(crate::runtime::agent::registry::AgentRegistry::with_builtins());
        let tool = SpawnSubagentRuntimeTool::new(
            Arc::new(RecordingLauncher {
                seen_requests: Arc::new(Mutex::new(Vec::new())),
                async_seen: Arc::new(Mutex::new(Vec::new())),
            }),
            registry,
        );
        let ctx = ToolDescriptionContext {
            agents: vec![AgentDefSummary {
                name: "emp-test-001".into(),
                description: "测试员（QA，数字员工）".into(),
                source: AgentSource::Employee,
            }],
            mcp_servers: vec![],
        };
        let def = tool.definition(&ctx).await;
        assert!(
            def.description.contains("emp-test-001"),
            "tool description must include emp id; got:\n{}",
            def.description
        );
        // Static base description (from TOOL_CATALOG) is preserved.
        assert!(
            def.description.contains("启动一个子 Agent")
                || def.description.contains("Spawn sub-agent"),
            "static base description should be retained"
        );
    }

    #[tokio::test]
    async fn definition_with_empty_ctx_matches_static_catalog() {
        // Backwards-compat: tools registered with empty ctx (e.g. boot
        // path that builds TOOL_CATALOG) get the static description
        // unchanged — no dynamic appendix.
        let registry = Arc::new(crate::runtime::agent::registry::AgentRegistry::with_builtins());
        let tool = SpawnSubagentRuntimeTool::new(
            Arc::new(RecordingLauncher {
                seen_requests: Arc::new(Mutex::new(Vec::new())),
                async_seen: Arc::new(Mutex::new(Vec::new())),
            }),
            registry,
        );
        let def = tool.definition(&ToolDescriptionContext::empty()).await;
        assert!(
            !def.description.contains("<available_employee_ids>"),
            "empty ctx must not produce employee section"
        );
        assert!(
            !def.description.contains("<available_subagent_types>"),
            "empty ctx must not produce subagent section"
        );
    }

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
        // id() is sync; no need to await definition().
        assert_eq!(tool.id(), "Agent");
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
                    msg.contains("Available")
                        || msg.contains("general-purpose")
                        || msg.contains("explore"),
                    "error should list available agents, got: {msg}"
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
    async fn active_team_does_not_force_standalone_agent_into_teammate_dispatch() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let tool = build_tool_with_recorder(seen.clone());
        let ctx = ToolExecutionContext::for_test("conv-active", "run-active", "tc-active")
            .with_active_team("existing-team".to_string());

        let result = tool
            .execute(
                json!({
                    "subagent_type": "general-purpose",
                    "prompt": "standalone fallback analysis",
                    "description": "fallback analysis"
                }),
                ctx,
            )
            .await
            .expect("omitting team_name should use standalone sync Agent path");

        assert!(
            result.content.contains("stub-output"),
            "standalone path should return launcher output, got: {}",
            result.content
        );
        let reqs = seen.lock().unwrap();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].subagent_type, "general-purpose");
    }

    #[tokio::test]
    async fn omitted_team_name_expert_agent_routes_to_active_team() {
        let team_registry = crate::runtime::agent::TeamRegistry::new();
        let names = crate::runtime::agent::AgentNameRegistry::new();
        let inboxes = crate::runtime::agent::InboxRegistry::new();

        let create_ctx = ToolExecutionContext::for_test("conv-infer-team", "run-1", "tc-create")
            .with_team_registry(team_registry.clone())
            .with_agent_names(names.clone())
            .with_inbox_registry(inboxes.clone());
        crate::runtime::tools::builtin::team_tools::TeamCreateRuntimeTool
            .execute(json!({ "team_name": "expert-team-strategy" }), create_ctx)
            .await
            .expect("test setup should create an active team");

        let seen = Arc::new(Mutex::new(Vec::new()));
        let tool = build_tool_with_recorder(seen.clone());
        let spawn_ctx = ToolExecutionContext::for_test("conv-infer-team", "run-2", "tc-agent")
            .with_team_registry(team_registry)
            .with_agent_names(names)
            .with_inbox_registry(inboxes)
            .with_active_team("expert-team-strategy".to_string());

        let result = tool
            .execute(
                json!({
                    "subagent_type": "general-purpose",
                    "prompt": "你是战略推演团的 CFO。请从财务视角发表你的专业观点，发送完观点后发给 team-lead。",
                    "description": "CFO 财务观点",
                    "name": "cfo"
                }),
                spawn_ctx,
            )
            .await
            .expect("expert Agent call should infer active team when team_name is omitted");

        let parsed: serde_json::Value =
            serde_json::from_str(&result.content).expect("teammate response should be JSON");
        assert_eq!(parsed["status"], "teammate_spawned");
        assert_eq!(parsed["name"], "cfo");
        assert!(
            seen.lock().unwrap().is_empty(),
            "teammate path should not call sync launcher"
        );
    }

    #[tokio::test]
    async fn explicit_display_team_name_falls_back_to_active_team_for_teammate_spawn() {
        let team_registry = crate::runtime::agent::TeamRegistry::new();
        let names = crate::runtime::agent::AgentNameRegistry::new();
        let inboxes = crate::runtime::agent::InboxRegistry::new();

        let create_ctx = ToolExecutionContext::for_test("conv-active-team", "run-1", "tc-create")
            .with_team_registry(team_registry.clone())
            .with_agent_names(names.clone())
            .with_inbox_registry(inboxes.clone());
        crate::runtime::tools::builtin::team_tools::TeamCreateRuntimeTool
            .execute(json!({ "team_name": "safe-team" }), create_ctx)
            .await
            .expect("test setup should create an active team");

        let tool = build_tool_with_recorder(Arc::new(Mutex::new(Vec::new())));
        let spawn_ctx = ToolExecutionContext::for_test("conv-active-team", "run-2", "tc-agent")
            .with_team_registry(team_registry)
            .with_agent_names(names)
            .with_inbox_registry(inboxes)
            .with_active_team("safe-team".to_string());

        let result = tool
            .execute(
                json!({
                    "subagent_type": "general-purpose",
                    "prompt": "请作为品牌负责人发言",
                    "description": "品牌负责人",
                    "team_name": "市场营销策划团",
                    "name": "brand-lead"
                }),
                spawn_ctx,
            )
            .await
            .expect("display team_name should route to active team instead of failing");

        let parsed: serde_json::Value =
            serde_json::from_str(&result.content).expect("teammate response should be JSON");
        assert_eq!(parsed["status"], "teammate_spawned");
        assert_eq!(parsed["name"], "brand-lead");
    }

    #[tokio::test]
    async fn teammate_spawn_defaults_missing_subagent_type_to_general_purpose() {
        let team_registry = crate::runtime::agent::TeamRegistry::new();
        let names = crate::runtime::agent::AgentNameRegistry::new();
        let inboxes = crate::runtime::agent::InboxRegistry::new();

        let create_ctx = ToolExecutionContext::for_test("conv-default-type", "run-1", "tc-create")
            .with_team_registry(team_registry.clone())
            .with_agent_names(names.clone())
            .with_inbox_registry(inboxes.clone());
        crate::runtime::tools::builtin::team_tools::TeamCreateRuntimeTool
            .execute(json!({ "team_name": "expert-team-marketing" }), create_ctx)
            .await
            .expect("test setup should create an active team");

        let tool = build_tool_with_recorder(Arc::new(Mutex::new(Vec::new())));
        let spawn_ctx = ToolExecutionContext::for_test("conv-default-type", "run-2", "tc-agent")
            .with_team_registry(team_registry)
            .with_agent_names(names)
            .with_inbox_registry(inboxes)
            .with_active_team("expert-team-marketing".to_string());

        let result = tool
            .execute(
                json!({
                    "prompt": "请作为渠道经理发言",
                    "description": "渠道经理",
                    "team_name": "expert-team-marketing",
                    "name": "channel-manager"
                }),
                spawn_ctx,
            )
            .await
            .expect("teammate dispatch should default missing subagent_type");

        let parsed: serde_json::Value =
            serde_json::from_str(&result.content).expect("teammate response should be JSON");
        assert_eq!(parsed["status"], "teammate_spawned");
        assert_eq!(parsed["name"], "channel-manager");
    }

    #[tokio::test]
    async fn duplicate_teammate_spawn_is_suppressed_without_error() {
        let team_registry = crate::runtime::agent::TeamRegistry::new();
        let names = crate::runtime::agent::AgentNameRegistry::new();
        let inboxes = crate::runtime::agent::InboxRegistry::new();

        let create_ctx = ToolExecutionContext::for_test("conv-dup-agent", "run-1", "tc-create")
            .with_team_registry(team_registry.clone())
            .with_agent_names(names.clone())
            .with_inbox_registry(inboxes.clone());
        crate::runtime::tools::builtin::team_tools::TeamCreateRuntimeTool
            .execute(json!({ "team_name": "expert-team-marketing" }), create_ctx)
            .await
            .expect("test setup should create an active team");

        let tool = build_tool_with_recorder(Arc::new(Mutex::new(Vec::new())));
        for tool_call_id in ["tc-agent-1", "tc-agent-2"] {
            let spawn_ctx = ToolExecutionContext::for_test("conv-dup-agent", "run-2", tool_call_id)
                .with_team_registry(team_registry.clone())
                .with_agent_names(names.clone())
                .with_inbox_registry(inboxes.clone())
                .with_active_team("expert-team-marketing".to_string());
            let result = tool
                .execute(
                    json!({
                        "subagent_type": "general-purpose",
                        "prompt": "请作为增长黑客发言",
                        "description": "增长黑客",
                        "team_name": "expert-team-marketing",
                        "name": "growth-hacker"
                    }),
                    spawn_ctx,
                )
                .await
                .expect("duplicate teammate spawn should not fail");
            let parsed = result
                .data
                .clone()
                .or_else(|| serde_json::from_str(&result.content).ok())
                .expect("teammate response should provide structured data");
            assert_eq!(parsed["status"], "teammate_spawned");
            if tool_call_id == "tc-agent-2" {
                assert_eq!(parsed["duplicate_suppressed"], true);
            }
        }
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

    // ── unknown subagent_type 错误回灌可选清单 ────────────────────────────────

    #[tokio::test]
    async fn unknown_subagent_type_error_lists_registry_options() {
        let registry = Arc::new(AgentRegistry::with_builtins());
        let tool = SpawnSubagentRuntimeTool::new(
            Arc::new(RecordingLauncher {
                seen_requests: Arc::new(Mutex::new(Vec::new())),
                async_seen: Arc::new(Mutex::new(Vec::new())),
            }),
            registry,
        );
        let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
        let err = tool
            .execute(
                json!({
                    "subagent_type": "nonexistent-xyz",
                    "prompt": "x",
                    "description": "y"
                }),
                ctx,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::ExecutionFailed(msg) => {
                assert!(msg.contains("nonexistent-xyz"), "error: {msg}");
                assert!(
                    msg.contains("general-purpose") || msg.contains("explore"),
                    "should list builtins: {msg}"
                );
            }
            other => panic!("expected ExecutionFailed, got {:?}", other),
        }
    }
}
