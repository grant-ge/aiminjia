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
            sup.mark_running(&(session.clone(), lead_id.clone())).await;
            true
        } else {
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

        // Drop all name bindings for this session so the Lead can re-create a
        // Team later without collisions.  Teammate worker loops will exit via
        // the inbox-closed path once their senders are dropped (P1.6 cleanup).
        ctx.agent_names().drop_session(&session).await;

        // LTR (B-gap3): also drop the per-session inbox and cancellation
        // registry entries.  Without this, dead Teammate AgentIds linger in
        // the global registries — SendMessage routing could still hit them
        // and TeammateStop's by-name lookup would still find stale tokens.
        if let Some(inbox_reg) = ctx.inbox_registry.as_ref() {
            inbox_reg.drop_session(&session).await;
        }
        if let Some(cancel_reg) = ctx.cancellation_registry.as_ref() {
            cancel_reg.drop_session(&session).await;
        }

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
