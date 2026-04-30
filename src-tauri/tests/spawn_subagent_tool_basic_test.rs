//! Basic structural tests for `SpawnSubagentRuntimeTool`.
//!
//! These tests do NOT exercise actual sub-agent execution (that requires a
//! real `LlmGateway`). They verify:
//!
//! - tool definition resolves with correct id
//! - `is_concurrency_safe` returns `true`
//! - missing required fields produce `ToolError::ExecutionFailed`
//! - unknown `subagent_type` produces a helpful error message
//! - `run_in_background=true` returns the `async_launched` JSON
//! - sync path invokes the launcher and returns its output
//! - permission mode is forwarded to the launch context
//! - empty `model` string is treated as inherit (not forwarded as empty)

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use app_lib::runtime::agent::registry::AgentRegistry;
use app_lib::runtime::ids::AgentId;
use app_lib::runtime::tools::builtin::spawn_subagent::{
    SpawnAsyncOutcome, SpawnSubagentContext, SpawnSubagentLauncher, SpawnSubagentRequest,
    SpawnSubagentRuntimeTool,
};
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::permission::PermissionMode;
use app_lib::runtime::tools::RuntimeTool;

// ─── Stub launcher ────────────────────────────────────────────────────────────

struct StubLauncher {
    output: String,
}

impl StubLauncher {
    fn returning(s: impl Into<String>) -> Self {
        Self { output: s.into() }
    }
}

#[async_trait]
impl SpawnSubagentLauncher for StubLauncher {
    async fn launch_sync(
        &self,
        request: SpawnSubagentRequest,
        _context: SpawnSubagentContext,
    ) -> Result<String> {
        Ok(format!(
            "stub: type={} model={:?} -> {}",
            request.subagent_type, request.effective_model, self.output
        ))
    }

    async fn launch_async(
        &self,
        request: SpawnSubagentRequest,
        _context: SpawnSubagentContext,
    ) -> Result<SpawnAsyncOutcome> {
        Ok(SpawnAsyncOutcome {
            agent_id: AgentId::new("stub-async-id"),
            name: request.name.clone(),
        })
    }
}

// ─── Helper to build the tool ─────────────────────────────────────────────────

fn build_tool() -> SpawnSubagentRuntimeTool {
    let registry = Arc::new(AgentRegistry::with_builtins());
    SpawnSubagentRuntimeTool::new(Arc::new(StubLauncher::returning("done")), registry)
}

// ─── Structural tests ─────────────────────────────────────────────────────────

#[test]
fn is_concurrency_safe_returns_true() {
    let tool = build_tool();
    assert!(
        tool.is_concurrency_safe(&serde_json::Value::Null),
        "spawn_subagent must be concurrency-safe"
    );
}

#[test]
fn definition_id_is_spawn_subagent() {
    let tool = build_tool();
    let def = tool.definition();
    assert_eq!(def.id, "spawn_subagent");
}

// ─── Required-field validation ────────────────────────────────────────────────

#[tokio::test]
async fn missing_subagent_type_returns_execution_failed() {
    let tool = build_tool();
    let ctx = ToolExecutionContext::for_test("c", "r", "tc");
    let err = tool
        .execute(json!({ "prompt": "go", "description": "test" }), ctx)
        .await
        .unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("subagent_type"),
        "error should name the missing field, got: {msg}"
    );
}

#[tokio::test]
async fn missing_prompt_returns_execution_failed() {
    let tool = build_tool();
    let ctx = ToolExecutionContext::for_test("c", "r", "tc");
    let err = tool
        .execute(
            json!({ "subagent_type": "browse_data_agent", "description": "test" }),
            ctx,
        )
        .await
        .unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("prompt"),
        "error should name 'prompt', got: {msg}"
    );
}

