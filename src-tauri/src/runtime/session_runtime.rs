use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;

// Import and re-export from chat module.  Types were previously defined here;
// they now live in `runtime::chat` to avoid circular imports.
use crate::runtime::cancellation::{CancellationReason, CancellationToken};
pub use crate::runtime::chat::ChatTurnRequest;
use crate::runtime::chat::{RuntimeChatTurnDriver, RuntimeLlmExecutor};
use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};
use crate::runtime::identity::IdentityMapping;
use crate::runtime::ids::{RunId, SessionId, ToolCallId};
use crate::runtime::query_engine::QueryEngine;
use crate::runtime::state::TurnState;
use crate::runtime::store::{
    AuthorizedWorkspaceRef, AuthorizedWorkspaceStore, PendingPermissionRequest,
    PendingPermissionRequestStore, PendingPermissionResolution, PermissionStore, PolicyDecision,
};
use crate::runtime::tools::permission::{persist_permission_decision, PermissionDestination};
use crate::transport::runtime_host::RuntimeHost;
use crate::transport::tauri_event_adapter::TauriEventAdapter;

#[derive(Clone)]
pub struct SessionRuntime {
    query_engine: QueryEngine,
    session_query_engines: Arc<Mutex<HashMap<String, QueryEngine>>>,
    session_cancel_roots: Arc<Mutex<HashMap<String, CancellationToken>>>,
    event_bus: RuntimeEventBus,
    /// S4 executor: owns the query loop; executor is a provider streaming adapter only.
    /// When present, `build_driver_for_turn` uses `RuntimeChatTurnDriver::with_llm_executor`.
    llm_executor: Option<Arc<dyn RuntimeLlmExecutor>>,
    authorized_workspace_store: Option<Arc<dyn AuthorizedWorkspaceStore>>,
    pending_permission_store: Arc<PendingPermissionRequestStore>,
    permission_store: Option<Arc<PermissionStore>>,
    agent_registry: Option<Arc<crate::runtime::agent::registry::AgentRegistry>>,
}

impl SessionRuntime {
    pub fn new(query_engine: QueryEngine, event_bus: RuntimeEventBus) -> Self {
        Self {
            query_engine,
            session_query_engines: Arc::new(Mutex::new(HashMap::new())),
            session_cancel_roots: Arc::new(Mutex::new(HashMap::new())),
            event_bus,
            llm_executor: None,
            authorized_workspace_store: None,
            pending_permission_store: Arc::new(PendingPermissionRequestStore::new()),
            permission_store: None,
            agent_registry: None,
        }
    }

    /// Construct a `SessionRuntime` wired to the S4 `RuntimeLlmExecutor`.
    ///
    /// The driver loop (`RuntimeChatTurnDriver::run_chat_turn_s4`) owns the
    /// query/tool loop; the executor is a pure provider streaming adapter.
    /// Use this constructor to switch production traffic to the S4 path.
    pub fn with_llm_executor(
        query_engine: QueryEngine,
        event_bus: RuntimeEventBus,
        executor: Arc<dyn RuntimeLlmExecutor>,
    ) -> Self {
        Self {
            query_engine,
            session_query_engines: Arc::new(Mutex::new(HashMap::new())),
            session_cancel_roots: Arc::new(Mutex::new(HashMap::new())),
            event_bus,
            llm_executor: Some(executor),
            authorized_workspace_store: None,
            pending_permission_store: Arc::new(PendingPermissionRequestStore::new()),
            permission_store: None,
            agent_registry: None,
        }
    }

    pub fn with_pending_permission_store(
        mut self,
        pending_permission_store: Arc<PendingPermissionRequestStore>,
    ) -> Self {
        self.pending_permission_store = pending_permission_store;
        self
    }

    pub fn with_authorized_workspace_store(
        mut self,
        authorized_workspace_store: Arc<dyn AuthorizedWorkspaceStore>,
    ) -> Self {
        self.authorized_workspace_store = Some(authorized_workspace_store);
        self
    }

    pub fn with_permission_store(mut self, permission_store: Arc<PermissionStore>) -> Self {
        self.permission_store = Some(permission_store);
        self
    }

