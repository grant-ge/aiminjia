//! `TeammateStop` runtime tool (P2.7) — forcibly cancels a Teammate by name.
//!
//! Used by the Lead when graceful shutdown handshake fails or in emergency
//! situations.  Trips the target's `CancellationToken`, which causes
//! `worker_runtime::run_teammate_idle` to take the cancelled branch and run
//! `cleanup_teammate`.  Idempotent — stopping an already-stopped Teammate
//! succeeds silently because the lookup returns `None` and we don't treat
//! that as an error.
//!
//! Distinct from the legacy `TaskStop` tool which targets async sub-agents
//! by `task_id` (= AgentId) in the `AsyncAgentTaskStore`.  This tool resolves
//! by **name** in `AgentNameRegistry` + `CancellationRegistry`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::runtime::cancellation::CancellationReason;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;
use crate::telemetry::{record_diagnostic, DiagnosticEvent, DiagnosticSource};

pub struct TeammateStopRuntimeTool;

#[async_trait]
impl RuntimeTool for TeammateStopRuntimeTool {
    fn id(&self) -> &str { "TeammateStop" }
    
    async fn definition(&self, _ctx: &crate::runtime::tools::ToolDescriptionContext) -> ToolDefinition {
        TOOL_CATALOG.get("TeammateStop").unwrap_or_else(|| {
            ToolDefinition::new(
                "TeammateStop",
                "Forcibly cancel a Teammate by name (Lead-only emergency tool).",
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
        let name = input
            .get("agent_name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                ToolError::ExecutionFailed(
                    "missing required string field `agent_name`".into(),
                )
            })?
            .to_string();

        let ws = crate::telemetry::diagnostics_workspace();
        record_diagnostic(
            &ws,
            DiagnosticEvent::new("tool.teammate_stop.entry", DiagnosticSource::Backend)
                .conversation_id(ctx.session_id.as_str())
                .run_id(ctx.run_id.as_str())
                .tool_call_id(ctx.tool_call_id.as_str())
                .payload(serde_json::json!({ "agent_name": name })),
        );

        let names = ctx.agent_names.clone().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "agent_names registry not configured — TeammateStop requires LTR wiring".into(),
            )
        })?;
        let cancels = ctx.cancellation_registry.clone().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "cancellation_registry not configured — TeammateStop requires LTR wiring"
                    .into(),
            )
        })?;

        let team_name = ctx.active_team_name.as_deref().unwrap_or("");
        // Resolve name -> AgentId; absence is treated as a soft success
        // (idempotent — the Teammate has already exited or never existed).
        let agent_id = match names.resolve(&ctx.session_id, team_name, &name).await {
            Some(id) => id,
            None => {
                record_diagnostic(
                    &ws,
                    DiagnosticEvent::new("tool.teammate_stop.not_found", DiagnosticSource::Backend)
                        .conversation_id(ctx.session_id.as_str())
                        .run_id(ctx.run_id.as_str())
                        .tool_call_id(ctx.tool_call_id.as_str())
                        .ok(true)
                        .payload(serde_json::json!({ "agent_name": name, "reason": "not_found" })),
                );
                return Ok(ToolResult::new(
                    "TeammateStop",
                    format!(
                        "no agent named `{name}` in this session — assuming already stopped (noop)"
                    ),
                    Some(json!({ "stopped": false, "agent_name": name, "reason": "not_found" })),
                ));
            }
        };

        match cancels.get(&ctx.session_id, team_name, &agent_id).await {
            Some(token) => {
                token.cancel_with_reason(CancellationReason::UserCancel);
                record_diagnostic(
                    &ws,
                    DiagnosticEvent::new("tool.teammate_stop.cancelled", DiagnosticSource::Backend)
                        .conversation_id(ctx.session_id.as_str())
                        .run_id(ctx.run_id.as_str())
                        .tool_call_id(ctx.tool_call_id.as_str())
                        .agent_id(agent_id.as_str())
                        .ok(true)
                        .payload(serde_json::json!({ "agent_name": name })),
                );
                Ok(ToolResult::new(
                    "TeammateStop",
                    format!("Teammate `{name}` cancelled"),
                    Some(json!({
                        "stopped": true,
                        "agent_name": name,
                        "agent_id": agent_id.as_str(),
                    })),
                ))
            }
            None => {
                record_diagnostic(
                    &ws,
                    DiagnosticEvent::new("tool.teammate_stop.no_cancel_token", DiagnosticSource::Backend)
                        .conversation_id(ctx.session_id.as_str())
                        .run_id(ctx.run_id.as_str())
                        .tool_call_id(ctx.tool_call_id.as_str())
                        .agent_id(agent_id.as_str())
                        .ok(true)
                        .payload(serde_json::json!({ "agent_name": name, "reason": "no_cancel_token" })),
                );
                Ok(ToolResult::new(
                    "TeammateStop",
                    format!(
                        "agent `{name}` resolved but has no cancellation token registered — noop"
                    ),
                    Some(json!({
                        "stopped": false,
                        "agent_name": name,
                        "reason": "no_cancel_token",
                    })),
                ))
            }
        }
    }
}