#[tokio::test]
async fn missing_description_returns_execution_failed() {
    let tool = build_tool();
    let ctx = ToolExecutionContext::for_test("c", "r", "tc");
    let err = tool
        .execute(
            json!({ "subagent_type": "browse_data_agent", "prompt": "do it" }),
            ctx,
        )
        .await
        .unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("description"),
        "error should name 'description', got: {msg}"
    );
}

// ─── Unknown subagent_type ────────────────────────────────────────────────────

#[tokio::test]
async fn unknown_subagent_type_returns_helpful_error() {
    let tool = build_tool();
    let ctx = ToolExecutionContext::for_test("c", "r", "tc");
    let err = tool
        .execute(
            json!({
                "subagent_type": "does_not_exist_xyz",
                "prompt": "run",
                "description": "test"
            }),
            ctx,
        )
        .await
        .unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("does_not_exist_xyz"),
        "error should echo the unknown type, got: {msg}"
    );
    // Should hint at where to put agent definitions
    assert!(
        msg.contains("~/.renlijia") || msg.contains("agents/") || msg.contains("builtin"),
        "error should hint at agent location, got: {msg}"
    );
}

// ─── Async path ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn run_in_background_true_returns_not_implemented_placeholder() {
    let tool = build_tool();
    let ctx = ToolExecutionContext::for_test("c", "r", "tc");
    let result = tool
        .execute(
            json!({
                "subagent_type": "browse_data_agent",
                "prompt": "extract data",
                "description": "async test",
                "run_in_background": true
            }),
            ctx,
        )
        .await
        .expect("async path must not return Err");
    // P6.2: async path now returns async_launched JSON, not the old placeholder.
    let parsed: serde_json::Value =
        serde_json::from_str(&result.content).expect("response must be valid JSON");
    assert_eq!(
        parsed.get("status").and_then(|v| v.as_str()),
        Some("async_launched"),
        "status should be async_launched, got: {}",
        result.content
    );
}

// ─── Sync path ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn sync_path_returns_launcher_output() {
    let registry = Arc::new(AgentRegistry::with_builtins());
    let tool = SpawnSubagentRuntimeTool::new(
        Arc::new(StubLauncher::returning("analysis complete")),
        registry,
    );
    let ctx = ToolExecutionContext::for_test("c", "r", "tc");
    let result = tool
        .execute(
            json!({
                "subagent_type": "browse_data_agent",
                "prompt": "extract rows",
                "description": "sync test"
            }),
            ctx,
        )
        .await
        .expect("sync path should succeed");
    assert!(
        result.content.contains("analysis complete"),
        "result should contain launcher output, got: {}",
        result.content
    );
}

// ─── Permission mode forwarding ───────────────────────────────────────────────

#[tokio::test]
async fn permission_mode_is_forwarded_to_launch_context() {
    use std::sync::Mutex;

    struct CapturingLauncher {
        captured_mode: Arc<Mutex<Option<PermissionMode>>>,
    }

    #[async_trait]
    impl SpawnSubagentLauncher for CapturingLauncher {
        async fn launch_sync(
            &self,
            _request: SpawnSubagentRequest,
            context: SpawnSubagentContext,
        ) -> Result<String> {
            *self.captured_mode.lock().unwrap() = Some(context.permission_mode);
            Ok("done".into())
        }

        async fn launch_async(
            &self,
            _request: SpawnSubagentRequest,
            _context: SpawnSubagentContext,
        ) -> Result<SpawnAsyncOutcome> {
            Ok(SpawnAsyncOutcome {
                agent_id: AgentId::new("stub-id"),
                name: None,
            })
        }
    }

    let captured = Arc::new(Mutex::new(None));
    let registry = Arc::new(AgentRegistry::with_builtins());
    let tool = SpawnSubagentRuntimeTool::new(
        Arc::new(CapturingLauncher {
            captured_mode: captured.clone(),
        }),
        registry,
    );
    let ctx = ToolExecutionContext::for_test("c-perm", "r-perm", "tc-perm")
        .with_permission_mode(PermissionMode::DontAsk);

    tool.execute(
        json!({
            "subagent_type": "browse_data_agent",
            "prompt": "run",
            "description": "perm test"
        }),
        ctx,
    )
    .await
    .expect("should succeed");

    let mode = captured.lock().unwrap();
    assert_eq!(
        *mode,
        Some(PermissionMode::DontAsk),
        "launcher should receive DontAsk permission mode"
    );
}

