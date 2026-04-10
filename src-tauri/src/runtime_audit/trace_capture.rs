use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::commands::chat::testsupport;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapturedEvent {
    pub name: String,
    pub payload: serde_json::Value,
}

#[derive(Clone, Debug, Default)]
pub struct CapturedTrace {
    pub events: Vec<CapturedEvent>,
}

impl CapturedTrace {
    pub fn new(events: Vec<CapturedEvent>) -> Self {
        Self { events }
    }

    pub fn event_names(&self) -> Vec<String> {
        self.events.iter().map(|event| event.name.clone()).collect()
    }
}

#[derive(Clone, Copy, Debug)]
pub enum LegacyTraceScenario {
    BasicChat,
    SingleTool,
}

pub async fn capture_legacy_trace(scenario: LegacyTraceScenario) -> Result<CapturedTrace> {
    match scenario {
        LegacyTraceScenario::BasicChat => {
            testsupport::run_send_message_through_runtime("conv-trace", "hello").await
        }
        LegacyTraceScenario::SingleTool => {
            testsupport::run_tool_turn_through_runtime("conv-trace", "python_exec").await
        }
    }
}
