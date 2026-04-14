use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::events::{AgentIdleScope, RuntimeEvent, RuntimeEventKind};
use crate::runtime::ids::{AgentId, RunId};
use crate::runtime::query_engine::QueryEngine;
use crate::runtime::state::TurnState;

/// The chat turn request type.  Defined here to avoid circular imports between
/// `session_runtime` and `chat`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatTurnRequest {
    pub conversation_id: String,
    pub content: String,
    pub file_ids: Vec<String>,
    /// The run_id assigned by `SessionRuntime` for this turn.
    /// Callers should use `ChatTurnRequest::new` for ad-hoc creation (generates a
    /// fresh id) or `SessionRuntime::run_chat_request` which overwrites the id
    /// with the single authoritative id generated for this turn.
    pub run_id: RunId,
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
            run_id: RunId::new(uuid::Uuid::new_v4().to_string()),
        }
    }
}

/// Trait implemented by legacy turn executors.
///
/// Retained as a runtime-controlled helper during the chat-runtime-first migration.
/// The runtime (via `RuntimeChatTurnDriver`) remains the single turn entry; the
/// executor is invoked by the driver — not by `SessionRuntime` directly — to
/// preserve current LLM / transport behaviour while the migration is in progress.
/// Will be removed once Tasks 3/4 complete the migration.
#[async_trait]
pub trait RuntimeTurnExecutor: Send + Sync {
    async fn run_chat_turn(
        &self,
        request: ChatTurnRequest,
    ) -> std::result::Result<(), String>;
}

/// Runtime-owned chat turn driver.
///
/// Single entry point for chat turn orchestration.  There are two execution modes:
///
/// **No-executor mode** (pure runtime path, used in tests and future production):
///   `run_chat_turn` emits `StreamStarted`, then calls `QueryEngine::run()` which
///   emits `StreamDelta → MessagePersisted → StreamDone` on the bus.  The
///   `TauriEventAdapter` translates these to the expected frontend legacy events.
///
/// **Executor-backed mode** (current production, until Tasks 3/4 complete):
///   `run_chat_turn` emits `StreamStarted` on the bus, invokes the legacy executor
///   as a runtime-controlled helper (it performs the real LLM / tool loop), then
///   emits `MessagePersisted`, `StreamDone`, and `AgentIdle` through the bus so
///   `TauriEventAdapter` can deliver the corresponding legacy frontend events
///   (`message:updated`, `streaming:done`, `agent:idle`).
///
/// This design satisfies three constraints simultaneously:
///   1. `SessionRuntime` never full-delegates; the driver is the only turn owner.
///   2. `MessagePersisted` and `StreamDone` appear in `recorded_events()`.
///   3. `TauriEventAdapter` is notified so frontend receives `streaming:done`,
///      `message:updated`, and `agent:idle`.  `AgentGuard::clear()` and `Drop` must
///      NOT re-emit these events to avoid duplicates.
#[derive(Clone)]
pub struct RuntimeChatTurnDriver {
    query_engine: QueryEngine,
    event_bus: RuntimeEventBus,
    /// Legacy executor helper, present on production paths during migration.
    legacy_executor: Option<Arc<dyn RuntimeTurnExecutor>>,
}

impl RuntimeChatTurnDriver {
    pub fn new(query_engine: QueryEngine, event_bus: RuntimeEventBus) -> Self {
        Self {
            query_engine,
            event_bus,
            legacy_executor: None,
        }
    }

    pub fn with_legacy_executor(
        query_engine: QueryEngine,
        event_bus: RuntimeEventBus,
        legacy_executor: Arc<dyn RuntimeTurnExecutor>,
    ) -> Self {
        Self {
            query_engine,
            event_bus,
            legacy_executor: Some(legacy_executor),
        }
    }

    pub async fn run_chat_turn(
        &self,
        turn: &mut TurnState,
        request: &ChatTurnRequest,
    ) -> Result<()> {
        // Emit StreamStarted on the bus in all cases.  TauriEventAdapter does not
        // map StreamStarted to a legacy event, so this is always safe.
        self.event_bus
            .emit(RuntimeEvent::new(
                turn.session_id().clone(),
                turn.run_id().clone(),
                RuntimeEventKind::StreamStarted,
            ))
            .await?;

        if let Some(executor) = &self.legacy_executor {
            // Executor-backed mode: the legacy helper owns real LLM / tool work
            // and fires frontend legacy events directly via the Tauri app handle.
            // We invoke it as a runtime-controlled helper (not a full delegate).
            executor
                .run_chat_turn(request.clone())
                .await
                .map_err(|e| anyhow::anyhow!(e))?;

            // Emit runtime events through the bus so TauriEventAdapter can
            // deliver the corresponding legacy frontend events (message:updated,
            // streaming:done, agent:idle).
            let session_id = turn.session_id().clone();
            let run_id = turn.run_id().clone();
            self.event_bus
                .emit(RuntimeEvent::message_persisted(
                    session_id.clone(),
                    run_id.clone(),
                    format!("exec-msg-{}", run_id.as_str()),
                    "assistant",
                    serde_json::json!({"executor_owned": true}),
                ))
                .await?;
            self.event_bus
                .emit(RuntimeEvent::stream_done(session_id.clone(), run_id.clone()))
                .await?;
            self.event_bus
                .emit(RuntimeEvent::new(
                    session_id,
                    run_id.clone(),
                    RuntimeEventKind::AgentIdle {
                        agent_id: AgentId::new(format!("agent-{}", run_id.as_str())),
                        scope: AgentIdleScope::Primary,
                    },
                ))
                .await?;
        } else {
            // Pure runtime mode: QueryEngine drives the full turn and emits
            // StreamDelta → MessagePersisted → StreamDone through the bus.
            // TauriEventAdapter translates these to the expected frontend events.
            self.query_engine.run(turn, &self.event_bus).await?;
        }

        Ok(())
    }
}
