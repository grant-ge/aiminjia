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
