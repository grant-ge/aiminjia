#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct PermissionDenialRecord {
    pub tool_name: String,
    pub tool_call_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum ChatTurnOutcome {
    Success,
    Cancelled,
    MaxIterationsReached { iterations: usize },
    BudgetExceeded { reason: String, total_cost_usd: f64 },
    ExecutionError { message: String },
}

impl ChatTurnOutcome {
    pub fn is_error(&self) -> bool {
        matches!(
            self,
            ChatTurnOutcome::MaxIterationsReached { .. }
                | ChatTurnOutcome::BudgetExceeded { .. }
                | ChatTurnOutcome::ExecutionError { .. }
        )
    }

    pub fn is_success(&self) -> bool {
        matches!(self, ChatTurnOutcome::Success)
    }
}
