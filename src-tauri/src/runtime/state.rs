use crate::runtime::cancellation::CancellationToken;
use crate::runtime::human_interaction::{OutputBinding, TurnOrigin};
use crate::runtime::identity::{IdentityMapping, RuntimeIdentity};
use crate::runtime::ids::{AgentId, RunId, SessionId, ToolCallId};
use crate::runtime::tools::permission::PermissionMode;

#[derive(Clone, Debug)]
pub struct TurnState {
    identity: RuntimeIdentity,
    legacy_conversation_id: Option<String>,
    agent_id: Option<AgentId>,
    user_input: String,
    pending_assistant_output: String,
    active_tool_call: Option<ToolCallId>,
    cancellation: CancellationToken,
    permission_mode: PermissionMode,
    turn_origin: TurnOrigin,
    output_binding: OutputBinding,
    /// The primary LLM model name used for this turn (e.g. "deepseek-v3").
    /// Populated by the driver after TurnConfig is built; empty string until set.
    primary_model: String,
    /// LTR P2.8: when `true` this turn is an async runner (Teammate / async
    /// sub-agent) with no UI thread.  Propagated into every ToolExecutionContext
    /// built from this turn so `apply_async_auto_deny` can convert any
    /// `Ask` permission decision to `Deny` instead of blocking forever.
    is_async: bool,
}

impl TurnState {
    pub fn new(mapping: IdentityMapping, run_id: RunId, user_input: String) -> Self {
        let identity = RuntimeIdentity::new(mapping.session_id.clone(), run_id);
        Self {
            identity,
            legacy_conversation_id: mapping.legacy_conversation_id,
            agent_id: None,
            user_input,
            pending_assistant_output: String::new(),
            active_tool_call: None,
            cancellation: CancellationToken::new(),
            permission_mode: PermissionMode::Default,
            turn_origin: TurnOrigin::App,
            output_binding: OutputBinding::AppOnly,
            primary_model: String::new(),
            is_async: false,
        }
    }

    /// LTR P2.8: mark this turn as running inside an async runner (Teammate /
    /// async sub-agent).  Causes every ToolExecutionContext built from this
    /// turn to carry `is_async = true`, which triggers `apply_async_auto_deny`
    /// on any permission `Ask` decision.
    pub fn with_async(mut self, is_async: bool) -> Self {
        self.is_async = is_async;
        self
    }

    pub fn is_async(&self) -> bool {
        self.is_async
    }

    /// Attach a cancellation token to this turn, replacing the default one created in `new`.
    ///
    /// Use this to wire the session → turn cancel cascade:
    /// ```ignore
    /// let turn = TurnState::new(mapping, run_id, input)
    ///     .with_cancellation(session_cancel_token.child_token());
    /// ```
    pub fn with_cancellation(mut self, token: CancellationToken) -> Self {
        self.cancellation = token;
        self
    }

    pub fn with_permission_mode(mut self, mode: PermissionMode) -> Self {
        self.permission_mode = mode;
        self
    }

    pub fn with_human_interaction_metadata(
        mut self,
        turn_origin: TurnOrigin,
        output_binding: OutputBinding,
    ) -> Self {
        self.turn_origin = turn_origin;
        self.output_binding = output_binding;
        self
    }

    /// Build a [`crate::runtime::tools::context::ToolExecutionContext`] for a single tool call
    /// within this turn.
    ///
    /// Creates a **child** cancellation token from the turn-level token so that:
    /// - Cancelling the turn cascades down to the running tool call.
    /// - Cancelling one tool call does not affect the turn token or sibling tool calls.
    pub fn build_execution_context(
        &self,
        tool_call_id: impl Into<String>,
    ) -> crate::runtime::tools::context::ToolExecutionContext {
        crate::runtime::tools::context::ToolExecutionContext::new(
            self.session_id().clone(),
            self.run_id().clone(),
            self.agent_id().cloned(),
            tool_call_id,
            self.cancellation.child_token(),
        )
        .with_permission_mode(self.permission_mode)
        .with_human_interaction_metadata(self.turn_origin.clone(), self.output_binding.clone())
        .with_async(self.is_async)
    }

    pub fn session_id(&self) -> &SessionId {
        self.identity.session_id()
    }

    pub fn run_id(&self) -> &RunId {
        self.identity.run_id()
    }

    pub fn legacy_conversation_id(&self) -> Option<&str> {
        self.legacy_conversation_id.as_deref()
    }

    pub fn agent_id(&self) -> Option<&AgentId> {
        self.agent_id.as_ref()
    }

    pub fn set_agent_id(&mut self, agent_id: AgentId) {
        self.agent_id = Some(agent_id);
    }

    pub fn user_input(&self) -> &str {
        &self.user_input
    }

    pub fn pending_assistant_output(&self) -> &str {
        &self.pending_assistant_output
    }

    pub fn append_output(&mut self, delta: &str) {
        self.pending_assistant_output.push_str(delta);
    }

    pub fn active_tool_call(&self) -> Option<&ToolCallId> {
        self.active_tool_call.as_ref()
    }

    pub fn set_active_tool_call(&mut self, tool_call_id: Option<ToolCallId>) {
        self.active_tool_call = tool_call_id;
    }

    pub fn cancellation(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    pub fn permission_mode(&self) -> PermissionMode {
        self.permission_mode
    }

    pub fn turn_origin(&self) -> &TurnOrigin {
        &self.turn_origin
    }

    pub fn output_binding(&self) -> &OutputBinding {
        &self.output_binding
    }

    pub fn primary_model(&self) -> &str {
        &self.primary_model
    }

    pub fn set_primary_model(&mut self, model: String) {
        self.primary_model = model;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::identity::IdentityMapping;
    use crate::runtime::ids::RunId;
    use crate::runtime::tools::permission::PermissionMode;

    #[test]
    fn build_execution_context_inherits_permission_mode() {
        let turn = TurnState::new(
            IdentityMapping::from_legacy_conversation_id("conv-state-mode".to_string()),
            RunId::new("run-state-mode"),
            "hello".to_string(),
        )
        .with_permission_mode(PermissionMode::Plan);

        let ctx = turn.build_execution_context("tool-call-state-mode");

        assert_eq!(ctx.permission_mode, PermissionMode::Plan);
    }

    #[test]
    fn build_execution_context_inherits_human_interaction_metadata() {
        let origin = crate::runtime::human_interaction::TurnOrigin::Im {
            platform: crate::runtime::human_interaction::ImPlatform::Dingtalk,
            external_conversation_key: "chat-1".to_string(),
            sender_id: Some("sender-1".to_string()),
            sender_label: Some("钉钉用户".to_string()),
            account_id: Some("bot-1".to_string()),
            thread_id: None,
        };
        let binding = crate::runtime::human_interaction::OutputBinding::im(
            crate::runtime::human_interaction::ImPlatform::Dingtalk,
            "sess",
            "chat-1",
            true,
        );
        let turn = TurnState::new(
            IdentityMapping::from_legacy_conversation_id("conv-state-origin".to_string()),
            RunId::new("run-state-origin"),
            "hello".to_string(),
        )
        .with_human_interaction_metadata(origin.clone(), binding.clone());

        let ctx = turn.build_execution_context("tool-call-state-origin");

        assert_eq!(ctx.turn_origin, origin);
        assert_eq!(ctx.output_binding, binding);
    }
}
