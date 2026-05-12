use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use serde_json::json;

use crate::runtime::chat::PermissionDenialRecord;
use crate::runtime::dependencies::ManagedRuntimeResolver;
use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};
use crate::runtime::path_auth::{RuleSource, ToolPermissionContext};
use crate::runtime::state::TurnState;
use crate::runtime::store::AuthorizedWorkspaceRef;
use crate::runtime::tools::permission::{PermissionDecision, PermissionReason};
use crate::runtime::tools::{
    CapabilityContext, FileOperations, FileStateCache, InterruptBehavior, StorageCapability,
    ToolDispatcher, ToolExecutionContext,
};

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
    /// File operations accessor injected from the transport layer (stub, retained for compatibility).
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
    /// Managed Node/Python runtime resolver propagated into capability contexts.
    runtime_resolver: Option<ManagedRuntimeResolver>,
    /// Base permission context loaded from PermissionStore at session setup
    /// (UserSettings working dirs + allow_rules).  Per-turn attachment dirs are
    /// merged in at capability-build time via `session_attachment_dirs`.
    /// `None` in test/legacy paths → capability gets `ToolPermissionContext::empty()`.
    base_permission_ctx: Option<Arc<ToolPermissionContext>>,
    /// Optional reference to the PermissionStore so `build_turn_permission_ctx`
    /// can re-load `additional_working_dirs` / `allow_rules` on every turn —
    /// crucial for ack-driven grants taking effect within the same turn.
    /// When None, falls back to the (stale) `base_permission_ctx` snapshot.
    permission_store: Option<Arc<crate::runtime::store::PermissionStore>>,
    /// Session-scoped accumulation of working dirs derived from attachments across
    /// all turns (source = Session).  Grows monotonically within a session;
    /// never evicted until the session engine is dropped.
    /// Wrapped in `Arc<Mutex<...>>` so the field survives the value-clone performed
    /// by `with_authorized_workspace` / `with_permission_ctx` builder calls.
    session_attachment_dirs: Arc<Mutex<HashMap<PathBuf, RuleSource>>>,
    /// LTR registries injected by SessionRuntime — propagated into every
    /// ToolExecutionContext this engine builds.  `None` in legacy/test paths.
    team_registry: Option<Arc<crate::runtime::agent::TeamRegistry>>,
    agent_names: Option<Arc<crate::runtime::agent::AgentNameRegistry>>,
    inbox_registry: Option<Arc<crate::runtime::agent::InboxRegistry>>,
    lead_idle: Option<Arc<crate::runtime::agent::LeadIdleSupervisor>>,
    cancellation_registry: Option<Arc<crate::runtime::agent::CancellationRegistry>>,
    /// LTR (B-gap2): per-conversation directory rooted at
    /// `<aijia_home>/users/{scope}/conversations/{conv_id}`.  Propagated into
    /// every ToolExecutionContext this engine builds; spawn_subagent reads it
    /// and forwards it into the child worker so transcript JSONL +
    /// `.meta.json` + team_context attachments land on disk.
    conv_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct TotalTokenUsage {
    pub tokens_in: u64,
    pub tokens_out: u64,
    /// Anthropic-style accumulated prompt-cache write tokens.
    pub cache_creation_input_tokens: u64,
    /// Anthropic-style accumulated prompt-cache read tokens.
    pub cache_read_input_tokens: u64,
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
            file_ops: None,
            read_file_state: Arc::new(FileStateCache::new()),
            total_usage: Arc::new(Mutex::new(TotalTokenUsage::default())),
            permission_denials: Arc::new(Mutex::new(Vec::new())),
            max_budget_usd: None,
            cost_per_1k_tokens: None,
            runtime_resolver: None,
            base_permission_ctx: None,
            permission_store: None,
            session_attachment_dirs: Arc::new(Mutex::new(HashMap::new())),
            team_registry: None,
            agent_names: None,
            inbox_registry: None,
            lead_idle: None,
            cancellation_registry: None,
            conv_dir: None,
        }
    }

    /// Clone static runtime configuration while creating fresh per-session state.
    ///
    /// Session-scoped fields (`authorized_workspace`, `read_file_state`,
    /// `total_usage`, `session_attachment_dirs`) are reset and must be injected
    /// by SessionRuntime.  Static configuration fields (`tool_dispatcher`,
    /// `workspace_path`, `file_ops`, `max_budget_usd`,
    /// `cost_per_1k_tokens`, `runtime_resolver`, `base_permission_ctx`) are
    /// preserved from the base engine.
    pub fn clone_with_fresh_session_state(&self) -> Self {
        Self {
            tool_dispatcher: self.tool_dispatcher.clone(),
            workspace_path: self.workspace_path.clone(),
            authorized_workspace: None,
            file_ops: self.file_ops.clone(),
            read_file_state: Arc::new(FileStateCache::new()),
            total_usage: Arc::new(Mutex::new(TotalTokenUsage::default())),
            permission_denials: Arc::new(Mutex::new(Vec::new())),
            max_budget_usd: self.max_budget_usd,
            cost_per_1k_tokens: self.cost_per_1k_tokens,
            runtime_resolver: self.runtime_resolver.clone(),
            base_permission_ctx: self.base_permission_ctx.clone(),
            permission_store: self.permission_store.clone(),
            session_attachment_dirs: Arc::new(Mutex::new(HashMap::new())),
            team_registry: self.team_registry.clone(),
            agent_names: self.agent_names.clone(),
            inbox_registry: self.inbox_registry.clone(),
            lead_idle: self.lead_idle.clone(),
            cancellation_registry: self.cancellation_registry.clone(),
            conv_dir: self.conv_dir.clone(),
        }
    }

    /// Attach a workspace path so that workspace-scoped tools executed through
    /// this engine receive a properly populated `CapabilityContext`.
    pub fn with_workspace_path(mut self, workspace_path: PathBuf) -> Self {
        self.workspace_path = Some(workspace_path);
        self
    }

    /// LTR (P1.7/P2.2): inject the per-process Team / name / inbox registries
    /// so that every ToolExecutionContext built by this engine carries them.
    /// Without these, TeamCreate / SendMessage panic with the
    /// "registry not injected" message.
    pub fn with_ltr_registries(
        mut self,
        team: Arc<crate::runtime::agent::TeamRegistry>,
        names: Arc<crate::runtime::agent::AgentNameRegistry>,
        inboxes: Arc<crate::runtime::agent::InboxRegistry>,
    ) -> Self {
        self.team_registry = Some(team);
        self.agent_names = Some(names);
        self.inbox_registry = Some(inboxes);
        self
    }

    /// LTR (B-gap1) test convenience: inject just the AgentNameRegistry, for
    /// tests that exercise Path A wiring without needing a Team or Inbox.
    pub fn with_agent_names(
        mut self,
        names: Arc<crate::runtime::agent::AgentNameRegistry>,
    ) -> Self {
        self.agent_names = Some(names);
        self
    }

    pub fn with_lead_idle(
        mut self,
        sup: Arc<crate::runtime::agent::LeadIdleSupervisor>,
    ) -> Self {
        self.lead_idle = Some(sup);
        self
    }

    pub fn with_cancellation_registry(
        mut self,
        reg: Arc<crate::runtime::agent::CancellationRegistry>,
    ) -> Self {
        self.cancellation_registry = Some(reg);
        self
    }

    /// LTR (B-gap2): attach the per-conversation directory.  See `conv_dir`.
    pub fn with_conv_dir(mut self, dir: PathBuf) -> Self {
        self.conv_dir = Some(dir);
        self
    }

    /// LTR (B-gap1): accessors for chat_turn_driver to wire Path A
    /// (mark_running on entry, mark_idle before AgentIdle).
    pub fn lead_idle_supervisor(&self) -> Option<&Arc<crate::runtime::agent::LeadIdleSupervisor>> {
        self.lead_idle.as_ref()
    }
    pub fn agent_names(&self) -> Option<&Arc<crate::runtime::agent::AgentNameRegistry>> {
        self.agent_names.as_ref()
    }

    /// Attach LTR registries (Team / name / inbox) onto an already-built
    /// ToolExecutionContext.  Helper to avoid duplicating the wiring in every
    /// tool-call build site.  No-op for whichever registry isn't configured
    /// — tools defensively check for `None` and error out themselves.
    fn attach_ltr_registries(
        &self,
        mut ctx: crate::runtime::tools::context::ToolExecutionContext,
    ) -> crate::runtime::tools::context::ToolExecutionContext {
        if let Some(team) = self.team_registry.clone() {
            ctx = ctx.with_team_registry(team);
        }
        if let Some(names) = self.agent_names.clone() {
            ctx = ctx.with_agent_names(names);
        }
        if let Some(inbox) = self.inbox_registry.clone() {
            ctx = ctx.with_inbox_registry(inbox);
        }
        if let Some(sup) = self.lead_idle.clone() {
            ctx = ctx.with_lead_idle(sup);
        }
        if let Some(reg) = self.cancellation_registry.clone() {
            ctx = ctx.with_cancellation_registry(reg);
        }
        if let Some(dir) = self.conv_dir.clone() {
            ctx = ctx.with_conv_dir(dir);
        }
        ctx
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

    /// Attach a file operations accessor (stub, retained for compile compatibility).
    pub fn with_file_ops(mut self, file_ops: Arc<dyn FileOperations>) -> Self {
        self.file_ops = Some(file_ops);
        self
    }

    pub fn with_runtime_resolver(
        mut self,
        runtime_resolver: Option<ManagedRuntimeResolver>,
    ) -> Self {
        self.runtime_resolver = runtime_resolver;
        self
    }

    pub fn with_read_file_state(mut self, read_file_state: Arc<FileStateCache>) -> Self {
        self.read_file_state = read_file_state;
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

    /// Inject a base `ToolPermissionContext` loaded from the persistent store
    /// (UserSettings working dirs + allow_rules).  Called by `SessionRuntime`
    /// after loading from `PermissionStore`.  Per-turn session attachment dirs are
    /// accumulated separately in `session_attachment_dirs`.
    pub fn with_permission_ctx(mut self, ctx: Arc<ToolPermissionContext>) -> Self {
        self.base_permission_ctx = Some(ctx);
        self
    }

    /// Inject the PermissionStore so `build_turn_permission_ctx` can re-load
    /// `additional_working_dirs` / `allow_rules` on every turn.  Required for
    /// ack-driven grants ("永久允许") to take effect within the same turn —
    /// otherwise the replay reads a stale `base_permission_ctx` snapshot.
    pub fn with_permission_store(
        mut self,
        store: Arc<crate::runtime::store::PermissionStore>,
    ) -> Self {
        self.permission_store = Some(store);
        self
    }

    /// Merge per-turn attachment-derived directories (source = Session) into the
    /// session-scoped accumulator.  Called by `RuntimeChatTurnDriver` at the start
    /// of each turn so that directories introduced by this turn remain available
    /// for all subsequent tool calls in the session.
    pub fn merge_session_attachment_dirs(&self, dirs: &[std::path::PathBuf]) {
        let mut acc = self
            .session_attachment_dirs
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        for dir in dirs {
            acc.insert(dir.clone(), RuleSource::Session);
        }
    }

    /// Build the per-turn `ToolPermissionContext` by combining:
    /// 1. Clone the base context (UserSettings working_dirs + allow_rules from
    ///    PermissionStore).
    /// 2. Set primary_root from authorized_workspace.root_path else workspace_path.
    /// 3. Set mode from turn.permission_mode().
    /// 4. Merge session_attachment_dirs as RuleSource::Session, preferring
    ///    pre-existing UserSettings source on duplicate paths.
    pub(crate) fn build_turn_permission_ctx(&self, turn: &TurnState) -> Arc<ToolPermissionContext> {
        // Always reload base from PermissionStore when available — this ensures
        // user "永久允许" grants take effect within the same turn (e.g. on the
        // replay after Ask resolution), not just on the next turn boundary.
        let base = if let Some(store) = self.permission_store.as_ref() {
            let entries = crate::runtime::path_auth::store_bridge::load_path_auth_entries(store);
            let mut ctx = ToolPermissionContext::empty();
            ctx.additional_working_dirs = entries.working_dirs;
            ctx.allow_rules = entries.allow_rules;
            ctx
        } else {
            self.base_permission_ctx
                .as_ref()
                .map(|ctx| (**ctx).clone())
                .unwrap_or_else(ToolPermissionContext::empty)
        };

        let mut ctx = base;

        // Set primary_root: authorized_workspace takes precedence over workspace_path.
        ctx.primary_root = self
            .authorized_workspace
            .as_ref()
            .map(|aw| aw.root_path.clone())
            .or_else(|| self.workspace_path.clone());

        // Propagate the turn's permission mode.
        ctx.mode = turn.permission_mode();

        // Merge session-accumulated attachment dirs (source = Session).
        {
            let acc = self
                .session_attachment_dirs
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            // Why: when the same path is both a persistent UserSettings entry and a
            // transient Session attachment, prefer the durable source so future UI /
            // telemetry sees the correct provenance.
            for (dir, source) in acc.iter() {
                ctx.additional_working_dirs
                    .entry(dir.clone())
                    .or_insert_with(|| source.clone());
            }
        }

        log::info!(
            "[build_turn_permission_ctx] mode={:?} primary_root={:?} additional_working_dirs={:?} allow_rules_count={} deny_rules_count={}",
            ctx.mode,
            ctx.primary_root,
            ctx.additional_working_dirs.keys().collect::<Vec<_>>(),
            ctx.allow_rules.len(),
            ctx.deny_rules.len(),
        );

        Arc::new(ctx)
    }

    /// Test-only accessor exposing the merge logic of `build_turn_permission_ctx`
    /// to integration tests in `tests/`.  Production code should call the
    /// `pub(crate)` version directly; this wrapper exists only so that the
    /// separate test binary can cross the crate boundary.
    pub fn build_turn_permission_ctx_for_test(
        &self,
        turn: &TurnState,
    ) -> Arc<ToolPermissionContext> {
        self.build_turn_permission_ctx(turn)
    }

    pub fn for_test(tool_dispatcher: Arc<ToolDispatcher>) -> Self {
        Self::with_dispatcher(tool_dispatcher)
    }

    pub fn read_file_state(&self) -> Arc<FileStateCache> {
        self.read_file_state.clone()
    }

    #[cfg(test)]
    pub fn authorized_workspace_for_test(&self) -> Option<AuthorizedWorkspaceRef> {
        self.authorized_workspace.clone()
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

    /// Returns whether the named tool is concurrency-safe for the given input.
    /// Defaults to `false` (conservative — serial execution) if tool is not registered
    /// or no dispatcher is configured.
    pub fn tool_concurrency_safe(&self, tool_name: &str, input: &serde_json::Value) -> bool {
        let Some(dispatcher) = self.tool_dispatcher.as_ref() else {
            return false;
        };
        dispatcher
            .tool_concurrency_safe(tool_name, input)
            .unwrap_or(false)
    }

    pub fn accumulate_usage(&self, tokens_in: u64, tokens_out: u64) {
        let mut usage = self
            .total_usage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        usage.tokens_in += tokens_in;
        usage.tokens_out += tokens_out;
    }

    /// Accumulate Anthropic-style prompt-cache token counters. Called separately
    /// from `accumulate_usage` so existing call sites keep compiling.
    pub fn accumulate_cache_usage(
        &self,
        cache_creation_input_tokens: u64,
        cache_read_input_tokens: u64,
    ) {
        if cache_creation_input_tokens == 0 && cache_read_input_tokens == 0 {
            return;
        }
        let mut usage = self
            .total_usage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        usage.cache_creation_input_tokens += cache_creation_input_tokens;
        usage.cache_read_input_tokens += cache_read_input_tokens;
    }

    pub fn estimated_cost_usd(&self) -> f64 {
        let Some(rate) = self.cost_per_1k_tokens else {
            return 0.0;
        };
        let usage = self.get_total_usage();
        // Anthropic prompt caching pricing: cache_creation_input_tokens are
        // *additional* to tokens_in and cost 1.25× the input rate; cache_read
        // tokens cost 0.1× the input rate. tokens_in itself counts only the
        // non-cached portion of the prompt. We don't have separate
        // input/output rates here, so use `rate` as the common unit price and
        // weight cache tokens accordingly.
        let weighted_tokens = usage.tokens_in as f64
            + usage.tokens_out as f64
            + (usage.cache_creation_input_tokens as f64) * 1.25
            + (usage.cache_read_input_tokens as f64) * 0.1;
        weighted_tokens / 1000.0 * rate
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
            crate::runtime::tools::ToolDispatchOutcome::Completed { event_names, .. } => {
                event_names
            }
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
            crate::runtime::tools::ToolDispatchOutcome::InteractionRequired(_) => {
                return Err(anyhow::anyhow!(
                    "Tool '{}' requires user interaction before it can run.",
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
        self.run_tool_call_with_bus_internal(turn, bus, call, None, None)
            .await
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
            None,
        )
        .await
    }

    pub async fn replay_interaction_tool_call_with_bus(
        &self,
        turn: &TurnState,
        bus: &RuntimeEventBus,
        call: crate::runtime::chat::tool_round_types::RuntimeToolCallRequest,
        interaction_resolution: serde_json::Value,
    ) -> Result<crate::runtime::chat::tool_round_types::RuntimeToolCallOutcome> {
        self.run_tool_call_with_bus_internal(
            turn,
            bus,
            call,
            Some(PermissionDecision::Allow {
                updated_input: None,
                reason: PermissionReason::Other("resolved_pending_interaction".into()),
            }),
            Some(interaction_resolution),
        )
        .await
    }

    async fn run_tool_call_with_bus_internal(
        &self,
        turn: &TurnState,
        bus: &RuntimeEventBus,
        call: crate::runtime::chat::tool_round_types::RuntimeToolCallRequest,
        permission_override: Option<PermissionDecision>,
        interaction_resolution: Option<serde_json::Value>,
    ) -> Result<crate::runtime::chat::tool_round_types::RuntimeToolCallOutcome> {
        use crate::runtime::chat::tool_round_types::RuntimeToolCallOutcome;

        let dispatcher = self
            .tool_dispatcher
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("tool dispatcher not configured"))?;
        let capability_scopes = dispatcher
            .tool_definition(&call.tool_name)
            .await
            .map(|definition| definition.capability_scope)
            .unwrap_or_default();

        // Build execution context with the real tool_call_id from the LLM.
        // TurnState centralizes tool-call scoped cancellation so each call gets
        // a child token of the turn token.
        let ctx = turn.build_execution_context(call.tool_call_id.clone());
        let ctx = self.attach_ltr_registries(ctx);

        // Inject capability context (Workspace-First guarantee) — same logic as
        // `run_tool_with_bus` so workspace-scoped tools receive the correct root.
        let capability_workspace = self.workspace_path.clone().or_else(|| {
            self.authorized_workspace
                .as_ref()
                .map(|aw| aw.root_path.clone())
        });
        let mut ctx = if let Some(workspace_path) = capability_workspace {
            let permission_ctx = self.build_turn_permission_ctx(turn);
            let capability = Arc::new(CapabilityContext {
                storage: Some(StorageCapability {
                    workspace_path,
                    authorized_workspace: self.authorized_workspace.clone(),
                    permission_ctx,
                }),
                workspace_id: Some(turn.session_id().as_str().to_string()),
                file_ops: self.file_ops.clone(),
                read_file_state: Some(self.read_file_state.clone()),
                file_reading_limits: None,
                notification_sink: None,
                runtime_resolver: self.runtime_resolver.clone(),
                is_subagent: turn.agent_id().is_some(),
            });
            ctx.with_capability(capability)
        } else {
            ctx
        };
        if let Some(permission_override) = permission_override {
            ctx = ctx.with_permission_override(permission_override);
        }
        ctx = ctx.with_current_tool_call_request(call.clone());
        if let Some(interaction_resolution) = interaction_resolution {
            ctx = ctx.with_interaction_resolution(interaction_resolution);
        }

        // Emit ToolCallExecuting before dispatching so the UI knows the tool
        // has started before any latency from the actual execution.
        bus.emit(RuntimeEvent::new(
            turn.session_id().clone(),
            turn.run_id().clone(),
            RuntimeEventKind::ToolCallExecuting {
                tool_call_id: crate::runtime::ids::ToolCallId::new(call.tool_call_id.clone()),
                tool_name: call.tool_name.clone(),
                input: call.args.clone(),
            },
        ))
        .await?;

        let msg_id = format!("tool-{}", uuid::Uuid::new_v4());
        let dispatch_start = std::time::Instant::now();
        // Dispatch using the real args from the LLM (not a synthetic placeholder).
        let dispatch_result = dispatcher
            .dispatch(&call.tool_name, call.args.clone(), ctx)
            .await;
        let duration_ms = dispatch_start.elapsed().as_millis() as u64;

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
                        content: tool_result.content.clone(),
                        msg_id: msg_id.clone(),
                        duration_ms: Some(duration_ms),
                    },
                ))
                .await?;

                Ok(RuntimeToolCallOutcome::Completed {
                    tool_call_id: call.tool_call_id,
                    tool_name: call.tool_name,
                    content: tool_result.content,
                    is_error: false,
                    msg_id,
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
                    capability_scopes,
                    original_request: call,
                    decision,
                })
            }
            Ok(crate::runtime::tools::ToolDispatchOutcome::InteractionRequired(request)) => {
                Ok(RuntimeToolCallOutcome::InteractionRequired {
                    tool_call_id: call.tool_call_id.clone(),
                    tool_name: call.tool_name.clone(),
                    original_request: call,
                    interaction_request: *request,
                })
            }
            Err(err) => {
                if let crate::runtime::tools::executor::ToolError::PermissionDenied(ref reason) =
                    err
                {
                    self.record_permission_denial(&call.tool_name, &call.tool_call_id, reason);
                }

                let content = match &err {
                    crate::runtime::tools::executor::ToolError::InputValidationError {
                        tool_name,
                        message,
                    } => format!("InputValidationError for tool '{tool_name}': {message}"),
                    other => other.to_string(),
                };

                bus.emit(RuntimeEvent::new(
                    turn.session_id().clone(),
                    turn.run_id().clone(),
                    RuntimeEventKind::ToolCallCompleted {
                        tool_call_id: crate::runtime::ids::ToolCallId::new(
                            call.tool_call_id.clone(),
                        ),
                        tool_name: call.tool_name.clone(),
                        is_error: true,
                        content: content.clone(),
                        msg_id: msg_id.clone(),
                        duration_ms: Some(duration_ms),
                    },
                ))
                .await?;

                Ok(RuntimeToolCallOutcome::Completed {
                    tool_call_id: call.tool_call_id,
                    tool_name: call.tool_name,
                    content,
                    is_error: true,
                    msg_id,
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
        let ctx = self.attach_ltr_registries(ctx);
        // Inject capability context when workspace_path is available so that
        // workspace-scoped runtime tools (read_workspace_file, etc.)
        // can resolve their root path correctly.  When no workspace_path is set
        // (legacy/test paths), capability remains None and tools that require it
        // will return PermissionDenied as expected.
        let capability_workspace = self.workspace_path.clone().or_else(|| {
            self.authorized_workspace
                .as_ref()
                .map(|aw| aw.root_path.clone())
        });
        let ctx = if let Some(workspace_path) = capability_workspace {
            let permission_ctx = self.build_turn_permission_ctx(turn);
            let capability = Arc::new(CapabilityContext {
                storage: Some(StorageCapability {
                    workspace_path,
                    authorized_workspace: self.authorized_workspace.clone(),
                    permission_ctx,
                }),
                workspace_id: Some(turn.session_id().as_str().to_string()),
                file_ops: self.file_ops.clone(),
                read_file_state: Some(self.read_file_state.clone()),
                file_reading_limits: None,
                notification_sink: None,
                runtime_resolver: self.runtime_resolver.clone(),
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
            crate::runtime::tools::ToolDispatchOutcome::Completed { event_names, .. } => {
                event_names
            }
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
            crate::runtime::tools::ToolDispatchOutcome::InteractionRequired(_) => {
                return Err(anyhow::anyhow!(
                    "Tool '{}' requires user interaction before it can run.",
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
                            input: serde_json::Value::Null, // legacy path: no original args
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
                            content: String::new(), // legacy path: no content available
                            msg_id: format!("tool-{}", uuid::Uuid::new_v4()),
                            duration_ms: None,
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
