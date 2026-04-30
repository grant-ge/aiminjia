use crate::runtime::agent::invocation::AgentInvocation;

#[derive(Clone, Debug)]
pub struct ResumeSnapshot {
    pub invocation: AgentInvocation,
}
