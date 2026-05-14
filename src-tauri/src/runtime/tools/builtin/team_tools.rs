//! TeamCreate / TeamDelete builtin tools (P1.7).
//!
//! These tools let the Lead LLM explicitly enter and leave "Team mode" for a
//! session.  Team mode is **opt-in** — there is no implicit promotion.
//!
//! - `TeamCreate` seeds the session's Team with the calling agent as Lead and
//!   registers the name `team-lead` in `AgentNameRegistry` so teammates can
//!   address the Lead via `SendMessage(to: "team-lead")`.
//! - `TeamDelete` cascades cancellation to every Teammate's worker (P1.6
//!   `run_teammate_idle` exits when its parent cancel token fires) and clears
//!   the registry entry.  The actual per-Teammate cancel-token plumbing arrives
//!   with P1.8 (session-lifecycle hook).  In P1.7 we drop the Team from the
//!   registry and clear name bindings; outstanding tasks self-clean via the
//!   inbox-closed path.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::runtime::agent::{Member, MemberRole};
use crate::runtime::ids::AgentId;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;
use crate::telemetry::{record_diagnostic, DiagnosticEvent, DiagnosticSource};

/// Canonical name registered for the Lead member; teammates address it as
/// `SendMessage(to: "team-lead")`.
pub const LEAD_NAME: &str = "team-lead";

fn default_team_name(session_id: &str) -> String {
    let short: String = session_id.chars().take(8).collect();
    format!("team-{short}")
}

// ─── TeamCreate ───────────────────────────────────────────────────────────────

pub struct TeamCreateRuntimeTool;

#[async_trait]
impl RuntimeTool for TeamCreateRuntimeTool {
    fn id(&self) -> &str { "TeamCreate" }
    