// ─── Model override ───────────────────────────────────────────────────────────

#[tokio::test]
async fn caller_model_overrides_definition() {
    use std::sync::Mutex;

    struct ModelCapture {
        model: Arc<Mutex<Option<Option<String>>>>,
    }

    #[async_trait]
    impl SpawnSubagentLauncher for ModelCapture {
        async fn launch_sync(
            &self,
            request: SpawnSubagentRequest,
            _context: SpawnSubagentContext,
        ) -> Result<String> {
            *self.model.lock().unwrap() = Some(request.effective_model.clone());
            Ok("done".into())
        }

        async fn launch_async(
            &self,
            _request: SpawnSubagentRequest,
            _context: SpawnSubagentContext,
        ) -> Result<SpawnAsyncOutcome> {
            Ok(SpawnAsyncOutcome {
                agent_id: AgentId::new("stub-id"),
                name: None,
            })
        }
    }

    let model_seen = Arc::new(Mutex::new(None));
    let registry = Arc::new(AgentRegistry::with_builtins());
    let tool = SpawnSubagentRuntimeTool::new(
        Arc::new(ModelCapture {
            model: model_seen.clone(),
        }),
        registry,
    );
    let ctx = ToolExecutionContext::for_test("c", "r", "tc");

    tool.execute(
        json!({
            "subagent_type": "browse_data_agent",
            "prompt": "run",
            "description": "model test",
            "model": "haiku"
        }),
        ctx,
    )
    .await
    .expect("should succeed");

    let seen = model_seen.lock().unwrap();
    assert_eq!(
        *seen,
        Some(Some("haiku".to_string())),
        "caller model override should be forwarded as effective_model"
    );
}

#[tokio::test]
async fn empty_model_string_treated_as_inherit() {
    use std::sync::Mutex;

    struct ModelCapture {
        model: Arc<Mutex<Option<Option<String>>>>,
    }

    #[async_trait]
    impl SpawnSubagentLauncher for ModelCapture {
        async fn launch_sync(
            &self,
            request: SpawnSubagentRequest,
            _context: SpawnSubagentContext,
        ) -> Result<String> {
            *self.model.lock().unwrap() = Some(request.effective_model.clone());
            Ok("done".into())
        }

        async fn launch_async(
            &self,
            _request: SpawnSubagentRequest,
            _context: SpawnSubagentContext,
        ) -> Result<SpawnAsyncOutcome> {
            Ok(SpawnAsyncOutcome {
                agent_id: AgentId::new("stub-id"),
                name: None,
            })
        }
    }

    let model_seen = Arc::new(Mutex::new(None));
    let registry = Arc::new(AgentRegistry::with_builtins());
    let tool = SpawnSubagentRuntimeTool::new(
        Arc::new(ModelCapture {
            model: model_seen.clone(),
        }),
        registry,
    );
    let ctx = ToolExecutionContext::for_test("c", "r", "tc");

    tool.execute(
        json!({
            "subagent_type": "browse_data_agent",
            "prompt": "run",
            "description": "empty model test",
            "model": ""
        }),
        ctx,
    )
    .await
    .expect("should succeed");

    let seen = model_seen.lock().unwrap();
    // browse_data_agent definition uses AgentModel::Inherit → None
    // empty caller model → treated as inherit → definition.model=Inherit → None
    assert_eq!(
        seen.as_ref().and_then(|m| m.as_deref()),
        None,
        "empty caller model must not override (effective_model should be None for Inherit definition)"
    );
}
