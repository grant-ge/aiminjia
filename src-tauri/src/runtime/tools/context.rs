use std::sync::{Arc, Mutex};

use crate::runtime::agent::{
    AgentNameRegistry, CancellationRegistry, InboxRegistry, LeadIdleSupervisor, TeamRegistry,
};
use crate::runtime::cancellation::CancellationToken;
use crate::runtime::hooks::config::HookRegistry;
use crate::runtime::ids::{AgentId, RunId, SessionId, ToolCallId};
use crate::runtime::store::PermissionStore;
use crate::runtime::tools::capability::SharedCapabilityContext;
use crate::runtime::tools::permission::{PermissionDecision, PermissionMode};

#[derive(Default)]
pub struct EventCollectingSink {
    events: Mutex<Vec<String>>,
}

impl EventCollectingSink {
    pub fn emit(&self, event_name: &str) {
        self.events.lock().unwrap().push(event_name.to_string());
    }

    pub fn snapshot(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }
}

/// Per-invocation context threaded through every [`crate::runtime::tools::RuntimeTool`].
///
/// # Capability boundary (Phase 2)
///
/// The optional `capability` field carries a [`crate::runtime::tools::capability::CapabilityContext`]
/// that exposes only the narrow set of services a runtime tool may legitimately
/// access (workspace path, scoped identifiers).
///
/// New tools should read services from `ctx.capability` rather than being
/// handed a full [`crate::plugin::context::PluginContext`].  Legacy tools
/// continue to receive `PluginContext` via the
/// [`crate::runtime::tools::LegacyToolAdapter`] bridge — `ToolExecutionContext`
/// itself never carries the full plugin context.
#[derive(Clone)]
pub struct ToolExecutionContext {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub agent_id: Option<AgentId>,
    pub tool_call_id: ToolCallId,
    pub cancellation: CancellationToken,
    pub event_sink: Arc<EventCollectingSink>,
    /// Optional capability-scoped context.  `None` for legacy/test paths that
    /// do not need service access.  New `RuntimeTool` implementations that need
    /// workspace or storage info should declare their intent here rather than
    /// accepting a full `PluginContext`.
    pub capability: Option<SharedCapabilityContext>,
    /// Optional permission override injected by the orchestration layer when a
    /// pending permission ask has already been resolved and the original tool
    /// call should be replayed without re-entering the permission pipeline.
    pub permission_override: Option<PermissionDecision>,
    /// Permission mode transform applied after the pipeline returns a decision.
    pub permission_mode: PermissionMode,
    /// 可选的 PermissionStore，供工具 check_permissions 做细粒度规则查询。
    pub permission_store: Option<Arc<PermissionStore>>,
    /// Optional session-scoped hooks executed around tool dispatch.
    pub hook_registry: Option<Arc<HookRegistry>>,
    /// User-submitted interaction data injected when replaying an interactive tool.
    pub interaction_resolution: Option<serde_json::Value>,
    /// Original tool-call request, used by interactive tools to build replayable requests.
    pub current_tool_call_request:
        Option<crate::runtime::chat::tool_round_types::RuntimeToolCallRequest>,
    /// Task V2 persistence root (AiJiaHome), used by task runtime tools.
    pub task_store_root: Option<std::path::PathBuf>,
    /// Per-process Team registry injected by the orchestration layer.
    /// `None` for legacy / test paths that do not need team operations.
    pub team_registry: Option<Arc<TeamRegistry>>,
    /// Per-process agent-name registry injected by the orchestration layer.
    /// `None` for legacy / test paths that do not need name resolution.
    pub agent_names: Option<Arc<AgentNameRegistry>>,
    /// Per-process inbox registry — looks up an agent's mpsc inbox from its
    /// AgentId.  Required by SendMessage (P2.2); `None` elsewhere.
    pub inbox_registry: Option<Arc<InboxRegistry>>,
    /// LTR (P2.4): per-process Lead idle-state supervisor.  Used by SendMessage
    /// to enqueue/wake the Lead, and by the chat turn driver to self-check
    /// at turn end.  `None` for legacy paths.
    pub lead_idle: Option<Arc<LeadIdleSupervisor>>,
    /// LTR (P2.7): per-process cancellation registry — looks up an agent's
    /// CancellationToken by (SessionId, AgentId).  Required by TeammateStop.
    pub cancellation_registry: Option<Arc<CancellationRegistry>>,
    /// LTR (P2.8): true when this tool call runs inside an async runner
    /// (Teammate idle loop or async sub-agent) that has no UI thread to
    /// surface user-facing prompts to.  Permission decisions of `Ask` are
    /// auto-denied when this is true; default false (Lead / interactive runs).
    pub is_async: bool,
    /// LTR (B-gap2): per-conversation directory rooted at
    /// `<aijia_home>/users/{scope}/conversations/{conv_id}`.  Tools that
    /// spawn Teammates/sub-agents propagate this into worker contexts so
    /// transcript JSONL, `.meta.json` sidecars, and team_context
    /// attachments land on disk.  `None` in legacy/test paths.
    pub conv_dir: Option<std::path::PathBuf>,
}

