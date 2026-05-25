//! P10.1 e2e: parent → spawn_subagent tool → launcher, wired through dispatcher.
//!
//! Verifies the full sync sub-agent path: parent emits a `spawn_subagent`
//! tool_use → `ToolDispatcher::dispatch` routes it → `SpawnSubagentRuntimeTool`
//! resolves the agent type via `AgentRegistry::with_builtins()` → calls the
//! injected `SpawnSubagentLauncher` → launcher output arrives back as
//! `ToolResult`.  No real LLM or worker_runtime is involved.
//!
//! Plan reference: docs/superpowers/plans/2026-04-30-lotus-subagent-mode-b-master-plan.md
//! §13 Task P10.1.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use app_lib::runtime::agent::registry::AgentRegistry;
use app_lib::runtime::ids::{AgentId, RunId, SessionId};
use app_lib::runtime::tools::builtin::spawn_subagent::{
    SpawnAsyncOutcome, SpawnSubagentContext, SpawnSubagentLauncher, SpawnSubagentRequest,
    SpawnSubagentRuntimeTool,
};
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::executor::ToolResult;
use app_lib::runtime::tools::permission::PermissionMode;
use app_lib::runtime::tools::{AllowAllPermissionPipeline, ToolDispatcher};

// ─── Recording launcher ───────────────────────────────────────────────────────

/// A stub launcher that records the request and context it received, then
/// returns a deterministic output string.
struct RecordingLauncher {
    /// Fixed output returned from every `launch_sync` call.
    output: String,
    /// Recorded request from the most recent call.
    last_request: Arc<Mutex<Option<SpawnSubagentRequest>>>,
    /// Recorded context from the most recent call.
    last_context: Arc<Mutex<Option<CapturedContext>>>,
}

/// Serialisable subset of `SpawnSubagentContext` captured for assertions.
#[derive(Debug, Clone)]
struct CapturedContext {
    session_id: SessionId,
    parent_run_id: Option<RunId>,
    parent_agent_id: Option<AgentId>,
    permission_mode: PermissionMode,
}

impl RecordingLauncher {
    fn new(
        output: impl Into<String>,
    ) -> (
        Arc<Self>,
        Arc<Mutex<Option<SpawnSubagentRequest>>>,
        Arc<Mutex<Option<CapturedContext>>>,
    ) {
        let last_request = Arc::new(Mutex::new(None));
        let last_context = Arc::new(Mutex::new(None));
        let launcher = Arc::new(Self {
            output: output.into(),
            last_request: last_request.clone(),
            last_context: last_context.clone(),
        });
        (launcher, last_request, last_context)
    }
}

#[async_trait]
impl SpawnSubagentLauncher for RecordingLauncher {
    async fn launch_sync(
        &self,
        request: SpawnSubagentRequest,
        context: SpawnSubagentContext,
    ) -> Result<String> {
        *self.last_request.lock().unwrap() = Some(request);
        *self.last_context.lock().unwrap() = Some(CapturedContext {
            session_id: context.session_id,
            parent_run_id: context.parent_run_id,
            parent_agent_id: context.parent_agent_id,
            permission_mode: context.permission_mode,
        });
        Ok(self.output.clone())
    }

    async fn launch_async(
        &self,
        _request: SpawnSubagentRequest,
        _context: SpawnSubagentContext,
    ) -> Result<SpawnAsyncOutcome> {
        Ok(SpawnAsyncOutcome {
            agent_id: AgentId::new("stub-async-id"),
            name: None,
        })
    }
}

// ─── Helper ───────────────────────────────────────────────────────────────────

fn build_dispatcher_with_launcher(launcher: Arc<dyn SpawnSubagentLauncher>) -> ToolDispatcher {
    let registry = Arc::new(AgentRegistry::with_builtins());
    let tool = Arc::new(SpawnSubagentRuntimeTool::new(launcher, registry));
    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(tool);
    dispatcher
}

// ─── Test 1: explore end-to-end through dispatcher ───────────────────────────

