use crate::runtime::ids::AgentId;

#[derive(Clone, Debug)]
pub struct TeamContext {
    pub team_id: String,
    pub agent_ids: Vec<AgentId>,
}
