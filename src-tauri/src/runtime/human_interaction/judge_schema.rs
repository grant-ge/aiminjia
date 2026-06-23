use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgeAction {
    Resolve,
    AbandonNewTurn,
    Clarify,
    NotForInteraction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgeKind {
    Permission,
    AskUserQuestion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanInteractionJudgeDecision {
    pub action: JudgeAction,
    pub kind: JudgeKind,
    #[serde(default)]
    pub payload: serde_json::Value,
    pub reason: String,
}

impl HumanInteractionJudgeDecision {
    pub fn parse_json(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }
}
