use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use serde_json::json;

use crate::runtime::chat::PermissionDenialRecord;
use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};
use crate::runtime::store::AuthorizedWorkspaceRef;
use crate::runtime::state::TurnState;
use crate::runtime::tools::{
    CapabilityContext, FileOperations, FileStateCache, InterruptBehavior, StorageCapability,
    ToolDispatcher, ToolExecutionContext,
};
use crate::runtime::tools::permission::{PermissionDecision, PermissionReason};

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
    /// Session-scoped cache shared by all read-file tool calls in this engine.
    read_file_state: Arc<FileStateCache>,
    /// Session-scoped accumulated token usage across turns.
    total_usage: Arc<Mutex<TotalTokenUsage>>,
    /// Session-scoped accumulation of permission denials across tool calls.
    permission_denials: Arc<Mutex<Vec<PermissionDenialRecord>>>,
    /// Optional USD budget cap for the current session.
    max_budget_usd: Option<f64>,
    /// Simplified cost estimate rate shared across input/output tokens.
    cost_per_1k_tokens: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct TotalTokenUsage {
    pub tokens_in: u64,
    pub tokens_out: u64,
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
            read_file_state: Arc::new(FileStateCache::new()),
            total_usage: Arc::new(Mutex::new(TotalTokenUsage::default())),
            permission_denials: Arc::new(Mutex::new(Vec::new())),
            max_budget_usd: None,
            cost_per_1k_tokens: None,
        }
    }

    /// Clone static runtime configuration while creating fresh per-session state.
    ///
    /// Session-scoped fields (`authorized_workspace`, `read_file_state`,
    /// `total_usage`) are reset and must be injected by SessionRuntime.
    pub fn clone_with_fresh_session_state(&self) -> Self {
        Self {
            tool_dispatcher: self.tool_dispatcher.clone(),
            workspace_path: self.workspace_path.clone(),
            authorized_workspace: None,
            browser_available: self.browser_available,
            file_ops: self.file_ops.clone(),
            read_file_state: Arc::new(FileStateCache::new()),
            total_usage: Arc::new(Mutex::new(TotalTokenUsage::default())),
            permission_denials: Arc::new(Mutex::new(Vec::new())),
            max_budget_usd: self.max_budget_usd,
            cost_per_1k_tokens: self.cost_per_1k_tokens,
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

    pub fn with_max_budget_usd(mut self, max_budget_usd: f64) -> Self {
        self.max_budget_usd = Some(max_budget_usd);
        self
    }

    pub fn with_cost_per_1k_tokens(mut self, cost_per_1k_tokens: f64) -> Self {
        self.cost_per_1k_tokens = Some(cost_per_1k_tokens);
        self
    }

    pub fn for_test(tool_dispatcher: Arc<ToolDispatcher>) -> Self {
        Self::with_dispatcher(tool_dispatcher)
    }

    pub fn read_file_state(&self) -> Arc<FileStateCache> {
        self.read_file_state.clone()
    }

    pub fn get_total_usage(&self) -> TotalTokenUsage {
        self.total_usage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn get_permission_denials(&self) -> Vec<PermissionDenialRecord> {
        self.permission_denials
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn max_budget_usd(&self) -> Option<f64> {
        self.max_budget_usd
    }

    pub fn tool_interrupt_behavior(&self, tool_name: &str) -> InterruptBehavior {
        let Some(dispatcher) = self.tool_dispatcher.as_ref() else {
            return InterruptBehavior::Block;
        };
        dispatcher
            .tool_interrupt_behavior(tool_name)
            .unwrap_or(InterruptBehavior::Block)
    }

    pub fn accumulate_usage(&self, tokens_in: u64, tokens_out: u64) {
        let mut usage = self
            .total_usage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        usage.tokens_in += tokens_in;
        usage.tokens_out += tokens_out;
    }

    pub fn estimated_cost_usd(&self) -> f64 {
        let Some(rate) = self.cost_per_1k_tokens else {
            return 0.0;
        };
        let usage = self.get_total_usage();
        let total_k_tokens = (usage.tokens_in + usage.tokens_out) as f64 / 1000.0;
        total_k_tokens * rate
    }

    pub fn is_budget_exceeded(&self) -> bool {
        let Some(max) = self.max_budget_usd else {
            return false;
        };
        self.estimated_cost_usd() >= max
    }

    fn record_permission_denial(&self, tool_name: &str, tool_call_id: &str, reason: &str) {
        self.permission_denials
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(PermissionDenialRecord {
                tool_name: tool_name.to_string(),
                tool_call_id: tool_call_id.to_string(),
                reason: reason.to_string(),
            });
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
    /// `run_tool_with_bus` remains available for the legacy/test paths that
    /// supply only a tool name; both methods inject the same capability shape
    /// (including session-scoped `read_file_state`) when workspace capability
    /// is available.
    pub async fn run_tool_call_with_bus(
        &self,
        turn: &TurnState,
        bus: &RuntimeEventBus,
        call: crate::runtime::chat::tool_round_types::RuntimeToolCallRequest,
    ) -> Result<crate::runtime::chat::tool_round_types::RuntimeToolCallOutcome> {
        self.run_tool_call_with_bus_internal(turn, bus, call, None).await
    }

    pub async fn replay_tool_call_with_bus(
        &self,
        turn: &TurnState,
        bus: &RuntimeEventBus,
        mut call: crate::runtime::chat::tool_round_types::RuntimeToolCallRequest,
        updated_input: Option<serde_json::Value>,
    ) -> Result<crate::runtime::chat::tool_round_types::RuntimeToolCallOutcome> {
        if let Some(input) = updated_input {
            call.args = input;
        }
        self.run_tool_call_with_bus_internal(
            turn,
            bus,
            call,
            Some(PermissionDecision::Allow {
                updated_input: None,
                reason: PermissionReason::Other("resolved_pending_permission".into()),
            }),
        )
        .await
    }

    async fn run_tool_call_with_bus_internal(
        &self,
        turn: &TurnState,
        bus: &RuntimeEventBus,
        call: crate::runtime::chat::tool_round_types::RuntimeToolCallRequest,
        permission_override: Option<PermissionDecision>,
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
        let mut ctx = if let Some(workspace_path) = capability_workspace {
            let capability = Arc::new(CapabilityContext {
                storage: Some(StorageCapability {
                    workspace_path,
                    authorized_workspace: self.authorized_workspace.clone(),
                }),
                workspace_id: Some(turn.session_id().as_str().to_string()),
                browser_available: self.browser_available,
                file_ops: self.file_ops.clone(),
                read_file_state: Some(self.read_file_state.clone()),
                file_reading_limits: None,
                notification_sink: None,
                is_subagent: turn.agent_id().is_some(),
            });
            ctx.with_capability(capability)
        } else {
            ctx
        };
        if let Some(permission_override) = permission_override {
            ctx = ctx.with_permission_override(permission_override);
        }

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
                max_result_size_chars,
                context_modifier_message,
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

                Ok(RuntimeToolCallOutcome::Completed {
                    tool_call_id: call.tool_call_id,
                    tool_name: call.tool_name,
                    content: tool_result.content,
                    is_error: false,
                    file_meta: tool_result.file_meta,
                    is_degraded: tool_result.is_degraded,
                    degradation_notice: tool_result.degradation_notice,
                    max_result_size_chars,
                    context_modifier_message,
                })
            }
            Ok(crate::runtime::tools::ToolDispatchOutcome::AskRequired(decision)) => {
                // S6 transition: Ask is now surfaced as a structured AskRequired variant
                // instead of being flattened into a Completed(is_error=true) outcome.
                // TurnDriver/transport can structurally distinguish Ask from Deny/error.
                log::warn!(
                    "run_tool_call_with_bus: tool '{}' returned Ask — \
                     surfacing as AskRequired outcome for pending permission routing. Decision: {:?}",
                    call.tool_name, decision
                );

                Ok(RuntimeToolCallOutcome::AskRequired {
                    tool_call_id: call.tool_call_id.clone(),
                    tool_name: call.tool_name.clone(),
                    original_request: call,
                    decision,
                })
            }
            Err(err) => {
                if let crate::runtime::tools::executor::ToolError::PermissionDenied(ref reason) = err
                {
                    self.record_permission_denial(&call.tool_name, &call.tool_call_id, reason);
                }

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

                let content = match &err {
                    crate::runtime::tools::executor::ToolError::InputValidationError {
                        tool_name,
                        message,
                    } => format!("InputValidationError for tool '{tool_name}': {message}"),
                    other => other.to_string(),
                };

                Ok(RuntimeToolCallOutcome::Completed {
                    tool_call_id: call.tool_call_id,
                    tool_name: call.tool_name,
                    content,
                    is_error: true,
                    file_meta: None,
                    is_degraded: false,
                    degradation_notice: None,
                    max_result_size_chars: 8_000,
                    context_modifier_message: None,
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
                read_file_state: Some(self.read_file_state.clone()),
                file_reading_limits: None,
                notification_sink: None,
                is_subagent: turn.agent_id().is_some(),
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