    async fn definition(&self, _ctx: &crate::runtime::tools::ToolDescriptionContext) -> ToolDefinition {
        TOOL_CATALOG.get("TeamCreate").unwrap_or_else(|| {
            ToolDefinition::new("TeamCreate", "Mark this session as a multi-agent Team.")
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let team_name_input = input
            .get("team_name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let session = ctx.session_id.clone();
        let lead_id = ctx
            .agent_id
            .clone()
            .unwrap_or_else(|| AgentId::new(format!("lead-{}", session.as_str())));

        let team_name = team_name_input.unwrap_or_else(|| default_team_name(session.as_str()));

        let ws = crate::telemetry::diagnostics_workspace();
        record_diagnostic(
            &ws,
            DiagnosticEvent::new("tool.team_create.entry", DiagnosticSource::Backend)
                .conversation_id(session.as_str())
                .run_id(ctx.run_id.as_str())
                .tool_call_id(ctx.tool_call_id.as_str())
                .agent_id(lead_id.as_str())
                .payload(serde_json::json!({ "team_name": team_name })),
        );

        let lead = Member {
            agent_id: lead_id.clone(),
            name: LEAD_NAME.to_string(),
            role: MemberRole::Lead,
            created_at: chrono::Utc::now(),
            last_active_at: chrono::Utc::now(),
        };

        ctx.team_registry()
            .create(session.clone(), lead, team_name.clone())
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        // Register the Lead's name so teammates can SendMessage(to: "team-lead").
        // Duplicates here mean the Lead has already entered Team mode under a
        // stale registration — surface that as a real error.
        ctx.agent_names()
            .register(&session, LEAD_NAME, lead_id.clone())
            .await
            .map_err(|e| {
                // Roll back the team to keep state consistent.
                let registry = ctx.team_registry().clone();
                let session_for_rollback = session.clone();
                tokio::spawn(async move {
                    let _ = registry.delete(&session_for_rollback).await;
                });
                ToolError::ExecutionFailed(format!(
                    "Failed to register Lead name `{LEAD_NAME}`: {e}"
                ))
            })?;

        // LTR: register an inbox for the Lead so teammates can deliver via
        // `SendMessage(to: "team-lead")`.  Without this, send_message.rs
        // resolves the Lead's name but errors with "registered but has no
        // inbox".  We use soft-injection here: production paths (lib.rs)
        // always wire `inbox_registry`, but legacy/test paths that build a
        // `ToolExecutionContext` without LTR registries still work — they
        // just lose the inbox routing (which they don't exercise anyway).
        let lead_inbox_registered = if let Some(inbox_reg) = ctx.inbox_registry.as_ref() {
            let lead_inbox = crate::runtime::agent::AgentInbox::new(64);
            inbox_reg
                .register(&session, lead_id.clone(), lead_inbox)
                .await;
            true
        } else {
            log::warn!(
                "[TeamCreate] inbox_registry not injected — Lead inbox will not be \
                 reachable via SendMessage(to: \"team-lead\"). This is expected for \
                 unit tests; production paths must wire with_ltr_registries()."
            );
            false
        };

        // LTR P2 follow-up: align the LeadIdleSupervisor's view with reality.
        // The supervisor's state machine starts uninitialized; without this
        // mark_running, a teammate that calls SendMessage during the same
        // user turn that built the Team would hit the supervisor in its
        // default Idle state, fire wake_fn, and try to start a duplicate
        // continuation turn — which RunRegistry correctly rejects with
        // "This conversation is already processing", leaving the inbox
        // message sitting unread until the next external trigger.
        //
        // By marking running here we route the SendMessage through the
        // `already_running_pending_recorded` branch instead, so Path A
        // (mark_idle at user-turn end) will pick up the pending flag and
        // start the continuation turn at the natural boundary.
        let lead_supervisor_marked_running = if let Some(sup) = ctx.lead_idle.as_ref() {
            log::info!(
                "[TeamCreate][diag] mark_running invoked session={} lead_id={}",
                session.as_str(),
                lead_id.as_str()
            );
            sup.mark_running(&(session.clone(), lead_id.clone())).await;
            true
        } else {
            log::warn!(
                "[TeamCreate][diag] ctx.lead_idle is None — supervisor will not be \
                 marked Running; teammate SendMessage will likely fail to wake Lead"
            );
            false
        };

        record_diagnostic(
            &ws,
            DiagnosticEvent::new("tool.team_create.completed", DiagnosticSource::Backend)
                .conversation_id(session.as_str())
                .run_id(ctx.run_id.as_str())
                .tool_call_id(ctx.tool_call_id.as_str())
                .agent_id(lead_id.as_str())
                .ok(true)
                .payload(serde_json::json!({
                    "team_name": team_name,
                    "lead_name": LEAD_NAME,
                    "lead_inbox_registered": lead_inbox_registered,
                    "lead_supervisor_marked_running": lead_supervisor_marked_running,
                })),
        );

        // 持久化 team.json 到会话目录，供 teammate boot prompt 引用的
        // `团队配置: <conv_dir>/team.json` 路径生效。best-effort：内存
        // registry 是 source-of-truth，落盘失败只 warn。
        log::info!(
            "[TeamCreate][diag] persist check: conv_dir={:?} session={}",
            ctx.conv_dir.as_ref().map(|p| p.display().to_string()),
            session.as_str()
        );
        if let Some(ref conv_dir) = ctx.conv_dir {
            match ctx.team_registry().persist(&session, conv_dir).await {
                Ok(()) => log::info!(
                    "[TeamCreate] persisted team.json at {}",
                    conv_dir.join("team.json").display()
                ),
                Err(e) => log::warn!("[TeamCreate] persist team.json failed: {e}"),
            }
        } else {
            log::warn!(
                "[TeamCreate] ctx.conv_dir is None — team.json NOT persisted (this means SessionRuntime didn't inject conv_dir into the QueryEngine used by this tool call)"
            );
        }

        Ok(ToolResult::new(
            "TeamCreate",
            format!(
                "Team `{team_name}` created for session `{}` with Lead `{LEAD_NAME}`",
                session.as_str()
            ),
            Some(json!({
                "team_name": team_name,
                "session_id": session.as_str(),
                "lead_name": LEAD_NAME,
            })),
        ))
    }
}

// ─── TeamDelete ───────────────────────────────────────────────────────────────

pub struct TeamDeleteRuntimeTool;

#[async_trait]
impl RuntimeTool for TeamDeleteRuntimeTool {
    fn id(&self) -> &str { "TeamDelete" }
    
    async fn definition(&self, _ctx: &crate::runtime::tools::ToolDescriptionContext) -> ToolDefinition {
        TOOL_CATALOG.get("TeamDelete").unwrap_or_else(|| {
            ToolDefinition::new("TeamDelete", "Exit Team mode and dismiss all teammates.")
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn execute(
        &self,
        _input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let session = ctx.session_id.clone();
        let ws = crate::telemetry::diagnostics_workspace();

        record_diagnostic(
            &ws,
            DiagnosticEvent::new("tool.team_delete.entry", DiagnosticSource::Backend)
                .conversation_id(session.as_str())
                .run_id(ctx.run_id.as_str())
                .tool_call_id(ctx.tool_call_id.as_str()),
        );

        let team_handle = ctx.team_registry().delete(&session).await;
        let (team_name, teammate_count) = if let Some(handle) = team_handle.as_ref() {
            let t = handle.lock().await;
            (t.team_name.clone(), t.teammates.len())
        } else {
            (String::new(), 0)
        };

        // TeamDelete only dissolves the team logical grouping; per-session
        // registries (agent_names / inbox_registry / cancellation_registry /
        // lead_idle) are session-scoped resources and are cleaned up by the
        // session-close hook, not here. Clearing them mid-session breaks the
        // Lead's continuing identity and causes mark_idle to lose track of
        // the supervisor state.

        record_diagnostic(
            &ws,
            DiagnosticEvent::new("tool.team_delete.completed", DiagnosticSource::Backend)
                .conversation_id(session.as_str())
                .run_id(ctx.run_id.as_str())
                .tool_call_id(ctx.tool_call_id.as_str())
                .ok(true)
                .payload(serde_json::json!({
                    "team_existed": team_handle.is_some(),
                    "team_name": team_name,
                    "teammates_dismissed": teammate_count,
                })),
        );

        let json = json!({
            "session_id": session.as_str(),
            "team_existed": team_handle.is_some(),
            "team_name": team_name,
            "teammates_dismissed": teammate_count,
        });

        let msg = if team_handle.is_some() {
            format!(
                "Team `{team_name}` deleted; {teammate_count} teammate(s) dismissed."
            )
        } else {
            format!(
                "No team existed for session `{}` — TeamDelete is a noop.",
                session.as_str()
            )
        };

        // 同步删除磁盘上的 team.json。best-effort：内存 registry 已经在
        // 上面 delete() 了，磁盘清理失败不影响 tool 返回。
        if let Some(ref conv_dir) = ctx.conv_dir {
            if let Err(e) = crate::runtime::agent::TeamRegistry::delete_persisted(conv_dir) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("[TeamDelete] delete team.json failed: {e}");
                }
            }
        }

        Ok(ToolResult::new("TeamDelete", msg, Some(json)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_team_name_uses_short_session_id() {
        assert_eq!(default_team_name("abcdef1234567"), "team-abcdef12");
        assert_eq!(default_team_name("short"), "team-short");
    }
}