/// Full path: dispatcher → SpawnSubagentRuntimeTool → RecordingLauncher.
///
/// Proves:
/// - `ToolResult.content` equals the launcher's deterministic output verbatim
/// - Request fields (`subagent_type`, `prompt`, `description`, `name`,
///   `run_in_background`, `effective_model`) are propagated correctly
/// - `explore` agent has `AgentModel::Inherit` so `effective_model == None`
///   when no caller-supplied `model` field is present
/// - Context fields (`session_id`, `parent_run_id`, `parent_agent_id`,
///   `permission_mode`) are forwarded from `ToolExecutionContext`
#[tokio::test]
async fn explore_agent_e2e_through_dispatcher() {
    use app_lib::runtime::tools::dispatcher::ToolDispatchOutcome;

    let expected_output = "FOUND: Cargo.toml at /workspace/Cargo.toml";
    let (launcher, req_slot, ctx_slot) = RecordingLauncher::new(expected_output);
    let dispatcher = build_dispatcher_with_launcher(launcher);

    let ctx = ToolExecutionContext::for_test("sess-e2e-1", "run-e2e-1", "tc-e2e-1")
        .with_permission_mode(PermissionMode::DontAsk);

    let input = json!({
        "subagent_type": "explore",
        "prompt":        "find Cargo.toml in workspace",
        "description":   "locate Cargo manifest",
    });

    let outcome = dispatcher
        .dispatch("Agent", input, ctx)
        .await
        .expect("dispatch must succeed");

    // ── Outcome must be Completed, not AskRequired or InteractionRequired ──
    let result: ToolResult = match outcome {
        ToolDispatchOutcome::Completed { result, .. } => result,
        other => panic!(
            "expected Completed, got: {:?}",
            std::mem::discriminant(&other)
        ),
    };

    // ── ToolResult.content equals launcher output verbatim ─────────────────
    assert_eq!(
        result.content, expected_output,
        "ToolResult.content must be the launcher output verbatim"
    );

    // ── Request assertions ─────────────────────────────────────────────────
    let req = req_slot
        .lock()
        .unwrap()
        .clone()
        .expect("launcher must have been called");
    assert_eq!(req.subagent_type, "explore");
    assert_eq!(req.prompt, "find Cargo.toml in workspace");
    assert_eq!(req.description, "locate Cargo manifest");
    assert!(
        req.effective_model.is_none(),
        "explore.model=Inherit with no caller model → effective_model must be None, got: {:?}",
        req.effective_model
    );
    assert!(
        !req.run_in_background,
        "run_in_background defaults to false"
    );
    assert!(req.name.is_none(), "no name field in input → must be None");

    // ── Context assertions ────────────────────────────────────────────────
    let cap = ctx_slot
        .lock()
        .unwrap()
        .clone()
        .expect("context must have been captured");
    assert_eq!(cap.session_id.as_str(), "sess-e2e-1");
    assert_eq!(
        cap.parent_run_id.as_ref().map(|r| r.as_str()),
        Some("run-e2e-1"),
        "parent_run_id must be populated from ToolExecutionContext.run_id"
    );
    // for_test() creates ctx with agent_id=None
    assert!(
        cap.parent_agent_id.is_none(),
        "parent_agent_id must be None when ToolExecutionContext.agent_id is None"
    );
    assert_eq!(
        cap.permission_mode,
        PermissionMode::DontAsk,
        "permission_mode must propagate from ToolExecutionContext"
    );
}

// ─── Test 2: caller-supplied model overrides Inherit definition ───────────────

/// Locks in three-tier model resolution from spawn_subagent.rs:148-152.
///
/// explore.model = Inherit; caller supplies "some-cloud-model-id"
/// → effective_model must be Some("some-cloud-model-id")
#[tokio::test]
async fn caller_model_overrides_inherit_definition() {
    let (launcher, req_slot, _) = RecordingLauncher::new("done");
    let dispatcher = build_dispatcher_with_launcher(launcher);

    let ctx = ToolExecutionContext::for_test("sess-model", "run-model", "tc-model");

    let input = json!({
        "subagent_type": "explore",
        "prompt":        "search for tests",
        "description":   "model override test",
        "model":         "some-cloud-model-id",
    });

    dispatcher
        .dispatch("Agent", input, ctx)
        .await
        .expect("dispatch must succeed");

    let req = req_slot
        .lock()
        .unwrap()
        .clone()
        .expect("launcher must have been called");
    assert_eq!(
        req.effective_model,
        Some("some-cloud-model-id".to_string()),
        "caller-supplied model must win over definition.model=Inherit"
    );
}

// ─── Test 3: general-purpose agent resolves through the registry ──────────────

/// Quick smoke: both new P9.1 builtins (`general-purpose`, `explore`) are
/// reachable via `AgentRegistry::with_builtins()` through the live dispatcher
/// path.  Verifies that no registration step was missed.
#[tokio::test]
async fn general_purpose_agent_resolves_through_registry() {
    use app_lib::runtime::tools::dispatcher::ToolDispatchOutcome;

    let (launcher, req_slot, _) = RecordingLauncher::new("general-purpose completed");
    let dispatcher = build_dispatcher_with_launcher(launcher);

    let ctx = ToolExecutionContext::for_test("sess-gp", "run-gp", "tc-gp");

    let input = json!({
        "subagent_type": "general-purpose",
        "prompt":        "summarise the project",
        "description":   "smoke test for general-purpose builtin",
    });

    let outcome = dispatcher
        .dispatch("Agent", input, ctx)
        .await
        .expect("dispatch must succeed for general-purpose");

    let result = match outcome {
        ToolDispatchOutcome::Completed { result, .. } => result,
        other => panic!(
            "expected Completed, got discriminant {:?}",
            std::mem::discriminant(&other)
        ),
    };

    assert_eq!(result.content, "general-purpose completed");

    let req = req_slot
        .lock()
        .unwrap()
        .clone()
        .expect("launcher must have been called");
    assert_eq!(req.subagent_type, "general-purpose");
    // general-purpose.model = Inherit, no caller model → effective_model=None
    assert!(
        req.effective_model.is_none(),
        "general-purpose.model=Inherit → effective_model must be None, got: {:?}",
        req.effective_model
    );
}
