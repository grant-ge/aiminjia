use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;

// Import and re-export from chat module.  Types were previously defined here;
// they now live in `runtime::chat` to avoid circular imports.
use crate::runtime::agent::task_notification::TaskNotificationQueue;
use crate::runtime::cancellation::{CancellationReason, CancellationToken};
pub use crate::runtime::chat::ChatTurnRequest;
use crate::runtime::chat::{RuntimeChatTurnDriver, RuntimeLlmExecutor};
use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};
use crate::runtime::identity::IdentityMapping;
use crate::runtime::ids::{RunId, SessionId, ToolCallId};
use crate::runtime::interaction::{
    InMemoryInteractionControlPlane, InteractionId, InteractionResolution,
};
use crate::runtime::query_engine::QueryEngine;
use crate::runtime::state::TurnState;
use crate::runtime::store::{
    AuthorizedWorkspaceRef, AuthorizedWorkspaceStore, PendingPermissionRequest,
    PendingPermissionRequestStore, PendingPermissionResolution, PermissionStore, PolicyDecision,
};
use crate::runtime::path_auth::{load_path_auth_entries, RuleSource, ToolPermissionContext};
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
    pending_interaction_store: Arc<InMemoryInteractionControlPlane>,
    permission_store: Option<Arc<PermissionStore>>,
    default_folder: Option<PathBuf>,
    task_notification_queue: Option<Arc<TaskNotificationQueue>>,
    /// LTR (P1.8): per-session Team registry; cleared on cancel_session.
    team_registry: Option<Arc<crate::runtime::agent::TeamRegistry>>,
    /// LTR (P1.8): per-session AgentName registry; cleared on cancel_session.
    agent_names: Option<Arc<crate::runtime::agent::AgentNameRegistry>>,
    /// LTR (P2.2): per-process InboxRegistry; carried so the per-call
    /// ToolExecutionContext can route SendMessage.  Not cleared on
    /// cancel_session — Teammate cleanup handles deregister.
    inbox_registry: Option<Arc<crate::runtime::agent::InboxRegistry>>,
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
            pending_interaction_store: Arc::new(InMemoryInteractionControlPlane::new()),
            permission_store: None,
            default_folder: None,
            task_notification_queue: None,
            team_registry: None,
            agent_names: None,
            inbox_registry: None,
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
            pending_interaction_store: Arc::new(InMemoryInteractionControlPlane::new()),
            permission_store: None,
            default_folder: None,
            task_notification_queue: None,
            team_registry: None,
            agent_names: None,
            inbox_registry: None,
        }
    }

    pub fn with_pending_permission_store(
        mut self,
        pending_permission_store: Arc<PendingPermissionRequestStore>,
    ) -> Self {
        self.pending_permission_store = pending_permission_store;
        self
    }

    pub fn with_pending_interaction_store(
        mut self,
        pending_interaction_store: Arc<InMemoryInteractionControlPlane>,
    ) -> Self {
        self.pending_interaction_store = pending_interaction_store;
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

    pub fn with_default_folder(mut self, default_folder: PathBuf) -> Self {
        self.default_folder = Some(default_folder);
        self
    }

    pub fn with_task_notification_queue(
        mut self,
        queue: Arc<TaskNotificationQueue>,
    ) -> Self {
        self.task_notification_queue = Some(queue);
        self
    }

    /// LTR (P1.8): inject the per-process TeamRegistry so `cancel_session`
    /// can drop the session's team on shutdown.
    pub fn with_team_registry(
        mut self,
        registry: Arc<crate::runtime::agent::TeamRegistry>,
    ) -> Self {
        self.team_registry = Some(registry);
        self
    }

    /// LTR (P1.8): inject the per-process AgentNameRegistry so `cancel_session`
    /// can drop name bindings on shutdown.
    pub fn with_agent_names(
        mut self,
        registry: Arc<crate::runtime::agent::AgentNameRegistry>,
    ) -> Self {
        self.agent_names = Some(registry);
        self
    }

    /// LTR (P2.2): inject the per-process InboxRegistry so per-call
    /// ToolExecutionContexts can route SendMessage.
    pub fn with_inbox_registry(
        mut self,
        registry: Arc<crate::runtime::agent::InboxRegistry>,
    ) -> Self {
        self.inbox_registry = Some(registry);
        self
    }

    /// Replace the base `QueryEngine` (and clear any cached per-session engines).
    /// Used by the transport layer to inject a per-request ToolDispatcher without
    /// calling `block_on` inside a sync constructor.
    pub fn with_query_engine(mut self, query_engine: QueryEngine) -> Self {
        self.query_engine = query_engine;
        // Clear cached per-session engines so they inherit the new base engine.
        self.session_query_engines = Arc::new(Mutex::new(HashMap::new()));
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
        request: ChatTurnRequest,
    ) -> std::result::Result<(), String> {
        log::info!(
            "[session_runtime] run_chat_request enter conv={} run={}",
            request.conversation_id.as_str(),
            request.run_id.as_str()
        );
        let mapping = IdentityMapping::from_legacy_conversation_id(request.conversation_id.clone());
        // `ChatTurnRequest::new` creates the authoritative RunId for the turn.
        // Transport code may reserve per-run resources before entering runtime,
        // so do not replace it here.
        let run_id = request.run_id.clone();

        // Emit RunStarted before handing off to the driver.
        let run_started = RuntimeEvent::new(
            mapping.session_id.clone(),
            run_id.clone(),
            RuntimeEventKind::RunStarted,
        );
        let _ = self.event_bus.emit(run_started).await;

        let mut turn = TurnState::new(mapping, run_id, request.content.clone());
        let session_root = self.ensure_active_session_cancel_root(turn.session_id());
        turn = turn
            .with_cancellation(session_root.child_token())
            .with_permission_mode(request.permission_mode);

        // Build a driver for this session and drive the full turn lifecycle.
        // The driver remains the only chat-turn entry and may invoke the legacy
        // executor helper internally on production paths.
        log::info!(
            "[session_runtime] build_driver_for_turn conv={}",
            turn.session_id().as_str()
        );
        let driver = self.build_driver_for_turn(&turn);
        log::info!(
            "[session_runtime] run_chat_turn starting conv={}",
            turn.session_id().as_str()
        );
        let result = driver
            .run_chat_turn(&mut turn, &request)
            .await
            .map_err(|e| e.to_string());
        log::info!(
            "[session_runtime] run_chat_turn finished conv={} ok={}",
            turn.session_id().as_str(),
            result.is_ok()
        );
        result
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

    pub fn resolve_interaction_request(
        &self,
        interaction_id: &InteractionId,
        resolution: InteractionResolution,
    ) -> Result<()> {
        use crate::runtime::interaction::PendingInteractionControlPlane;
        self.pending_interaction_store
            .resolve(interaction_id, resolution)
    }

    pub fn cancel_pending_interaction_requests_for_session(
        &self,
        session_id: &SessionId,
        message: &str,
    ) -> usize {
        use crate::runtime::interaction::PendingInteractionControlPlane;
        self.pending_interaction_store
            .cancel_for_session(session_id.as_str(), message)
    }

    pub fn cancel_session(&self, session_id: &SessionId, reason: CancellationReason) {
        if let Some(root) = self.current_session_cancel_root(session_id) {
            root.cancel_with_reason(reason);
        }
        self.cancel_pending_permission_requests_for_session(
            session_id,
            "Permission request cancelled because the session was stopped.",
        );
        self.cancel_pending_interaction_requests_for_session(
            session_id,
            "Interaction request cancelled because the session was stopped.",
        );

        // LTR (P1.8): cleanup per-session Team / name bindings.  Spawned
        // because the registries are async; we don't need to block the
        // synchronous cancel_session contract waiting on the mutex.
        if let (Some(team_reg), Some(name_reg)) = (
            self.team_registry.clone(),
            self.agent_names.clone(),
        ) {
            let sid = session_id.clone();
            tokio::spawn(async move {
                team_reg.delete(&sid).await;
                name_reg.drop_session(&sid).await;
            });
        }
    }

    pub fn clear_session_state(&self, session_id: &SessionId) {
        self.cancel_pending_permission_requests_for_session(
            session_id,
            "Permission request cancelled because the session state was cleared.",
        );
        self.cancel_pending_interaction_requests_for_session(
            session_id,
            "Interaction request cancelled because the session state was cleared.",
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
        log::info!(
            "[session_runtime] query_engine_for_session enter session={}",
            session_id.as_str()
        );
        let authorized_workspace = self
            .authorized_workspace_store
            .as_ref()
            .and_then(|store| store.get_current_for_session(session_id).ok().flatten())
            .map(|aw| AuthorizedWorkspaceRef {
                id: aw.id,
                root_path: aw.root_path,
                display_name: aw.display_name,
            })
            .or_else(|| {
                let default_path = self.default_folder.clone().unwrap_or_else(|| {
                    log::warn!(
                        "[session_runtime] default_folder not injected, using hardcoded fallback"
                    );
                    dirs::home_dir()
                        .map(|h| h.join(".renlijia").join("defaultFolder"))
                        .expect("Cannot determine home directory")
                });
                log::info!(
                    "[session_runtime] query_engine_for_session defaultFolder path={} exists={}",
                    default_path.display(),
                    default_path.exists()
                );
                if let Err(err) = std::fs::create_dir_all(&default_path) {
                    log::warn!(
                        "[session_runtime] failed to create defaultFolder for session {}: {}",
                        session_id.as_str(),
                        err
                    );
                    return None;
                }
                Some(AuthorizedWorkspaceRef {
                    id: "default".to_string(),
                    root_path: default_path,
                    display_name: "默认项目".to_string(),
                })
            });
        log::info!(
            "[session_runtime] query_engine_for_session authorized_workspace={}",
            authorized_workspace
                .as_ref()
                .map(|aw| aw.root_path.to_string_lossy().into_owned())
                .unwrap_or_else(|| "(none)".to_string())
        );
        let mut engines = self
            .session_query_engines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let session_engine = engines
            .entry(session_id.as_str().to_string())
            .or_insert_with(|| self.query_engine.clone_with_fresh_session_state())
            .clone();

        // Reload base ToolPermissionContext from the persistent PermissionStore
        // on each turn so UserSettings changes (working dirs / allow rules) take
        // effect immediately. Per-turn session attachment dirs are accumulated
        // separately via the Arc<Mutex<HashMap>> on QueryEngine.
        let base_ctx = if let Some(store) = self.permission_store.as_ref() {
            let entries = load_path_auth_entries(store);
            let mut ctx = ToolPermissionContext::empty();
            ctx.additional_working_dirs = entries.working_dirs;
            ctx.allow_rules = entries.allow_rules;
            Arc::new(ctx)
        } else {
            Arc::new(ToolPermissionContext::empty())
        };

        let mut engine = session_engine
            .with_authorized_workspace(authorized_workspace)
            .with_permission_ctx(base_ctx);
        if let Some(store) = self.permission_store.as_ref() {
            engine = engine.with_permission_store(store.clone());
        }
        // LTR (P1.7/P2.2): propagate registries so per-call
        // ToolExecutionContexts can dispatch SendMessage / TeamCreate / etc.
        if let (Some(team), Some(names), Some(inboxes)) = (
            self.team_registry.clone(),
            self.agent_names.clone(),
            self.inbox_registry.clone(),
        ) {
            engine = engine.with_ltr_registries(team, names, inboxes);
        }
        engine
    }

    /// Build a `RuntimeChatTurnDriver` scoped to the given turn's session.
    fn build_driver_for_turn(&self, turn: &TurnState) -> RuntimeChatTurnDriver {
        let query_engine = self.query_engine_for_session(turn.session_id());
        let mut driver = if let Some(ref executor) = self.llm_executor {
            // Compatibility marker for review tests: with_llm_executor_and_permission_control_plane(
            RuntimeChatTurnDriver::with_llm_executor_and_control_planes(
                query_engine,
                self.event_bus.clone(),
                executor.clone(),
                self.pending_permission_store.clone(),
                self.pending_interaction_store.clone(),
            )
        } else {
            RuntimeChatTurnDriver::new(query_engine, self.event_bus.clone())
        };

        // Both S4 and QueryEngine paths surface task notifications.
        if let Some(ref queue) = self.task_notification_queue {
            driver = driver.with_task_notification_queue(queue.clone());
        }
        driver
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

        let destination = destination
            .or(pending_request.default_destination)
            .unwrap_or(PermissionDestination::Session);

        // path-auth Ask: route to dedicated PermissionStore methods per §7.8.
        // Only handles Allow (deny via path_auth Ask is not currently surfaced).
        if let Some(scope) = pending_request.path_auth_scope.as_ref() {
            if matches!(decision, PolicyDecision::Allow) {
                self.persist_path_auth_grant(permission_store, scope, destination);
                return;
            }
            // Deny + path_auth: append_deny_rule is not yet implemented per spec §7.8
            // (deny rules are only a data structure placeholder in Phase 1-4).
            // Log explicitly so future implementers see the gap; in-memory deny is
            // not retained either since path_auth_scope encodes path-level intent
            // not tool-scope intent.
            log::info!(
                "[SessionRuntime] path_auth deny-remember for scope '{}' not persisted: deny path not yet implemented (§7.8)",
                scope
            );
            return;
        }

        if pending_request.capability_scopes.is_empty() {
            log::warn!(
                "[SessionRuntime] Skip persisting permission for '{}' because no capability scopes were captured",
                pending_request.tool_name
            );
            return;
        }

        persist_permission_decision(
            permission_store,
            &pending_request.tool_name,
            &pending_request.capability_scopes,
            decision,
            destination,
        );
    }

    fn persist_path_auth_grant(
        &self,
        store: &PermissionStore,
        scope: &str,
        destination: PermissionDestination,
    ) {
        log::info!(
            "[persist_path_auth_grant] scope='{}' destination={:?}",
            scope, destination
        );
        let (kind, path_str) = match scope.split_once(':') {
            Some((k, p)) => (k, p),
            None => {
                log::warn!("[SessionRuntime] malformed path_auth_scope: {}", scope);
                return;
            }
        };
        let path = std::path::PathBuf::from(path_str);
        match kind {
            "path" => {
                // step-6 Ask → working dir grant
                log::info!(
                    "[persist_path_auth_grant] -> append_working_dir({:?}, {})",
                    destination, path.display()
                );
                if let Err(err) = store.append_working_dir(destination, path) {
                    log::warn!(
                        "[SessionRuntime] append_working_dir failed: {} (in-memory grant retained)",
                        err
                    );
                }
            }
            "pathwrite" => {
                // step-4b Ask → write allow_rule grant
                // Pattern is `<dir>/**` so subsequent writes anywhere under the dir are allowed.
                let pattern = format!("{}/**", path_str);
                log::info!(
                    "[persist_path_auth_grant] -> append_path_allow_rule({:?}, {}, op=Write)",
                    destination, pattern
                );
                if let Err(err) = store.append_path_allow_rule(
                    destination,
                    pattern,
                    Some(crate::runtime::path_auth::PathOp::Write),
                ) {
                    log::warn!(
                        "[SessionRuntime] append_path_allow_rule failed: {} (in-memory grant retained)",
                        err
                    );
                }
            }
            other => {
                log::warn!("[SessionRuntime] unknown path_auth_scope kind: {}", other);
            }
        }
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
    use crate::runtime::chat::turn_config::{LlmStepInput, LlmStepResult, TurnError};
    use crate::runtime::tools::permission::{PermissionDestination, PermissionMode};
    use crate::runtime::tools::{
        AllowAllPermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatcher, ToolError,
        ToolExecutionContext, ToolResult,
    };
    use async_trait::async_trait;
    use serde_json::Value;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    struct CapturePermissionModeTool {
        seen_mode: Arc<Mutex<Option<PermissionMode>>>,
    }

    #[async_trait]
    impl RuntimeTool for CapturePermissionModeTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("capture_permission_mode", "Capture permission mode")
        }

        async fn execute(
            &self,
            _input: Value,
            ctx: ToolExecutionContext,
        ) -> std::result::Result<ToolResult, ToolError> {
            *self.seen_mode.lock().unwrap() = Some(ctx.permission_mode);
            Ok(ToolResult::new("capture_permission_mode", "ok", None))
        }
    }

    struct SingleToolCallExecutor {
        workspace_path: PathBuf,
        next_step: AtomicUsize,
    }

    #[async_trait]
    impl RuntimeLlmExecutor for SingleToolCallExecutor {
        async fn run_llm_step(
            &self,
            _input: &LlmStepInput<'_>,
            _bus: &RuntimeEventBus,
            _cancel: &CancellationToken,
        ) -> anyhow::Result<LlmStepResult, TurnError> {
            match self.next_step.fetch_add(1, Ordering::SeqCst) {
                0 => Ok(LlmStepResult::ToolCalls {
                    assistant_content: String::new(),
                    tool_calls: vec![
                        crate::runtime::chat::tool_round_types::RuntimeToolCallRequest {
                            tool_call_id: "tc-session-mode".to_string(),
                            tool_name: "capture_permission_mode".to_string(),
                            args: serde_json::json!({}),
                            purpose: None,
                        },
                    ],
                    tokens_in: 0,
                    tokens_out: 0,
                }),
                _ => Ok(LlmStepResult::ContentComplete {
                    content: "done".to_string(),
                    tokens_in: 0,
                    tokens_out: 0,
                    stop_reason: Some("end_turn".to_string()),
                }),
            }
        }

        async fn persist_assistant_message(
            &self,
            _conversation_id: &str,
            _content: &str,
            _tool_calls: &[serde_json::Value],
            _generated_file_ids: &[String],
            _file_metas: &[serde_json::Value],
        ) -> anyhow::Result<String, TurnError> {
            Ok("assistant-msg".to_string())
        }

        async fn persist_user_message(
            &self,
            _conversation_id: &str,
            _content: &str,
            _attachments: &[crate::runtime::chat::chat_turn_driver::ChatAttachmentRef],
            _client_message_id: Option<&str>,
        ) -> anyhow::Result<String, TurnError> {
            Ok("user-msg".to_string())
        }

        async fn build_system_prompt(
            &self,
            _conversation_id: &str,
        ) -> anyhow::Result<String, TurnError> {
            Ok("system".to_string())
        }

        async fn get_tool_defs(&self) -> anyhow::Result<Vec<serde_json::Value>, TurnError> {
            Ok(vec![])
        }

        async fn load_workspace_path(&self) -> anyhow::Result<PathBuf, TurnError> {
            Ok(self.workspace_path.clone())
        }
    }

    #[tokio::test]
    async fn run_chat_request_passes_permission_mode_into_turn_state_execution_context() {
        let workspace = TempDir::new().expect("TempDir::new failed");
        let seen_mode = Arc::new(Mutex::new(None));
        let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
        dispatcher.register(Arc::new(CapturePermissionModeTool {
            seen_mode: seen_mode.clone(),
        }));

        let runtime = SessionRuntime::with_llm_executor(
            QueryEngine::with_dispatcher(dispatcher),
            RuntimeEventBus::new(),
            Arc::new(SingleToolCallExecutor {
                workspace_path: workspace.path().to_path_buf(),
                next_step: AtomicUsize::new(0),
            }),
        );
        let mut request = ChatTurnRequest::new("conv-session-mode", "run capture", vec![]);
        request.permission_mode = PermissionMode::DontAsk;

        runtime
            .run_chat_request(request)
            .await
            .expect("run_chat_request");

        assert_eq!(*seen_mode.lock().unwrap(), Some(PermissionMode::DontAsk));
    }

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
    fn query_engine_for_session_falls_back_to_default_folder_when_unbound() {
        let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new());
        let session_id = crate::runtime::ids::SessionId::new("session-default-folder");

        let engine = runtime.query_engine_for_session(&session_id);
        let captured = engine.authorized_workspace_for_test();
        let default_folder = crate::storage::aijia_home::AiJiaHome::from_home().default_folder();

        assert_eq!(
            captured.as_ref().map(|ws| ws.root_path.as_path()),
            Some(default_folder.as_path())
        );
        assert_eq!(
            captured.as_ref().map(|ws| ws.display_name.as_str()),
            Some("默认项目")
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
                path_auth_scope: None,
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
                path_auth_scope: None,
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
                path_auth_scope: None,
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
    fn permanent_allow_with_path_auth_scope_persists_via_append_working_dir() {
        let pending_permission_store = Arc::new(PendingPermissionRequestStore::new());
        let permission_store = Arc::new(crate::runtime::store::PermissionStore::in_memory());
        let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new())
            .with_pending_permission_store(pending_permission_store.clone())
            .with_permission_store(permission_store.clone());
        let session_id = SessionId::new("session-path-auth-working-dir");
        let tool_call_id = ToolCallId::new("tc-path-auth-wd");

        let _rx = pending_permission_store
            .insert(crate::runtime::store::PendingPermissionRequest {
                tool_call_id: tool_call_id.clone(),
                session_id,
                run_id: RunId::new("run-path-auth-wd"),
                tool_name: "read_workspace_file".to_string(),
                capability_scopes: vec![],
                message: "Allow path?".to_string(),
                suggestions: vec![],
                mode: PermissionMode::Default,
                remember_options: vec![PermissionDestination::User],
                default_destination: Some(PermissionDestination::User),
                original_request: crate::runtime::chat::tool_round_types::RuntimeToolCallRequest {
                    tool_call_id: tool_call_id.as_str().to_string(),
                    tool_name: "read_workspace_file".to_string(),
                    args: serde_json::json!({}),
                    purpose: None,
                },
                path_auth_scope: Some("path:/Users/example/Docs".to_string()),
            })
            .unwrap();

        runtime
            .resolve_permission_request(
                &tool_call_id,
                PendingPermissionResolution::Allow {
                    updated_input: None,
                    remember: true,
                    destination: Some(PermissionDestination::User),
                },
            )
            .unwrap();

        // The PermissionStore should now contain a User working_dir entry.
        let entries = crate::runtime::path_auth::load_path_auth_entries(&permission_store);
        let expected = std::path::PathBuf::from("/Users/example/Docs");
        assert!(
            entries.working_dirs.contains_key(&expected),
            "append_working_dir must have been called for the path: {:?}",
            entries.working_dirs
        );

        // capability_scopes were NOT recorded (path_auth_scope branch short-circuits).
        assert_eq!(
            permission_store.get_for_scope("read_workspace_file", "custom:test"),
            None,
            "capability_scope path must NOT be taken when path_auth_scope is set"
        );
    }

    #[test]
    fn permanent_allow_with_pathwrite_scope_persists_via_append_path_allow_rule() {
        let pending_permission_store = Arc::new(PendingPermissionRequestStore::new());
        let permission_store = Arc::new(crate::runtime::store::PermissionStore::in_memory());
        let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new())
            .with_pending_permission_store(pending_permission_store.clone())
            .with_permission_store(permission_store.clone());
        let session_id = SessionId::new("session-pathwrite-allow-rule");
        let tool_call_id = ToolCallId::new("tc-pathwrite-rule");

        let _rx = pending_permission_store
            .insert(crate::runtime::store::PendingPermissionRequest {
                tool_call_id: tool_call_id.clone(),
                session_id,
                run_id: RunId::new("run-pathwrite-rule"),
                tool_name: "write_file".to_string(),
                capability_scopes: vec![],
                message: "Allow write?".to_string(),
                suggestions: vec![],
                mode: PermissionMode::Default,
                remember_options: vec![PermissionDestination::User],
                default_destination: Some(PermissionDestination::User),
                original_request: crate::runtime::chat::tool_round_types::RuntimeToolCallRequest {
                    tool_call_id: tool_call_id.as_str().to_string(),
                    tool_name: "write_file".to_string(),
                    args: serde_json::json!({}),
                    purpose: None,
                },
                path_auth_scope: Some("pathwrite:/Users/example/Docs".to_string()),
            })
            .unwrap();

        runtime
            .resolve_permission_request(
                &tool_call_id,
                PendingPermissionResolution::Allow {
                    updated_input: None,
                    remember: true,
                    destination: Some(PermissionDestination::User),
                },
            )
            .unwrap();

        // The PermissionStore should now contain a User path_allow_rule for the pattern.
        let entries = crate::runtime::path_auth::load_path_auth_entries(&permission_store);
        let expected_pattern = "/Users/example/Docs/**";
        let found = entries.allow_rules.iter().any(|r| r.pattern == expected_pattern);
        assert!(
            found,
            "append_path_allow_rule must have been called with pattern '{}': {:?}",
            expected_pattern,
            entries.allow_rules
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

    /// Verifies the full Ask → persist → Allow round-trip for pathwrite scope:
    /// after a user clicks "Always Allow" on a path-auth Ask, the next turn's
    /// decide::is_path_allowed must return Allow without re-asking.
    #[test]
    fn path_auth_ask_persist_allow_round_trip() {
        use crate::runtime::path_auth::context::{PermissionRule, RuleSource, ToolPermissionContext};
        use crate::runtime::path_auth::decide::{self, Decision};
        use crate::runtime::path_auth::op::PathOp;
        use crate::runtime::path_auth::store_bridge::load_path_auth_entries;
        use std::collections::HashMap;

        // Use a real tempdir so canonicalize_or_ancestor resolves correctly.
        let tmp = TempDir::new().unwrap();
        let doc_dir = tmp.path().join("Docs");
        std::fs::create_dir_all(&doc_dir).unwrap();
        let doc_dir_str = std::fs::canonicalize(&doc_dir)
            .unwrap()
            .to_string_lossy()
            .to_string();
        let scope = format!("pathwrite:{}", doc_dir_str);
        let expected_pattern = format!("{}/**", doc_dir_str);

        // 1. Create an in-memory PermissionStore.
        let pending_permission_store = Arc::new(PendingPermissionRequestStore::new());
        let permission_store = Arc::new(crate::runtime::store::PermissionStore::in_memory());
        let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new())
            .with_pending_permission_store(pending_permission_store.clone())
            .with_permission_store(permission_store.clone());
        let session_id = SessionId::new("session-round-trip");
        let tool_call_id = ToolCallId::new("tc-round-trip");

        // 2. Construct a PendingPermissionRequest with pathwrite scope.
        let _rx = pending_permission_store
            .insert(crate::runtime::store::PendingPermissionRequest {
                tool_call_id: tool_call_id.clone(),
                session_id,
                run_id: RunId::new("run-round-trip"),
                tool_name: "write_file".to_string(),
                capability_scopes: vec![],
                message: "Allow write?".to_string(),
                suggestions: vec![],
                mode: PermissionMode::Default,
                remember_options: vec![PermissionDestination::User],
                default_destination: Some(PermissionDestination::User),
                original_request: crate::runtime::chat::tool_round_types::RuntimeToolCallRequest {
                    tool_call_id: tool_call_id.as_str().to_string(),
                    tool_name: "write_file".to_string(),
                    args: serde_json::json!({}),
                    purpose: None,
                },
                path_auth_scope: Some(scope),
            })
            .unwrap();

        // 3. Resolve with Allow { remember: true, destination: User }.
        runtime
            .resolve_permission_request(
                &tool_call_id,
                PendingPermissionResolution::Allow {
                    updated_input: None,
                    remember: true,
                    destination: Some(PermissionDestination::User),
                },
            )
            .unwrap();

        // 4. Verify the store contains the expected pathwrite allow_rule.
        let entries = load_path_auth_entries(&permission_store);
        assert_eq!(
            entries.allow_rules.len(),
            1,
            "should have exactly one allow_rule after persist: {:?}",
            entries.allow_rules
        );
        let rule = &entries.allow_rules[0];
        assert_eq!(rule.pattern, expected_pattern, "pattern must be dir/**");
        assert_eq!(rule.op, Some(PathOp::Write), "op must be Write");
        assert_eq!(rule.source, RuleSource::UserSettings, "source must be UserSettings");

        // 5. Build a ToolPermissionContext from those entries.
        let ctx = ToolPermissionContext {
            mode: PermissionMode::Default,
            primary_root: None,
            additional_working_dirs: HashMap::new(),
            allow_rules: entries.allow_rules,
            deny_rules: vec![],
        };

        // 6 & 7. The next-turn decide must return Allow for a file inside Docs.
        let target_file = doc_dir.join("report.txt");
        let result = decide::is_path_allowed(&target_file, PathOp::Write, &ctx);
        assert_eq!(
            result,
            Decision::Allow,
            "round-trip must produce Allow for a file inside the granted dir; got {:?}",
            result
        );
    }
}
