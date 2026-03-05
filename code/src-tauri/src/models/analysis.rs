#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisState {
    pub id: String,
    pub conversation_id: String,
    pub current_step: u32,
    pub step_status: serde_json::Value,
    pub state_data: serde_json::Value,
    pub updated_at: String,
}
