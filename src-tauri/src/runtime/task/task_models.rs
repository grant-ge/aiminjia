use crate::runtime::ids::{AgentId, RunId, TaskId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct TaskRecord {
    pub task_id: TaskId,
    pub parent_run_id: RunId,
    pub owner_agent_id: Option<AgentId>,
    pub subject: String,
    pub status: TaskStatus,
}
