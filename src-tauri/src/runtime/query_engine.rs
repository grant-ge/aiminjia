use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::json;

use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};
use crate::runtime::state::TurnState;
use crate::runtime::tools::{CapabilityContext, StorageCapability, ToolDispatcher, ToolExecutionContext};

#[derive(Clone, Default)]
pub struct QueryEngine {
    tool_dispatcher: Option<Arc<ToolDispatcher>>,
    /// Workspace path injected at construction time so that workspace-scoped
    /// runtime tools receive a `CapabilityContext` when executing via this engine.
    /// `None` in test/legacy paths that do not need capability context.
    workspace_path: Option<PathBuf>,
}

impl QueryEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_dispatcher(tool_dispatcher: Arc<ToolDispatcher>) -> Self {
        Self {
            tool_dispatcher: Some(tool_dispatcher),
            workspace_path: None,
        }
    }

    /// Attach a workspace path so that workspace-scoped tools executed through
    /// this engine receive a properly populated `CapabilityContext`.
    pub fn with_workspace_path(mut self, workspace_path: PathBuf) -> Self {
        self.workspace_path = Some(workspace_path);
        self
    }

    pub fn for_test(tool_dispatcher: Arc<ToolDispatcher>) -> Self {
        Self::with_dispatcher(tool_dispatcher)
    }

    pub async fn run(&self, turn: &mut TurnState, bus: &RuntimeEventBus) -> Result<()> {
        if turn.cancellation().is_cancelled() {
            bus.emit(RuntimeEvent::new(
                turn.session_id().clone(),
                turn.run_id().clone(),
                RuntimeEventKind::RunCancelled,
            ))
            .await?;
            return Err(anyhow!("turn already cancelled"));
        }

        let content = format!("runtime:{}", turn.user_input());
        turn.append_output(&content);
        bus.emit(RuntimeEvent::stream_delta(
            turn.session_id().clone(),
            turn.run_id().clone(),
            content.clone(),
        ))
        .await?;
        bus.emit(RuntimeEvent::message_persisted(
            turn.session_id().clone(),
            turn.run_id().clone(),
            format!("msg-{}", turn.run_id().as_str()),
            "assistant",
            json!({"text": content}),
        ))
        .await?;
        bus.emit(RuntimeEvent::stream_done(
            turn.session_id().clone(),
            turn.run_id().clone(),
        ))
        .await?;
        Ok(())
    }

    pub async fn run_single_tool_turn(
        &self,
        conversation_id: &str,
        run_id: &str,
        tool_name: &str,
    ) -> Result<Vec<String>> {
        let dispatcher = self
            .tool_dispatcher
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("tool dispatcher not configured"))?;
        let ctx = ToolExecutionContext::for_test(
            conversation_id,
            run_id,
            format!("tool-call-{tool_name}"),
        );
        let mut result = dispatcher
            .dispatch(tool_name, json!({"tool": tool_name}), ctx)
            .await?;
        result.event_names.push("streaming:done".to_string());
        Ok(result.event_names)
    }

    pub async fn run_tool_with_bus(
        &self,
        turn: &TurnState,
        bus: &RuntimeEventBus,
        tool_name: &str,
    ) -> Result<()> {
        let dispatcher = self
            .tool_dispatcher
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("tool dispatcher not configured"))?;
        let ctx = ToolExecutionContext::new(
            turn.session_id().clone(),
            turn.run_id().clone(),
            turn.agent_id().cloned(),
            format!("tool-call-{tool_name}"),
            turn.cancellation(),
        );
        // Inject capability context when workspace_path is available so that
        // workspace-scoped runtime tools (list_directory, read_workspace_file, etc.)
        // can resolve their root path correctly.  When no workspace_path is set
        // (legacy/test paths), capability remains None and tools that require it
        // will return PermissionDenied as expected.
        let ctx = if let Some(ref wp) = self.workspace_path {
            let capability = Arc::new(CapabilityContext {
                storage: Some(StorageCapability {
                    workspace_path: wp.clone(),
                    authorized_workspace: None,
                }),
                workspace_id: Some(turn.session_id().as_str().to_string()),
                browser_available: false,
            });
            ctx.with_capability(capability)
        } else {
            ctx
        };
        let outcome = dispatcher
            .dispatch(tool_name, json!({"tool": tool_name}), ctx)
            .await?;
        for event_name in outcome.event_names {
            match event_name.as_str() {
                "tool:executing" => {
                    bus.emit(RuntimeEvent::new(
                        turn.session_id().clone(),
                        turn.run_id().clone(),
                        RuntimeEventKind::ToolCallExecuting {
                            tool_call_id: crate::runtime::ids::ToolCallId::new(format!(
                                "tool-call-{tool_name}"
                            )),
                            tool_name: tool_name.to_string(),
                        },
                    ))
                    .await?;
                }
                "tool:completed" => {
                    bus.emit(RuntimeEvent::new(
                        turn.session_id().clone(),
                        turn.run_id().clone(),
                        RuntimeEventKind::ToolCallCompleted {
                            tool_call_id: crate::runtime::ids::ToolCallId::new(format!(
                                "tool-call-{tool_name}"
                            )),
                            tool_name: tool_name.to_string(),
                        },
                    ))
                    .await?;
                }
                _ => {}
            }
        }
        bus.emit(RuntimeEvent::stream_done(
            turn.session_id().clone(),
            turn.run_id().clone(),
        ))
        .await?;
        Ok(())
    }
}
