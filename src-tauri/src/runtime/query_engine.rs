use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::json;

use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};
use crate::runtime::store::AuthorizedWorkspaceRef;
use crate::runtime::state::TurnState;
use crate::runtime::tools::{CapabilityContext, StorageCapability, ToolDispatcher, ToolExecutionContext};

#[derive(Clone, Default)]
pub struct QueryEngine {
    tool_dispatcher: Option<Arc<ToolDispatcher>>,
    /// Workspace path injected at construction time so that workspace-scoped
    /// runtime tools receive a `CapabilityContext` when executing via this engine.
    /// `None` in test/legacy paths that do not need capability context.
    workspace_path: Option<PathBuf>,
    /// Session-scoped authorized workspace injected before a turn runs.
    /// When present, runtime tools resolve against this path first.
    authorized_workspace: Option<AuthorizedWorkspaceRef>,
}

impl QueryEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_dispatcher(tool_dispatcher: Arc<ToolDispatcher>) -> Self {
        Self {
            tool_dispatcher: Some(tool_dispatcher),
            workspace_path: None,
            authorized_workspace: None,
        }
    }

    /// Attach a workspace path so that workspace-scoped tools executed through
    /// this engine receive a properly populated `CapabilityContext`.
    pub fn with_workspace_path(mut self, workspace_path: PathBuf) -> Self {
        self.workspace_path = Some(workspace_path);
        self
    }

    /// Attach the session-authorized workspace so runtime tools can access it
    /// through `CapabilityContext.storage.authorized_workspace`.
    pub fn with_authorized_workspace(
        mut self,
        authorized_workspace: Option<AuthorizedWorkspaceRef>,
    ) -> Self {
        self.authorized_workspace = authorized_workspace;
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

        if turn.executor_backed() {
            bus.emit(RuntimeEvent::new(
                turn.session_id().clone(),
                turn.run_id().clone(),
                RuntimeEventKind::StreamStarted,
            ))
            .await?;
            return Ok(());
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

    /// Execute a single LLM-issued tool call and report progress through the event bus.
    ///
    /// This is the production-grade successor to `run_tool_with_bus`.  Unlike
    /// the older method it accepts a full [`RuntimeToolCallRequest`] (with a real
    /// `tool_call_id` and the actual argument payload from the LLM) and returns a
    /// typed [`RuntimeToolCallOutcome`] that the caller can embed in the tool-
    /// result message sent back to the LLM.
    ///
    /// The method:
    /// 1. Builds a `ToolExecutionContext` using the call's `tool_call_id` so that
    ///    all events carry a stable, LLM-issued identifier.
    /// 2. Injects a `CapabilityContext` with `workspace_path` /
    ///    `authorized_workspace` when available (Workspace-First guarantee).
    /// 3. Emits `ToolCallExecuting` / `ToolCallCompleted` runtime events through
    ///    the bus using the real `tool_call_id`.
    /// 4. Returns a [`RuntimeToolCallOutcome`] indicating success or failure
    ///    without surfacing internal errors as transport-layer panics.
    ///
    /// Existing `run_tool_with_bus` is **not modified** and remains available for
    /// the legacy/test paths that supply only a tool name.
    pub async fn run_tool_call_with_bus(
        &self,
        turn: &TurnState,
        bus: &RuntimeEventBus,
        call: crate::runtime::chat::tool_round_types::RuntimeToolCallRequest,
    ) -> Result<crate::runtime::chat::tool_round_types::RuntimeToolCallOutcome> {
        use crate::runtime::chat::tool_round_types::RuntimeToolCallOutcome;

        let dispatcher = self
            .tool_dispatcher
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("tool dispatcher not configured"))?;

        // Build execution context with the real tool_call_id from the LLM.
        let ctx = ToolExecutionContext::new(
            turn.session_id().clone(),
            turn.run_id().clone(),
            turn.agent_id().cloned(),
            call.tool_call_id.clone(),
            turn.cancellation(),
        );

        // Inject capability context (Workspace-First guarantee) — same logic as
        // `run_tool_with_bus` so workspace-scoped tools receive the correct root.
        let capability_workspace = self
            .workspace_path
            .clone()
            .or_else(|| self.authorized_workspace.as_ref().map(|aw| aw.root_path.clone()));
        let ctx = if let Some(workspace_path) = capability_workspace {
            let capability = Arc::new(CapabilityContext {
                storage: Some(StorageCapability {
                    workspace_path,
                    authorized_workspace: self.authorized_workspace.clone(),
                }),
                workspace_id: Some(turn.session_id().as_str().to_string()),
                browser_available: false,
            });
            ctx.with_capability(capability)
        } else {
            ctx
        };

        // Emit ToolCallExecuting before dispatching so the UI knows the tool
        // has started before any latency from the actual execution.
        bus.emit(RuntimeEvent::new(
            turn.session_id().clone(),
            turn.run_id().clone(),
            RuntimeEventKind::ToolCallExecuting {
                tool_call_id: crate::runtime::ids::ToolCallId::new(call.tool_call_id.clone()),
                tool_name: call.tool_name.clone(),
            },
        ))
        .await?;

        // Dispatch using the real args from the LLM (not a synthetic placeholder).
        let dispatch_result = dispatcher
            .dispatch(&call.tool_name, call.args.clone(), ctx)
            .await;

        match dispatch_result {
            Ok(outcome) => {
                bus.emit(RuntimeEvent::new(
                    turn.session_id().clone(),
                    turn.run_id().clone(),
                    RuntimeEventKind::ToolCallCompleted {
                        tool_call_id: crate::runtime::ids::ToolCallId::new(
                            call.tool_call_id.clone(),
                        ),
                        tool_name: call.tool_name.clone(),
                    },
                ))
                .await?;

                Ok(RuntimeToolCallOutcome {
                    tool_call_id: call.tool_call_id,
                    tool_name: call.tool_name,
                    content: outcome.result.content,
                    is_error: false,
                })
            }
            Err(err) => {
                bus.emit(RuntimeEvent::new(
                    turn.session_id().clone(),
                    turn.run_id().clone(),
                    RuntimeEventKind::ToolCallCompleted {
                        tool_call_id: crate::runtime::ids::ToolCallId::new(
                            call.tool_call_id.clone(),
                        ),
                        tool_name: call.tool_name.clone(),
                    },
                ))
                .await?;

                Ok(RuntimeToolCallOutcome {
                    tool_call_id: call.tool_call_id,
                    tool_name: call.tool_name,
                    content: err.to_string(),
                    is_error: true,
                })
            }
        }
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
        let capability_workspace = self
            .workspace_path
            .clone()
            .or_else(|| self.authorized_workspace.as_ref().map(|aw| aw.root_path.clone()));
        let ctx = if let Some(workspace_path) = capability_workspace {
            let capability = Arc::new(CapabilityContext {
                storage: Some(StorageCapability {
                    workspace_path,
                    authorized_workspace: self.authorized_workspace.clone(),
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
