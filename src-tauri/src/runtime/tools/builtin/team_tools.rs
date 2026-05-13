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
        let description_input = input
            .get("description")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let session = ctx.session_id.clone();
        let lead_id = ctx
            .agent_id
            .clone()
            .unwrap_or_else(|| AgentId::new(format!("lead-{}", session.as_str())));

        let team_name = team_name_input.unwrap_or_else(|| default_team_name(session.as_str()));

        // v0.3 decision #2: same conversation cannot hold two active teams.
        // Reject explicitly with a fixable error message (lead should TeamDelete
        // first if it wants a new team).
        if ctx.team_registry().get(&session).await.is_some() {
            return Err(ToolError::ExecutionFailed(
                "an active team already exists for this conversation — call TeamDelete first"
                    .to_string(),
            ));
        }

        let ws = crate::telemetry::diagnostics_workspace();
        record_diagnostic(
            &ws,
            DiagnosticEvent::new("tool.team_create.entry", DiagnosticSource::Backend)
                .conversation_id(session.as_str())
                .run_id(ctx.run_id.as_str())
                .tool_call_id(ctx.tool_call_id.as_str())
                .agent_id(lead_id.as_str())
                .payload(serde_json::json!({ "team_name": team_name, "description": description_input })),
        );

        let lead = Member {
            agent_id: lead_id.clone(),
            name: LEAD_NAME.to_string(),
            role: MemberRole::Lead,
            created_at: chrono::Utc::now(),
            last_active_at: chrono::Utc::now(),
            // Lead is always Active by construction (it ran TeamCreate to get here).
            status: crate::runtime::agent::MemberStatus::Active,
            stopped_at: None,
            stopped_reason: None,
        };

        let team_handle = ctx.team_registry()
            .create(session.clone(), lead, team_name.clone())
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        // Stamp description (the Team::new path doesn't take it; this stays
        // optional and only touches when the LLM supplied a non-empty value).
        if let Some(desc) = description_input.clone() {
            let mut t = team_handle.lock().await;
            t.description = Some(desc);
        }

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

        // v0.3: persist team.json so UI can read team state from disk as a
        // single source of truth (file::exists("team.json") = "this conv has
        // a team"). Best-effort: failure logs a warning but doesn't fail the
        // tool — the team still exists in memory and downstream actions work;
        // only the UI mirror is stale.
        if let Some(conv_dir) = ctx.conv_dir.as_ref() {
            if let Err(e) = ctx.team_registry().persist(&session, conv_dir).await {
                log::warn!(
                    "[TeamCreate] failed to persist team.json session={} err={}",
                    session.as_str(),
                    e
                );
            }
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

        // v0.3 soft delete sequence (decision #3 + bug fix for "Lead name already
        // registered" in b21ebe36):
        //
        // 1. Snapshot team in-memory, mark members Cancelled
        // 2. Archive snapshot to teams/history/{ts}.json
        // 3. Remove team.json from disk (UI sees "no active team")
        // 4. Drop team from registry
        // 5. **Unregister LEAD_NAME** — without this, next TeamCreate fails
        //    with "Lead name already registered"

        let team_handle = ctx.team_registry().get(&session).await;
        let (team_name, teammate_count, snapshot) = if let Some(handle) = team_handle.as_ref() {
            let mut t = handle.lock().await;
            t.disband("tool_call");
            let snap = crate::runtime::agent::TeamSnapshot::from(&*t);
            (t.team_name.clone(), t.teammates.len(), Some(snap))
        } else {
            (String::new(), 0, None)
        };

        // Archive history before dropping from memory + disk.
        let mut archive_ok = true;
        if let (Some(conv_dir), Some(snap)) = (ctx.conv_dir.as_ref(), snapshot.as_ref()) {
            if let Err(e) = crate::runtime::agent::TeamRegistry::archive_to_history(conv_dir, snap) {
                archive_ok = false;
                log::warn!(
                    "[TeamDelete] archive_to_history failed session={} err={}",
                    session.as_str(),
                    e
                );
            }
        }

        // Delete team.json from disk so UI sees "no active team".
        if let Some(conv_dir) = ctx.conv_dir.as_ref() {
            if let Err(e) = crate::runtime::agent::TeamRegistry::delete_persisted(conv_dir) {
                log::warn!(
                    "[TeamDelete] delete_persisted failed session={} err={}",
                    session.as_str(),
                    e
                );
            }
        }

        // Drop team from registry (we already snapshotted above).
        let _ = ctx.team_registry().delete(&session).await;

        // CRITICAL bug fix (b21ebe36): unregister LEAD_NAME so a subsequent
        // TeamCreate doesn't hit "Lead name already registered". v0.2 left
        // this dangling, causing the second TeamCreate in the same conv to
        // fail and break the entire group flow.
        ctx.agent_names()
            .unregister(&session, LEAD_NAME)
            .await;

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
                    "archived": archive_ok && snapshot.is_some(),
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
