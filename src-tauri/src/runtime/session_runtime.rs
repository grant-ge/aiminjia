use std::sync::Arc;

use anyhow::Result;

// Import and re-export from chat module.  Types were previously defined here;
// they now live in `runtime::chat` to avoid circular imports.
pub use crate::runtime::chat::{ChatTurnRequest, RuntimeTurnExecutor};
use crate::runtime::cancellation::CancellationToken;
use crate::runtime::chat::{RuntimeChatTurnDriver, RuntimeLlmExecutor};
use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};
use crate::runtime::identity::IdentityMapping;
use crate::runtime::ids::RunId;
use crate::runtime::query_engine::QueryEngine;
use crate::runtime::store::{AuthorizedWorkspaceRef, AuthorizedWorkspaceStore};
use crate::runtime::state::TurnState;
use crate::transport::runtime_host::RuntimeHost;
use crate::transport::tauri_event_adapter::TauriEventAdapter;

#[derive(Clone)]
pub struct SessionRuntime {
    query_engine: QueryEngine,
    event_bus: RuntimeEventBus,
    /// Legacy executor retained as a runtime-controlled helper during the
    /// chat-runtime-first migration.
    ///
    /// `SessionRuntime` no longer directly delegates full chat turns to this
    /// executor. Instead, `RuntimeChatTurnDriver` remains the single turn entry
    /// point and decides when the helper is invoked.
    legacy_executor: Option<Arc<dyn RuntimeTurnExecutor>>,
    /// S4 new executor: owns the query loop; executor is a provider streaming adapter only.
    /// When present, `build_driver_for_turn` uses `RuntimeChatTurnDriver::with_llm_executor`
    /// (S4 path) instead of the legacy executor path.
    llm_executor: Option<Arc<dyn RuntimeLlmExecutor>>,
    authorized_workspace_store: Option<Arc<dyn AuthorizedWorkspaceStore>>,
    /// Session-level cancellation token.  When set, every `TurnState` produced by
    /// `run_chat_request` receives a **child** token so that cancelling the session
    /// token cascades down to the active turn and its tool calls.
    ///
    /// The token is injected by the transport layer (e.g. via
    /// `with_cancellation_token`) and should be sourced from the same cancel
    /// hierarchy that `stop_streaming` / `RuntimeRunRegistry` uses, so that the
    /// runtime path and the legacy agent-loop path share one cancel tree.
    cancel_token: Option<CancellationToken>,
}

impl SessionRuntime {
    pub fn new(query_engine: QueryEngine, event_bus: RuntimeEventBus) -> Self {
        Self {
            query_engine,
            event_bus,
            legacy_executor: None,
            llm_executor: None,
            authorized_workspace_store: None,
            cancel_token: None,
        }
    }

    /// Construct a `SessionRuntime` with a legacy executor helper.
    ///
    /// The executor is no longer the direct owner of the full chat turn.
    /// `RuntimeChatTurnDriver` remains the single runtime entry and may invoke
    /// this helper on executor-backed production paths to preserve current LLM /
    /// transport behavior while ownership migrates into the runtime.
    pub fn with_executor(
        query_engine: QueryEngine,
        event_bus: RuntimeEventBus,
        turn_executor: Arc<dyn RuntimeTurnExecutor>,
    ) -> Self {
        Self {
            query_engine,
            event_bus,
            legacy_executor: Some(turn_executor),
            llm_executor: None,
            authorized_workspace_store: None,
            cancel_token: None,
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
            event_bus,
            legacy_executor: None,
            llm_executor: Some(executor),
            authorized_workspace_store: None,
            cancel_token: None,
        }
    }

    pub fn with_authorized_workspace_store(
        mut self,
        authorized_workspace_store: Arc<dyn AuthorizedWorkspaceStore>,
    ) -> Self {
        self.authorized_workspace_store = Some(authorized_workspace_store);
        self
    }

    /// Attach a session-level cancellation token.
    ///
    /// Every `TurnState` constructed by [`run_chat_request`] will receive a
    /// **child** of this token, so that cancelling `token` (e.g. from
    /// `stop_streaming`) cascades down to the active turn and its tool calls.
    ///
    /// The token should be sourced from the same cancel hierarchy that the
    /// `RuntimeRunRegistry` / gateway cancel path uses so that both the
    /// pure-runtime and the legacy agent-loop paths share one cancel tree.
    pub fn with_cancellation_token(mut self, token: CancellationToken) -> Self {
        self.cancel_token = Some(token);
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
        let mapping =
            IdentityMapping::from_legacy_conversation_id(request.conversation_id.clone());
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

        // If a session-level cancel token is present, derive a child token so
        // that cancelling the session (e.g. stop_streaming) cascades into this
        // turn and its tool calls.
        if let Some(ref session_token) = self.cancel_token {
            turn = turn.with_cancellation(session_token.child_token());
        }

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

    fn query_engine_for_session(
        &self,
        session_id: &crate::runtime::ids::SessionId,
    ) -> QueryEngine {
        let authorized_workspace = self
            .authorized_workspace_store
            .as_ref()
            .and_then(|store| store.get_current_for_session(session_id).ok().flatten())
            .map(|aw| AuthorizedWorkspaceRef {
                id: aw.id,
                root_path: aw.root_path,
                display_name: aw.display_name,
            });
        self.query_engine
            .clone()
            .with_authorized_workspace(authorized_workspace)
    }

    /// Build a `RuntimeChatTurnDriver` scoped to the given turn's session.
    fn build_driver_for_turn(&self, turn: &TurnState) -> RuntimeChatTurnDriver {
        let query_engine = self.query_engine_for_session(turn.session_id());
        // S4 path takes priority: when an llm_executor is present, use the S4 driver.
        if let Some(ref executor) = self.llm_executor {
            return RuntimeChatTurnDriver::with_llm_executor(
                query_engine,
                self.event_bus.clone(),
                executor.clone(),
            );
        }
        match &self.legacy_executor {
            Some(executor) => RuntimeChatTurnDriver::with_legacy_executor(
                query_engine,
                self.event_bus.clone(),
                executor.clone(),
            ),
            None => RuntimeChatTurnDriver::new(query_engine, self.event_bus.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tools::{
        AllowAllPermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatcher,
        ToolError, ToolExecutionContext, ToolResult,
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
            ToolDefinition::new("capture_authorized_workspace", "Capture authorized workspace")
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

        let mapping =
            IdentityMapping::from_legacy_conversation_id(session_id.as_str().to_string());
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
}