    pub fn with_agent_registry(
        mut self,
        agent_registry: Arc<crate::runtime::agent::registry::AgentRegistry>,
    ) -> Self {
        self.agent_registry = Some(agent_registry);
        self
    }

    pub fn for_test(host: Arc<dyn RuntimeHost>) -> Self {
        let adapter = Arc::new(TauriEventAdapter::new(host));
        let bus = RuntimeEventBus::new();
        bus.subscribe(adapter);
        Self::new(QueryEngine::new(), bus)
    }

    pub async fn run_turn(&self, turn: &mut TurnState) -> Result<()> {
        let query_engine = self.query_engine_for_session(turn.session_id());
        query_engine.run(turn, &self.event_bus).await
    }

    pub async fn run_chat_request(
        &self,
        mut request: ChatTurnRequest,
    ) -> std::result::Result<(), String> {
        request = self.apply_agent_tool_constraint(request);
        let mapping = IdentityMapping::from_legacy_conversation_id(request.conversation_id.clone());
        // Generate the single authoritative RunId for this turn here and propagate
        // it into the request so legacy_send_message_impl uses the same identity.
        let run_id = RunId::new(uuid::Uuid::new_v4().to_string());
        request.run_id = run_id.clone();

        // Emit RunStarted before handing off to the driver.
        let run_started = RuntimeEvent::new(
            mapping.session_id.clone(),
            run_id.clone(),
            RuntimeEventKind::RunStarted,
        );
        let _ = self.event_bus.emit(run_started).await;

        let mut turn = TurnState::new(mapping, run_id, request.content.clone());
        let session_root = self.ensure_active_session_cancel_root(turn.session_id());
        turn = turn.with_cancellation(session_root.child_token());

        // Build a driver for this session and drive the full turn lifecycle.
        // The driver remains the only chat-turn entry and may invoke the legacy
        // executor helper internally on production paths.
        let driver = self.build_driver_for_turn(&turn);
        driver
            .run_chat_turn(&mut turn, &request)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn run_for_test(
        &self,
        conversation_id: &str,
        run_id: &str,
        user_input: &str,
    ) -> Result<()> {
        let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id.to_string());
        let mut turn = TurnState::new(
            mapping,
            RunId::new(run_id.to_string()),
            user_input.to_string(),
        );
        self.run_turn(&mut turn).await
    }

    pub fn recorded_events(&self) -> Vec<crate::runtime::events::RuntimeEvent> {
        self.event_bus.recorded()
    }

    pub fn resolve_permission_request(
        &self,
        tool_call_id: &ToolCallId,
        resolution: PendingPermissionResolution,
    ) -> Result<()> {
        let pending_request = self.pending_permission_store.get(tool_call_id);
        self.pending_permission_store
            .resolve(tool_call_id, resolution.clone())?;
        if let Some(request) = pending_request.as_ref() {
            self.persist_resolved_permission(request, &resolution);
        }
        Ok(())
    }

    pub fn cancel_pending_permission_requests_for_session(
        &self,
        session_id: &SessionId,
        message: &str,
    ) -> usize {
        self.pending_permission_store
            .cancel_for_session(session_id, message)
    }

    pub fn cancel_session(&self, session_id: &SessionId, reason: CancellationReason) {
        if let Some(root) = self.current_session_cancel_root(session_id) {
            root.cancel_with_reason(reason);
        }
        self.cancel_pending_permission_requests_for_session(
            session_id,
            "Permission request cancelled because the session was stopped.",
        );
    }

