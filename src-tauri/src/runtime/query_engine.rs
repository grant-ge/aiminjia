use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::json;

use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};
use crate::runtime::store::AuthorizedWorkspaceRef;
use crate::runtime::state::TurnState;
use crate::runtime::tools::{CapabilityContext, FileOperations, StorageCapability, ToolDispatcher, ToolExecutionContext};

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
    /// Whether a browser connector is available for this session.
    /// Injected from `connector_engine.is_some()` on the production path so that
    /// browser-scope tools pass `CapabilityPermissionPipeline` checks.
    browser_available: bool,
    /// File operations accessor injected from the transport layer.
    /// When present, `load_file` runtime tool uses this to load files
    /// instead of bridging through `PluginContext`.
    file_ops: Option<Arc<dyn FileOperations>>,
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
            browser_available: false,
            file_ops: None,
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

    /// Set whether a browser connector is available for this session.
    ///
    /// When `true`, browser-scope tools pass the `CapabilityPermissionPipeline`
    /// check.  On the production path, pass `connector_engine.is_some()`.
    pub fn with_browser_available(mut self, browser_available: bool) -> Self {
        self.browser_available = browser_available;
        self
    }

    /// Attach a file operations accessor so that `load_file` runtime tool can
    /// operate through `CapabilityContext.file_ops` without a `PluginContext`.
    pub fn with_file_ops(mut self, file_ops: Arc<dyn FileOperations>) -> Self {
        self.file_ops = Some(file_ops);
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
        let outcome = dispatcher
            .dispatch(tool_name, json!({"tool": tool_name}), ctx)
            .await?;
        let mut event_names = match outcome {
            crate::runtime::tools::ToolDispatchOutcome::Completed { event_names, .. } => event_names,
            crate::runtime::tools::ToolDispatchOutcome::AskRequired(decision) => {
                // FIXME(S6): extend return type to carry Ask up to TurnDriver/transport.
                // S1 transition: Ask→error at QueryEngine boundary (not at Dispatcher).
                log::warn!(
                    "run_single_tool_turn: tool '{}' returned Ask — error fallback (S1). Decision: {:?}",
                    tool_name, decision
                );
                return Err(anyhow::anyhow!(
                    "Tool '{}' requires user confirmation before it can run.",
                    tool_name
                ));
            }
        };
        event_names.push("streaming:done".to_string());
        Ok(event_names)
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
        // TurnState centralizes tool-call scoped cancellation so each call gets
        // a child token of the turn token.
        let ctx = turn.build_execution_context(call.tool_call_id.clone());

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
                browser_available: self.browser_available,
                file_ops: self.file_ops.clone(),
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
            Ok(crate::runtime::tools::ToolDispatchOutcome::Completed {
                result: tool_result,
                ..
            }) => {
                bus.emit(RuntimeEvent::new(
                    turn.session_id().clone(),
                    turn.run_id().clone(),
                    RuntimeEventKind::ToolCallCompleted {
                        tool_call_id: crate::runtime::ids::ToolCallId::new(
                            call.tool_call_id.clone(),
                        ),
                        tool_name: call.tool_name.clone(),
                        is_error: false,
                    },
                ))
                .await?;

                Ok(RuntimeToolCallOutcome {
                    tool_call_id: call.tool_call_id,
                    tool_name: call.tool_name,
                    content: tool_result.content,
                    is_error: false,
                    file_meta: tool_result.file_meta,
                    is_degraded: tool_result.is_degraded,
                    degradation_notice: tool_result.degradation_notice,
                })
            }
            Ok(crate::runtime::tools::ToolDispatchOutcome::AskRequired(decision)) => {
                // S1 transition: Ask reaches QueryEngine as a structured decision,
                // NOT collapsed into Err at the Dispatcher level.
                //
                // Current limitation (S1): we convert Ask → error outcome here because
                // RuntimeToolCallOutcome is still a flat struct without an Ask variant.
                // The PermissionDecision is preserved in the log for debugging.
                //
                // FIXME(S6): extend RuntimeToolCallOutcome to enum with AskRequired variant,
                // then TurnDriver/transport can emit RuntimeEvent::PermissionAsk to the UI
                // and await user response. At that point, remove this conversion.
                log::warn!(
                    "run_tool_call_with_bus: tool '{}' returned Ask — \
                     converting to error outcome (S1 transition). Decision: {:?}",
                    call.tool_name, decision
                );
                bus.emit(RuntimeEvent::new(
                    turn.session_id().clone(),
                    turn.run_id().clone(),
                    RuntimeEventKind::ToolCallCompleted {
                        tool_call_id: crate::runtime::ids::ToolCallId::new(
                            call.tool_call_id.clone(),
                        ),
                        tool_name: call.tool_name.clone(),
                        is_error: true,
                    },
                ))
                .await?;

                Ok(RuntimeToolCallOutcome {
                    tool_call_id: call.tool_call_id,
                    tool_name: call.tool_name,
                    content: "Tool requires user confirmation before it can run.".to_string(),
                    is_error: true,
                    file_meta: None,
                    is_degraded: false,
                    degradation_notice: None,
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
                        is_error: true,
                    },
                ))
                .await?;

                Ok(RuntimeToolCallOutcome {
                    tool_call_id: call.tool_call_id,
                    tool_name: call.tool_name,
                    content: err.to_string(),
                    is_error: true,
                    file_meta: None,
                    is_degraded: false,
                    degradation_notice: None,
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
        let ctx = turn.build_execution_context(format!("tool-call-{tool_name}"));
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
                browser_available: self.browser_available,
                file_ops: self.file_ops.clone(),
            });
            ctx.with_capability(capability)
        } else {
            ctx
        };
        let outcome = dispatcher
            .dispatch(tool_name, json!({"tool": tool_name}), ctx)
            .await?;
        let event_names = match outcome {
            crate::runtime::tools::ToolDispatchOutcome::Completed { event_names, .. } => event_names,
            crate::runtime::tools::ToolDispatchOutcome::AskRequired(decision) => {
                // FIXME(S6): extend return type to carry Ask up to TurnDriver/transport.
                // S1 transition: Ask→error at QueryEngine boundary (not at Dispatcher).
                log::warn!(
                    "run_tool_with_bus: tool '{}' returned Ask — error fallback (S1). Decision: {:?}",
                    tool_name, decision
                );
                return Err(anyhow::anyhow!(
                    "Tool '{}' requires user confirmation before it can run.",
                    tool_name
                ));
            }
        };
        for event_name in event_names {
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
                            // Legacy run_tool_with_bus path: no error info available,
                            // default to success=true to preserve prior behaviour.
                            is_error: false,
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
