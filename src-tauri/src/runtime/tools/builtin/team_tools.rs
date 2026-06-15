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

use crate::runtime::agent::team_paths::validate_team_name;
use crate::runtime::agent::{Member, MemberRole};
use crate::runtime::ids::AgentId;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;
use crate::storage::file_store::types::ConversationMeta;
use crate::telemetry::{record_diagnostic, DiagnosticEvent, DiagnosticSource};

/// Canonical name registered for the Lead member; teammates address it as
/// `SendMessage(to: "team-lead")`.
pub const LEAD_NAME: &str = "team-lead";

fn default_team_name(session_id: &str) -> String {
    let short: String = session_id.chars().take(8).collect();
    format!("team-{short}")
}

/// Update `<conv_dir>/conv.json::active_team_name` atomically. Best-effort:
/// callers log warnings on failure but do not fail the tool. 文件不存在或无法解析
/// 时静默跳过——`conv.json` 由 SessionRuntime 在 conv 创建时写入，正常路径下必然存在。
fn update_conv_meta_active_team(
    conv_dir: &std::path::Path,
    name: Option<&str>,
) -> std::io::Result<()> {
    let path = conv_dir.join("conv.json");
    if !path.exists() {
        // 没有 meta 文件就不写——避免无中生有创造一个不完整的 ConversationMeta。
        return Ok(());
    }
    let bytes = std::fs::read(&path)?;
    let mut value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if let Some(obj) = value.as_object_mut() {
        match name {
            Some(n) => {
                obj.insert("active_team_name".to_string(), serde_json::json!(n));
            }
            None => {
                obj.insert("active_team_name".to_string(), serde_json::Value::Null);
            }
        }
    }
    let bytes = serde_json::to_vec_pretty(&value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    crate::storage::fs_atomic::write_atomic(&path, &bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
}

// ─── TeamCreate ───────────────────────────────────────────────────────────────

pub struct TeamCreateRuntimeTool;

#[async_trait]
impl RuntimeTool for TeamCreateRuntimeTool {
    fn id(&self) -> &str {
        "TeamCreate"
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
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

        let requested_team_name = team_name_input.clone();
        let mut team_name = team_name_input.unwrap_or_else(|| default_team_name(session.as_str()));

        // The LLM sometimes passes the human-facing display name (often
        // Chinese) even when the prompt supplies an internal ASCII name. Keep
        // TeamPaths strict, but normalize the tool boundary to a stable safe
        // fallback instead of burning a failed tool round.
        if let Err(e) = validate_team_name(&team_name) {
            let fallback = default_team_name(session.as_str());
            log::warn!(
                "[TeamCreate] invalid team_name {:?}: {}; using fallback {}",
                requested_team_name,
                e,
                fallback
            );
            team_name = fallback;
        }

        let ws = crate::telemetry::diagnostics_workspace();
        record_diagnostic(
            &ws,
            DiagnosticEvent::new("tool.team_create.entry", DiagnosticSource::Backend)
                .conversation_id(session.as_str())
                .run_id(ctx.run_id.as_str())
                .tool_call_id(ctx.tool_call_id.as_str())
                .agent_id(lead_id.as_str())
                .team_name(team_name.as_str())
                .payload(serde_json::json!({
                    "team_name": team_name,
                    "requested_team_name": requested_team_name,
                })),
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
            .register(&session, &team_name, LEAD_NAME, lead_id.clone())
            .await
            .map_err(|e| {
                // Roll back the team to keep state consistent.
                let registry = ctx.team_registry().clone();
                let session_for_rollback = session.clone();
                let tname_rb = team_name.clone();
                tokio::spawn(async move {
                    let _ = registry.delete_team(&session_for_rollback, &tname_rb).await;
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
                .register(&session, &team_name, lead_id.clone(), lead_inbox)
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
                .team_name(team_name.as_str())
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
            match ctx
                .team_registry()
                .persist(&session, &team_name, conv_dir)
                .await
            {
                Ok(()) => log::info!(
                    "[TeamCreate] persisted config.json at {}",
                    conv_dir
                        .join("teams")
                        .join(&team_name)
                        .join("config.json")
                        .display()
                ),
                Err(e) => log::warn!("[TeamCreate] persist config.json failed: {e}"),
            }
            // PR5: 写 conv.json::active_team_name，让重启 hydration 后能恢复 active team。
            if let Err(e) = update_conv_meta_active_team(conv_dir, Some(&team_name)) {
                log::warn!("[TeamCreate] update conv.json active_team_name failed: {e}");
            }
        } else {
            log::warn!(
                "[TeamCreate] ctx.conv_dir is None — team.json NOT persisted (this means SessionRuntime didn't inject conv_dir into the QueryEngine used by this tool call)"
            );
        }

        // Mark this team as the session's active team in the registry — the
        // single owner of active state. attach_ltr_registries on the NEXT
        // tool call in this turn will derive ctx.active_team_name from this
        // value (replaces the old QueryEngine.active_team_name cache that
        // could not be updated mid-turn).
        ctx.team_registry()
            .set_active(&session, team_name.clone())
            .await;

        Ok(ToolResult::new(
            "TeamCreate",
            format!(
                "Team `{team_name}` created for session `{}` with Lead `{LEAD_NAME}`",
                session.as_str()
            ),
            Some(json!({
                "team_name": team_name,
                "requested_team_name": requested_team_name,
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
    fn id(&self) -> &str {
        "TeamDelete"
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        TOOL_CATALOG.get("TeamDelete").unwrap_or_else(|| {
            ToolDefinition::new("TeamDelete", "Exit Team mode and dismiss all teammates.")
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
        let session = ctx.session_id.clone();
        let ws = crate::telemetry::diagnostics_workspace();

        // PR5: TeamDelete 现在按 team_name 精确删除。如果未提供，从 ctx.active_team_name 取。
        let team_name = input
            .get("team_name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| ctx.active_team_name.clone())
            .ok_or_else(|| {
                ToolError::ExecutionFailed(
                    "TeamDelete requires team_name (no active team in this conversation)".into(),
                )
            })?;

        record_diagnostic(
            &ws,
            DiagnosticEvent::new("tool.team_delete.entry", DiagnosticSource::Backend)
                .conversation_id(session.as_str())
                .run_id(ctx.run_id.as_str())
                .tool_call_id(ctx.tool_call_id.as_str())
                .team_name(team_name.as_str())
                .payload(json!({ "team_name": team_name })),
        );

        // 严格 cancel → 等待 → delete_team → rm -rf → idempotent sweep → active_team 重置
        // 顺序不可调换：先 cancel 让 worker 自我清理，再 delete 防止 race。

        // step a: 取消该 team 内所有 Teammate 的 cancel token
        let cancelled = if let Some(reg) = ctx.cancellation_registry.as_ref() {
            reg.cancel_team(&session, &team_name).await
        } else {
            0
        };

        // step b: 等 worker idle loop 在下一次 tokio::select! 检查到取消，自然退出 + 自清理
        if cancelled > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        // step c: 从 in-memory registry 移除该 team
        let team_handle = ctx.team_registry().delete_team(&session, &team_name).await;
        let teammate_count = if let Some(handle) = team_handle.as_ref() {
            let t = handle.lock().await;
            t.teammates.len()
        } else {
            0
        };
        let team_existed = team_handle.is_some();

        // step d: 软删除 — 写 `deleted_at = Utc::now()` 到 config.json，
        // teams/{name}/ 目录保留供历史回看（spec §6.A：抽屉时间线包括协议
        // 握手消息；解散后用户仍要能看完整对话）。如果以后要做"过期 team
        // 自动清理"，可以扫 deleted_at 早于 N 天的目录调用
        // `delete_persisted_team` 兜底回收。
        if let Some(ref conv_dir) = ctx.conv_dir {
            if let Err(e) =
                crate::runtime::agent::TeamRegistry::mark_deleted_on_disk(conv_dir, &team_name)
            {
                log::warn!("[TeamDelete] mark teams/{team_name}/config.json deleted failed: {e}");
            }
        }

        // step e: idempotent sweep — cleanup_teammate 已经清过单条，这里兜底
        if let Some(reg) = ctx.agent_names.as_ref() {
            reg.unregister_team(&session, &team_name).await;
        }
        if let Some(reg) = ctx.inbox_registry.as_ref() {
            reg.unregister_team(&session, &team_name).await;
        }

        // step f: 若此 team 是 active，清掉。两层：① TeamRegistry 内存
        // 单一 owner（同 turn 后续 tool 立刻看到）；② conv.json 持久化镜
        // 像（重启 hydration 用）。
        if ctx.team_registry().active(&session).await.as_deref() == Some(team_name.as_str()) {
            ctx.team_registry().clear_active(&session).await;
        }
        if let Some(ref conv_dir) = ctx.conv_dir {
            let path = conv_dir.join("conv.json");
            if let Ok(bytes) = std::fs::read(&path) {
                if let Ok(meta) = serde_json::from_slice::<ConversationMeta>(&bytes) {
                    if meta.active_team_name.as_deref() == Some(&team_name) {
                        if let Err(e) = update_conv_meta_active_team(conv_dir, None) {
                            log::warn!("[TeamDelete] reset conv.json active_team_name failed: {e}");
                        }
                    }
                }
            }
        }

        record_diagnostic(
            &ws,
            DiagnosticEvent::new("tool.team_delete.completed", DiagnosticSource::Backend)
                .conversation_id(session.as_str())
                .run_id(ctx.run_id.as_str())
                .tool_call_id(ctx.tool_call_id.as_str())
                .team_name(team_name.as_str())
                .ok(true)
                .payload(json!({
                    "team_existed": team_existed,
                    "team_name": team_name,
                    "teammates_dismissed": teammate_count,
                    "tokens_cancelled": cancelled,
                })),
        );

        let result_json = json!({
            "session_id": session.as_str(),
            "team_existed": team_existed,
            "team_name": team_name,
            "teammates_dismissed": teammate_count,
        });

        let msg = if team_existed {
            format!("Team `{team_name}` deleted; {teammate_count} teammate(s) dismissed.")
        } else {
            format!("Team `{team_name}` did not exist — TeamDelete is a noop.")
        };

        Ok(ToolResult::new("TeamDelete", msg, Some(result_json)))
    }
}

// ─── TeamSwitch ───────────────────────────────────────────────────────────────

/// Switch the conversation's active team. The Lead's subsequent tool calls
/// will route through this team's directories. Idempotent — switching to the
/// already-active team is a noop.
pub struct TeamSwitchRuntimeTool;

#[async_trait]
impl RuntimeTool for TeamSwitchRuntimeTool {
    fn id(&self) -> &str {
        "TeamSwitch"
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        TOOL_CATALOG.get("TeamSwitch").unwrap_or_else(|| {
            ToolDefinition::new(
                "TeamSwitch",
                "Switch the conversation's active team. Subsequent tool calls route to the new team.",
            )
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
        let team_name = input
            .get("team_name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ToolError::ExecutionFailed("missing team_name".into()))?
            .to_string();

        validate_team_name(&team_name)
            .map_err(|e| ToolError::ExecutionFailed(format!("invalid team_name: {e}")))?;

        let session = ctx.session_id.clone();

        // 校验 team 存在
        if ctx
            .team_registry()
            .get(&session, &team_name)
            .await
            .is_none()
        {
            return Err(ToolError::ExecutionFailed(format!(
                "team `{team_name}` not found in this conversation"
            )));
        }

        // 写 conv.json::active_team_name
        let prev = ctx.active_team_name.clone();
        if let Some(ref conv_dir) = ctx.conv_dir {
            update_conv_meta_active_team(conv_dir, Some(&team_name))
                .map_err(|e| ToolError::ExecutionFailed(format!("write conv.json failed: {e}")))?;
        }

        // 同步刷新 in-memory 单一 owner 让同 turn 后续 tool 看到新值。
        ctx.team_registry()
            .set_active(&session, team_name.clone())
            .await;

        let ws = crate::telemetry::diagnostics_workspace();
        record_diagnostic(
            &ws,
            DiagnosticEvent::new("tool.team_switch.completed", DiagnosticSource::Backend)
                .conversation_id(session.as_str())
                .run_id(ctx.run_id.as_str())
                .tool_call_id(ctx.tool_call_id.as_str())
                .team_name(team_name.as_str())
                .ok(true)
                .payload(json!({
                    "old_team_name": prev,
                    "new_team_name": team_name,
                })),
        );

        Ok(ToolResult::new(
            "TeamSwitch",
            format!("Switched active team to `{team_name}`"),
            Some(json!({
                "team_name": team_name,
                "previous_team_name": prev,
            })),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::agent::{AgentNameRegistry, InboxRegistry, TeamRegistry};

    #[test]
    fn default_team_name_uses_short_session_id() {
        assert_eq!(default_team_name("abcdef1234567"), "team-abcdef12");
        assert_eq!(default_team_name("short"), "team-short");
    }

    /// Regression for the bug observed in conversation 0a31f41a (2026-05-15):
    /// after the Lead calls TeamCreate, the same turn's subsequent tool calls
    /// must see the new active team. We previously cached active_team_name in
    /// QueryEngine and never refreshed it mid-turn — so SendMessage looked up
    /// the name registry under team_name="" and missed every teammate that
    /// was registered under the real team name, the Lead then gave up and
    /// called TeamDelete. The fix is to make TeamRegistry the single owner
    /// of "which team is active" and have tools mutate it directly.
    #[tokio::test]
    async fn team_create_publishes_active_to_registry_immediately() {
        let team_registry = TeamRegistry::new();
        let names = AgentNameRegistry::new();
        let inboxes = InboxRegistry::new();
        let ctx = ToolExecutionContext::for_test("conv-regression", "run-1", "call-tc")
            .with_team_registry(team_registry.clone())
            .with_agent_names(names.clone())
            .with_inbox_registry(inboxes.clone());

        let session = ctx.session_id.clone();
        // Sanity: registry starts with no active team.
        assert_eq!(team_registry.active(&session).await, None);

        TeamCreateRuntimeTool
            .execute(serde_json::json!({ "team_name": "regression-team" }), ctx)
            .await
            .expect("TeamCreate should succeed");

        // The whole point of the fix: active is set in the same turn.
        assert_eq!(
            team_registry.active(&session).await,
            Some("regression-team".to_string()),
            "TeamCreate must publish active_team_name to TeamRegistry so the \
             next tool call in the same turn sees it"
        );
    }

    #[tokio::test]
    async fn team_create_falls_back_when_llm_uses_display_name() {
        let team_registry = TeamRegistry::new();
        let names = AgentNameRegistry::new();
        let inboxes = InboxRegistry::new();
        let ctx = ToolExecutionContext::for_test("conv-display-name", "run-1", "call-tc")
            .with_team_registry(team_registry.clone())
            .with_agent_names(names.clone())
            .with_inbox_registry(inboxes.clone());
        let session = ctx.session_id.clone();

        let result = TeamCreateRuntimeTool
            .execute(serde_json::json!({ "team_name": "市场营销策划团" }), ctx)
            .await
            .expect("display-name team_name should be normalized instead of failing");

        assert_eq!(
            team_registry.active(&session).await,
            Some("team-conv-dis".into())
        );
        let data = result
            .data
            .expect("TeamCreate should return structured data");
        assert_eq!(data["team_name"], "team-conv-dis");
        assert_eq!(data["requested_team_name"], "市场营销策划团");
    }

    #[tokio::test]
    async fn team_delete_clears_active_when_deleted_team_was_active() {
        let team_registry = TeamRegistry::new();
        let names = AgentNameRegistry::new();
        let inboxes = InboxRegistry::new();
        let session_str = "conv-delete";
        let ctx_create = ToolExecutionContext::for_test(session_str, "run-1", "call-tc")
            .with_team_registry(team_registry.clone())
            .with_agent_names(names.clone())
            .with_inbox_registry(inboxes.clone());
        let session = ctx_create.session_id.clone();
        TeamCreateRuntimeTool
            .execute(serde_json::json!({ "team_name": "to-delete" }), ctx_create)
            .await
            .unwrap();
        assert_eq!(
            team_registry.active(&session).await,
            Some("to-delete".into())
        );

        let ctx_delete = ToolExecutionContext::for_test(session_str, "run-2", "call-td")
            .with_team_registry(team_registry.clone())
            .with_agent_names(names.clone())
            .with_inbox_registry(inboxes.clone());
        TeamDeleteRuntimeTool
            .execute(serde_json::json!({ "team_name": "to-delete" }), ctx_delete)
            .await
            .unwrap();

        assert_eq!(
            team_registry.active(&session).await,
            None,
            "TeamDelete on the active team must clear active in the registry"
        );
    }
}
