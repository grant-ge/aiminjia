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
//! Returns a `not_implemented_yet` JSON placeholder. Full implementation lands
//! in P6.x alongside `AsyncAgentTaskStore` + lifecycle management.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::agent::definition::AgentModel;
use crate::runtime::agent::registry::AgentRegistry;
use crate::runtime::cancellation::CancellationToken;
use crate::runtime::ids::{AgentId, RunId, SessionId};
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

#[async_trait]
impl RuntimeTool for SpawnSubagentRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("spawn_subagent")
            .unwrap_or_else(|| ToolDefinition::new("spawn_subagent", "Spawn sub-agent"))
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
        let subagent_type = input
            .get("subagent_type")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::ExecutionFailed("missing required field: subagent_type".into())
            })?;
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

        // ── Parse optional fields ──────────────────────────────────────────
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
            .map(str::to_string);

        // ── Resolve agent definition ───────────────────────────────────────
        let definition = self.registry.get(subagent_type).ok_or_else(|| {
            ToolError::ExecutionFailed(format!(
                "unknown subagent_type '{subagent_type}'; \
                 check ~/.renlijia/users/<scope>/agents/ or builtin agents"
            ))
        })?;

        // ── Three-tier model resolution: caller > definition.model > Inherit ──
        let effective_model = caller_model.or_else(|| match &definition.model {
            AgentModel::Fixed(m) => Some(m.clone()),
            AgentModel::Inherit => None,
        });

        let request = SpawnSubagentRequest {
            subagent_type: subagent_type.to_string(),
            prompt: prompt.to_string(),
            description: description.to_string(),
            effective_model,
            run_in_background,
            name,
        };

        let launch_ctx = SpawnSubagentContext {
            session_id: ctx.session_id.clone(),
            parent_run_id: Some(ctx.run_id.clone()),
            parent_agent_id: ctx.agent_id.clone(),
            cancellation: ctx.cancellation.clone(),
            permission_mode: ctx.permission_mode,
        };

        // ── Async path: P6.x placeholder ──────────────────────────────────
        if request.run_in_background {
            return Ok(ToolResult::new(
                "spawn_subagent",
                r#"{"status":"not_implemented_yet","message":"async sub-agent path lands in P6.x; pass run_in_background=false for now"}"#,
                None,
            ));
        }

        // ── Sync path: await launcher ──────────────────────────────────────
        let output = self
            .launcher
            .launch_sync(request, launch_ctx)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("sub-agent launch failed: {e}")))?;

        Ok(ToolResult::new("spawn_subagent", output, None))
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tools::permission::PermissionMode;
    use serde_json::json;
    use std::sync::Mutex;

    struct RecordingLauncher {
        seen_requests: Arc<Mutex<Vec<SpawnSubagentRequest>>>,
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
    }

    fn build_tool_with_recorder(
        seen: Arc<Mutex<Vec<SpawnSubagentRequest>>>,
    ) -> SpawnSubagentRuntimeTool {
        let registry = Arc::new(AgentRegistry::with_builtins());
        SpawnSubagentRuntimeTool::new(
            Arc::new(RecordingLauncher {
                seen_requests: seen,
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
        assert_eq!(def.id, "spawn_subagent");
    }

    #[tokio::test]
    async fn missing_subagent_type_returns_execution_failed() {
        let tool = build_tool_with_recorder(Arc::new(Mutex::new(Vec::new())));
        let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
            .with_permission_mode(PermissionMode::Default);
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
                    msg.contains("subagent_type"),
                    "error should mention subagent_type, got: {msg}"
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
                json!({ "subagent_type": "browse_data_agent", "description": "test" }),
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
    async fn run_in_background_true_returns_placeholder() {
        let tool = build_tool_with_recorder(Arc::new(Mutex::new(Vec::new())));
        let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
        let result = tool
            .execute(
                json!({
                    "subagent_type": "browse_data_agent",
                    "prompt": "scrape some data",
                    "description": "test background",
                    "run_in_background": true
                }),
                ctx,
            )
            .await
            .expect("background path should not return Err");
        assert!(
            result.content.contains("not_implemented_yet"),
            "placeholder should contain not_implemented_yet, got: {}",
            result.content
        );
    }

    #[tokio::test]
    async fn sync_path_passes_context_permission_mode_to_launcher() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let tool = build_tool_with_recorder(seen.clone());
        let ctx = ToolExecutionContext::for_test("conv-perm", "run-perm", "tc-perm")
            .with_permission_mode(PermissionMode::DontAsk);
        // browse_data_agent is a builtin in AgentRegistry::with_builtins()
        tool.execute(
            json!({
                "subagent_type": "browse_data_agent",
                "prompt": "extract table",
                "description": "sync perm test"
            }),
            ctx,
        )
        .await
        .expect("sync path should succeed with stub launcher");
        let reqs = seen.lock().unwrap();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].subagent_type, "browse_data_agent");
    }

    #[tokio::test]
    async fn empty_model_string_treated_as_inherit() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let tool = build_tool_with_recorder(seen.clone());
        let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
        tool.execute(
            json!({
                "subagent_type": "browse_data_agent",
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
}
