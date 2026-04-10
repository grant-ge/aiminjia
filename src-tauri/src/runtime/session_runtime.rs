use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::identity::IdentityMapping;
use crate::runtime::ids::RunId;
use crate::runtime::query_engine::QueryEngine;
use crate::runtime::state::TurnState;
use crate::transport::runtime_host::RuntimeHost;
use crate::transport::tauri_event_adapter::TauriEventAdapter;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatTurnRequest {
    pub conversation_id: String,
    pub content: String,
    pub file_ids: Vec<String>,
}

impl ChatTurnRequest {
    pub fn new(
        conversation_id: impl Into<String>,
        content: impl Into<String>,
        file_ids: Vec<String>,
    ) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            content: content.into(),
            file_ids,
        }
    }
}

#[async_trait]
pub trait RuntimeTurnExecutor: Send + Sync {
    async fn run_chat_turn(&self, request: ChatTurnRequest) -> std::result::Result<(), String>;
}

#[derive(Clone)]
pub struct SessionRuntime {
    query_engine: QueryEngine,
    event_bus: RuntimeEventBus,
    turn_executor: Option<Arc<dyn RuntimeTurnExecutor>>,
}

impl SessionRuntime {
    pub fn new(query_engine: QueryEngine, event_bus: RuntimeEventBus) -> Self {
        Self {
            query_engine,
            event_bus,
            turn_executor: None,
        }
    }

    pub fn with_executor(
        query_engine: QueryEngine,
        event_bus: RuntimeEventBus,
        turn_executor: Arc<dyn RuntimeTurnExecutor>,
    ) -> Self {
        Self {
            query_engine,
            event_bus,
            turn_executor: Some(turn_executor),
        }
    }

    pub fn for_test(host: Arc<dyn RuntimeHost>) -> Self {
        let adapter = Arc::new(TauriEventAdapter::new(host));
        let bus = RuntimeEventBus::new();
        bus.subscribe(adapter);
        Self::new(QueryEngine::new(), bus)
    }

    pub async fn run_turn(&self, turn: &mut TurnState) -> Result<()> {
        self.query_engine.run(turn, &self.event_bus).await
    }

    pub async fn run_chat_request(
        &self,
        request: ChatTurnRequest,
    ) -> std::result::Result<(), String> {
        if let Some(executor) = &self.turn_executor {
            return executor.run_chat_turn(request).await;
        }

        let mapping = IdentityMapping::from_legacy_conversation_id(request.conversation_id);
        let mut turn = TurnState::new(
            mapping,
            RunId::new(uuid::Uuid::new_v4().to_string()),
            request.content,
        );
        self.run_turn(&mut turn)
            .await
            .map_err(|err| err.to_string())
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
}
