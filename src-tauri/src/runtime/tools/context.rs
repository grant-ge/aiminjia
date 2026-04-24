use std::sync::{Arc, Mutex};

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