impl ToolExecutionContext {
    pub fn new(
        session_id: SessionId,
        run_id: RunId,
        agent_id: Option<AgentId>,
        tool_call_id: impl Into<String>,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            session_id,
            run_id,
            agent_id,
            tool_call_id: ToolCallId::new(tool_call_id.into()),
            cancellation,
            event_sink: Arc::new(EventCollectingSink::default()),
            capability: None,
            permission_override: None,
            permission_mode: PermissionMode::Default,
            permission_store: None,
            hook_registry: None,
            interaction_resolution: None,
            current_tool_call_request: None,
            task_store_root: None,
            team_registry: None,
            agent_names: None,
            inbox_registry: None,
            lead_idle: None,
            cancellation_registry: None,
            is_async: false,
            conv_dir: None,
        }
    }

    /// Attach a [`crate::runtime::tools::capability::CapabilityContext`] to this context.
    ///
    /// Call this in the orchestration layer (e.g. `AgentRuntime`) before
    /// dispatching to tools that declare capability-scoped service needs.
    pub fn with_capability(mut self, cap: SharedCapabilityContext) -> Self {
        self.capability = Some(cap);
        self
    }

    pub fn with_permission_override(mut self, decision: PermissionDecision) -> Self {
        self.permission_override = Some(decision);
        self
    }

    pub fn with_permission_mode(mut self, mode: PermissionMode) -> Self {
        self.permission_mode = mode;
        self
    }

    pub fn with_permission_store(mut self, store: Arc<PermissionStore>) -> Self {
        self.permission_store = Some(store);
        self
    }

    pub fn with_hook_registry(mut self, registry: Arc<HookRegistry>) -> Self {
        self.hook_registry = Some(registry);
        self
    }

    pub fn with_interaction_resolution(mut self, value: serde_json::Value) -> Self {
        self.interaction_resolution = Some(value);
        self
    }

    pub fn with_current_tool_call_request(
        mut self,
        request: crate::runtime::chat::tool_round_types::RuntimeToolCallRequest,
    ) -> Self {
        self.current_tool_call_request = Some(request);
        self
    }

    pub fn with_team_registry(mut self, registry: Arc<TeamRegistry>) -> Self {
        self.team_registry = Some(registry);
        self
    }

    pub fn with_agent_names(mut self, registry: Arc<AgentNameRegistry>) -> Self {
        self.agent_names = Some(registry);
        self
    }

    pub fn with_inbox_registry(mut self, registry: Arc<InboxRegistry>) -> Self {
        self.inbox_registry = Some(registry);
        self
    }

    pub fn with_lead_idle(mut self, supervisor: Arc<LeadIdleSupervisor>) -> Self {
        self.lead_idle = Some(supervisor);
        self
    }

    pub fn with_cancellation_registry(mut self, registry: Arc<CancellationRegistry>) -> Self {
        self.cancellation_registry = Some(registry);
        self
    }

    /// LTR (P2.8): mark this context as belonging to an async runner.
    /// See `is_async` field for semantics.
    pub fn with_async(mut self, is_async: bool) -> Self {
        self.is_async = is_async;
        self
    }

    /// LTR (B-gap2): attach the per-conversation directory.  See `conv_dir`.
    pub fn with_conv_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.conv_dir = Some(dir);
        self
    }

    /// Returns the process-wide [`InboxRegistry`].
    ///
    /// # Panics
    /// Panics if the orchestration layer did not inject a registry via
    /// [`Self::with_inbox_registry`].  SendMessage requires this.
    pub fn inbox_registry(&self) -> &Arc<InboxRegistry> {
        self.inbox_registry
            .as_ref()
            .expect("inbox_registry not injected into ToolExecutionContext — use with_inbox_registry()")
    }

    /// Returns the process-wide [`TeamRegistry`].
    ///
    /// # Panics
    /// Panics if the orchestration layer did not inject a registry via
    /// [`Self::with_team_registry`].  Tools that call this must only be
    /// dispatched through the full production path (not legacy / test stubs).
    pub fn team_registry(&self) -> &Arc<TeamRegistry> {
        self.team_registry
            .as_ref()
            .expect("team_registry not injected into ToolExecutionContext — use with_team_registry()")
    }

    /// Returns the process-wide [`AgentNameRegistry`].
    ///
    /// # Panics
    /// Panics if the orchestration layer did not inject a registry via
    /// [`Self::with_agent_names`].  Tools that call this must only be
    /// dispatched through the full production path (not legacy / test stubs).
    pub fn agent_names(&self) -> &Arc<AgentNameRegistry> {
        self.agent_names
            .as_ref()
            .expect("agent_names not injected into ToolExecutionContext — use with_agent_names()")
    }

    /// Convenience constructor for integration-test code.
    ///
    /// Creates an isolated root `CancellationToken` that is not connected to any
    /// session hierarchy.  This is intentional for test helpers — production code
    /// must call `ToolExecutionContext::new(…, parent.child_token())` instead.
    pub fn for_test(
        conversation_id: impl Into<String>,
        run_id: impl Into<String>,
        tool_call_id: impl Into<String>,
    ) -> Self {
        Self::new(
            SessionId::new(conversation_id.into()),
            RunId::new(run_id.into()),
            None,
            tool_call_id,
            // Test-only root token — cancel cascade not required in test helpers.
            CancellationToken::new(),
        )
    }
}
