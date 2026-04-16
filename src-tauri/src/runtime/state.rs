use crate::runtime::cancellation::CancellationToken;
use crate::runtime::identity::{IdentityMapping, RuntimeIdentity};
use crate::runtime::ids::{AgentId, RunId, SessionId, ToolCallId};

#[derive(Clone, Debug)]
pub struct TurnState {
    identity: RuntimeIdentity,
    legacy_conversation_id: Option<String>,
    agent_id: Option<AgentId>,
    user_input: String,
    pending_assistant_output: String,
    active_tool_call: Option<ToolCallId>,
    cancellation: CancellationToken,
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
        }
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
}