    pub fn clear_session_state(&self, session_id: &SessionId) {
        self.cancel_pending_permission_requests_for_session(
            session_id,
            "Permission request cancelled because the session state was cleared.",
        );
        self.session_cancel_roots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(session_id.as_str());
        let session_key = session_id.as_str().to_string();
        self.session_query_engines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&session_key);
    }

    fn query_engine_for_session(&self, session_id: &SessionId) -> QueryEngine {
        let authorized_workspace = self
            .authorized_workspace_store
            .as_ref()
            .and_then(|store| store.get_current_for_session(session_id).ok().flatten())
            .map(|aw| AuthorizedWorkspaceRef {
                id: aw.id,
                root_path: aw.root_path,
                display_name: aw.display_name,
            });
        let mut engines = self
            .session_query_engines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let session_engine = engines
            .entry(session_id.as_str().to_string())
            .or_insert_with(|| self.query_engine.clone_with_fresh_session_state())
            .clone();
        session_engine.with_authorized_workspace(authorized_workspace)
    }

    /// Build a `RuntimeChatTurnDriver` scoped to the given turn's session.
    fn build_driver_for_turn(&self, turn: &TurnState) -> RuntimeChatTurnDriver {
        let query_engine = self.query_engine_for_session(turn.session_id());
        if let Some(ref executor) = self.llm_executor {
            return RuntimeChatTurnDriver::with_llm_executor_and_permission_control_plane(
                query_engine,
                self.event_bus.clone(),
                executor.clone(),
                self.pending_permission_store.clone(),
            );
        }
        RuntimeChatTurnDriver::new(query_engine, self.event_bus.clone())
    }

    fn apply_agent_tool_constraint(&self, mut request: ChatTurnRequest) -> ChatTurnRequest {
        let Some(agent_name) = request.agent_name.as_deref() else {
            return request;
        };
        let Some(agent_registry) = self.agent_registry.as_ref() else {
            return request;
        };
        if let Some(definition) = agent_registry.get(agent_name) {
            request.allowed_tools = Some(definition.allowed_tools.clone());
        }
        request
    }

    fn persist_resolved_permission(
        &self,
        pending_request: &PendingPermissionRequest,
        resolution: &PendingPermissionResolution,
    ) {
        let Some(permission_store) = self.permission_store.as_ref() else {
            return;
        };

        let (remember, destination, decision) = match resolution {
            PendingPermissionResolution::Allow {
                remember,
                destination,
                ..
            } => (*remember, *destination, PolicyDecision::Allow),
            PendingPermissionResolution::Deny {
                remember,
                destination,
                ..
            } => (*remember, *destination, PolicyDecision::Deny),
            PendingPermissionResolution::Cancel { .. } => return,
        };

        if !remember {
            return;
        }

        if pending_request.capability_scopes.is_empty() {
            log::warn!(
                "[SessionRuntime] Skip persisting permission for '{}' because no capability scopes were captured",
                pending_request.tool_name
            );
            return;
        }

        let destination = destination
            .or(pending_request.default_destination)
            .unwrap_or(PermissionDestination::Session);

        persist_permission_decision(
            permission_store,
            &pending_request.tool_name,
            &pending_request.capability_scopes,
            decision,
            destination,
        );
    }

    fn ensure_active_session_cancel_root(&self, session_id: &SessionId) -> CancellationToken {
        let mut roots = self
            .session_cancel_roots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = roots
            .entry(session_id.as_str().to_string())
            .or_insert_with(CancellationToken::new);
        if root.is_cancelled() {
            *root = CancellationToken::new();
        }
        root.clone()
    }

    fn current_session_cancel_root(&self, session_id: &SessionId) -> Option<CancellationToken> {
        self.session_cancel_roots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(session_id.as_str())
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::agent::registry::AgentRegistry;
    use crate::runtime::tools::permission::{PermissionDestination, PermissionMode};
    use crate::runtime::tools::{
        AllowAllPermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatcher, ToolError,
        ToolExecutionContext, ToolResult,
    };
    use async_trait::async_trait;
    use serde_json::Value;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    struct CaptureAuthorizedWorkspaceTool {
        seen_root: Arc<Mutex<Option<PathBuf>>>,
    }

    #[async_trait]
    impl RuntimeTool for CaptureAuthorizedWorkspaceTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new(
                "capture_authorized_workspace",
                "Capture authorized workspace",
            )
        }

        async fn execute(
            &self,
            _input: Value,
            ctx: ToolExecutionContext,
        ) -> std::result::Result<ToolResult, ToolError> {
            let seen = ctx
                .capability
                .as_ref()
                .and_then(|cap| cap.storage.as_ref())
                .and_then(|storage| storage.authorized_workspace.as_ref())
                .map(|aw| aw.root_path.clone());
            *self.seen_root.lock().unwrap() = seen;
            Ok(ToolResult::new("capture_authorized_workspace", "ok", None))
        }
    }

    #[tokio::test]
    async fn run_turn_resolves_authorized_workspace_from_store() {
        let internal_workspace = TempDir::new().unwrap();
        let external_workspace = TempDir::new().unwrap();
        let seen_root = Arc::new(Mutex::new(None));
        let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
        dispatcher.register(Arc::new(CaptureAuthorizedWorkspaceTool {
            seen_root: seen_root.clone(),
        }));

        let store = Arc::new(crate::runtime::store::InMemoryAuthorizedWorkspaceStore::default());
        let session_id = crate::runtime::ids::SessionId::new("conv-authorized");
        store
            .replace_for_session(&crate::runtime::store::AuthorizedWorkspace {
                id: "aw-session".to_string(),
                session_id: session_id.clone(),
                root_path: external_workspace.path().to_path_buf(),
                display_name: "external".to_string(),
                authorized_at: chrono::Utc::now().to_rfc3339(),
            })
            .unwrap();

        let runtime = SessionRuntime::new(
            QueryEngine::with_dispatcher(dispatcher)
                .with_workspace_path(internal_workspace.path().to_path_buf()),
            RuntimeEventBus::new(),
        )
        .with_authorized_workspace_store(store);

        let mapping = IdentityMapping::from_legacy_conversation_id(session_id.as_str().to_string());
        let turn = TurnState::new(
            mapping,
            RunId::new("run-authorized"),
            "capture authorized workspace".to_string(),
        );
        let bus = RuntimeEventBus::new();

        runtime
            .query_engine_for_session(&session_id)
            .run_tool_with_bus(&turn, &bus, "capture_authorized_workspace")
            .await
            .unwrap();

        assert_eq!(
            seen_root.lock().unwrap().clone(),
            Some(external_workspace.path().to_path_buf())
        );
    }

    #[test]
    fn query_engine_for_session_reuses_state_within_session_and_isolates_across_sessions() {
        let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new());
        let session_a = crate::runtime::ids::SessionId::new("session-b2-a");
        let session_b = crate::runtime::ids::SessionId::new("session-b2-b");

        let engine_a_1 = runtime.query_engine_for_session(&session_a);
        engine_a_1.accumulate_usage(5, 7);

        let usage_a_2 = runtime
            .query_engine_for_session(&session_a)
            .get_total_usage();
        assert_eq!(usage_a_2.tokens_in, 5);
        assert_eq!(usage_a_2.tokens_out, 7);

        let usage_b = runtime
            .query_engine_for_session(&session_b)
            .get_total_usage();
        assert_eq!(
            usage_b.tokens_in, 0,
            "different sessions must not share total_usage.tokens_in"
        );
        assert_eq!(
            usage_b.tokens_out, 0,
            "different sessions must not share total_usage.tokens_out"
        );
    }

    #[test]
    fn clear_session_state_resets_cached_engine_state_for_that_session() {
        let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new());
        let session_id = crate::runtime::ids::SessionId::new("session-b2-clear");

        let engine_before_clear = runtime.query_engine_for_session(&session_id);
        let read_state_before_clear = engine_before_clear.read_file_state();
        engine_before_clear.accumulate_usage(8, 13);
        assert_eq!(engine_before_clear.get_total_usage().tokens_in, 8);
        assert_eq!(engine_before_clear.get_total_usage().tokens_out, 13);

        runtime.clear_session_state(&session_id);

        let engine_after_clear = runtime.query_engine_for_session(&session_id);
        let read_state_after_clear = engine_after_clear.read_file_state();
        let usage_after_clear = engine_after_clear.get_total_usage();
        assert_eq!(usage_after_clear.tokens_in, 0);
        assert_eq!(usage_after_clear.tokens_out, 0);
        assert!(
            !Arc::ptr_eq(&read_state_before_clear, &read_state_after_clear),
            "cleared session must get a fresh read_file_state cache"
        );
    }

    #[test]
    fn clear_session_state_cancels_pending_permission_requests_for_that_session() {
        let pending_permission_store = Arc::new(PendingPermissionRequestStore::new());
        let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new())
            .with_pending_permission_store(pending_permission_store.clone());
        let session_id = SessionId::new("session-b2-clear-pending");
        let tool_call_id = ToolCallId::new("tc-clear-pending");

        let resolution_rx = pending_permission_store
            .insert(crate::runtime::store::PendingPermissionRequest {
                tool_call_id: tool_call_id.clone(),
                session_id: session_id.clone(),
                run_id: RunId::new("run-clear-pending"),
                tool_name: "echo_tool".to_string(),
                capability_scopes: vec!["custom:test".to_string()],
                message: "need permission".to_string(),
                suggestions: vec!["Allow once".to_string()],
                mode: PermissionMode::Default,
                remember_options: vec![PermissionDestination::Session],
                default_destination: Some(PermissionDestination::Session),
                original_request: crate::runtime::chat::tool_round_types::RuntimeToolCallRequest {
                    tool_call_id: tool_call_id.as_str().to_string(),
                    tool_name: "echo_tool".to_string(),
                    args: serde_json::json!({}),
                    purpose: None,
                },
            })
            .unwrap();

        runtime.clear_session_state(&session_id);

        assert!(pending_permission_store.get(&tool_call_id).is_none());
        let resolution = resolution_rx.blocking_recv().unwrap();
        assert!(matches!(
            resolution,
            PendingPermissionResolution::Cancel { ref message }
                if message.contains("session state was cleared")
        ));
    }

    #[test]
    fn resolve_permission_request_persists_remembered_allow_to_selected_layer() {
        let pending_permission_store = Arc::new(PendingPermissionRequestStore::new());
        let permission_store = Arc::new(crate::runtime::store::PermissionStore::in_memory());
        let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new())
            .with_pending_permission_store(pending_permission_store.clone())
            .with_permission_store(permission_store.clone());
        let session_id = SessionId::new("session-b2-persist-allow");
        let tool_call_id = ToolCallId::new("tc-persist-allow");

        let _resolution_rx = pending_permission_store
            .insert(crate::runtime::store::PendingPermissionRequest {
                tool_call_id: tool_call_id.clone(),
                session_id,
                run_id: RunId::new("run-persist-allow"),
                tool_name: "echo_tool".to_string(),
                capability_scopes: vec!["custom:test".to_string()],
                message: "need permission".to_string(),
                suggestions: vec!["Allow once".to_string()],
                mode: PermissionMode::Default,
                remember_options: vec![
                    PermissionDestination::Session,
                    PermissionDestination::Workspace,
                    PermissionDestination::User,
                ],
                default_destination: Some(PermissionDestination::Session),
                original_request: crate::runtime::chat::tool_round_types::RuntimeToolCallRequest {
                    tool_call_id: tool_call_id.as_str().to_string(),
                    tool_name: "echo_tool".to_string(),
                    args: serde_json::json!({}),
                    purpose: None,
                },
            })
            .unwrap();

        runtime
            .resolve_permission_request(
                &tool_call_id,
                PendingPermissionResolution::Allow {
                    updated_input: None,
                    remember: true,
                    destination: Some(PermissionDestination::Workspace),
                },
            )
            .unwrap();

        assert!(pending_permission_store.get(&tool_call_id).is_none());
        assert_eq!(
            permission_store.get_for_scope("echo_tool", "custom:test"),
            Some(crate::runtime::store::PolicyDecision::Allow)
        );
    }

    #[test]
    fn resolve_permission_request_uses_default_destination_when_remembered_deny_has_no_target() {
        let pending_permission_store = Arc::new(PendingPermissionRequestStore::new());
        let permission_store = Arc::new(crate::runtime::store::PermissionStore::in_memory());
        let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new())
            .with_pending_permission_store(pending_permission_store.clone())
            .with_permission_store(permission_store.clone());
        let session_id = SessionId::new("session-b2-persist-deny");
        let tool_call_id = ToolCallId::new("tc-persist-deny");

        let _resolution_rx = pending_permission_store
            .insert(crate::runtime::store::PendingPermissionRequest {
                tool_call_id: tool_call_id.clone(),
                session_id,
                run_id: RunId::new("run-persist-deny"),
                tool_name: "echo_tool".to_string(),
                capability_scopes: vec!["custom:test".to_string()],
                message: "need permission".to_string(),
                suggestions: vec!["Deny".to_string()],
                mode: PermissionMode::Default,
                remember_options: vec![PermissionDestination::User],
                default_destination: Some(PermissionDestination::User),
                original_request: crate::runtime::chat::tool_round_types::RuntimeToolCallRequest {
                    tool_call_id: tool_call_id.as_str().to_string(),
                    tool_name: "echo_tool".to_string(),
                    args: serde_json::json!({}),
                    purpose: None,
                },
            })
            .unwrap();

        runtime
            .resolve_permission_request(
                &tool_call_id,
                PendingPermissionResolution::Deny {
                    message: "Denied by user".to_string(),
                    remember: true,
                    destination: None,
                },
            )
            .unwrap();

        assert_eq!(
            permission_store.get_for_scope("echo_tool", "custom:test"),
            Some(crate::runtime::store::PolicyDecision::Deny)
        );
    }

    #[test]
    fn session_runtime_reuses_cancel_root_until_it_is_cancelled() {
        let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new());
        let session = SessionId::new("sess-i1");

        let root_a = runtime.ensure_active_session_cancel_root(&session);
        let root_b = runtime.ensure_active_session_cancel_root(&session);

        runtime.cancel_session(
            &session,
            crate::runtime::cancellation::CancellationReason::Interrupt,
        );

        assert!(root_a.is_cancelled());
        assert!(root_b.is_cancelled());
        assert_eq!(
            root_a.reason(),
            Some(crate::runtime::cancellation::CancellationReason::Interrupt)
        );
        assert_eq!(
            root_b.reason(),
            Some(crate::runtime::cancellation::CancellationReason::Interrupt)
        );
    }

    #[test]
    fn session_runtime_rotates_cancel_root_after_interrupt() {
        let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new());
        let session = SessionId::new("sess-i1-rotate");

        let old_root = runtime.ensure_active_session_cancel_root(&session);
        runtime.cancel_session(
            &session,
            crate::runtime::cancellation::CancellationReason::Interrupt,
        );
        let new_root = runtime.ensure_active_session_cancel_root(&session);

        assert!(old_root.is_cancelled());
        assert_eq!(
            old_root.reason(),
            Some(crate::runtime::cancellation::CancellationReason::Interrupt)
        );
        assert!(!new_root.is_cancelled());
    }

    #[test]
    fn clear_session_state_drops_cached_cancel_root_for_that_session() {
        let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new());
        let session = SessionId::new("sess-i1-clear");

        let root_before = runtime.ensure_active_session_cancel_root(&session);
        runtime.clear_session_state(&session);
        let root_after = runtime.ensure_active_session_cancel_root(&session);

        runtime.cancel_session(
            &session,
            crate::runtime::cancellation::CancellationReason::Interrupt,
        );

        assert!(!root_before.is_cancelled());
        assert!(root_after.is_cancelled());
    }

    #[test]
    fn run_chat_request_with_agent_name_constrains_allowed_tools_from_registry() {
        let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new())
            .with_agent_registry(Arc::new(AgentRegistry::with_builtins()));
        let request =
            ChatTurnRequest::new("conv-agent-name", "hello", vec![]).with_agent_name(
                "browse_data_agent",
            );

        let constrained = runtime.apply_agent_tool_constraint(request);

        assert_eq!(constrained.agent_name.as_deref(), Some("browse_data_agent"));
        assert!(constrained.allowed_tools.is_some());
        let allowed = constrained.allowed_tools.unwrap();
        assert_eq!(allowed.len(), 6);
        for tool in [
            "browse_and_extract",
            "browse_navigate",
            "read_page_content",
            "page_execute_js",
            "extract_table_data",
            "extract_with_pagination",
        ] {
            assert!(
                allowed.iter().any(|allowed_tool| allowed_tool == tool),
                "agent-constrained allowlist must include {tool}"
            );
        }
    }
}
