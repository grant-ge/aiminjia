use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookConfig {
    pub event: HookEvent,
    pub command: String,
    pub tool_filter: Option<String>,
    pub timeout_secs: Option<u64>,
}

impl HookConfig {
    pub fn matches_tool(&self, tool_name: &str) -> bool {
        match &self.tool_filter {
            Some(filter) => filter == tool_name,
            None => true,
        }
    }

    pub fn effective_timeout_secs(&self) -> u64 {
        self.timeout_secs.unwrap_or(30)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookRegistry {
    pub hooks: Vec<HookConfig>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn hooks_for(&self, event: HookEvent, tool_name: &str) -> Vec<&HookConfig> {
        self.hooks
            .iter()
            .filter(|hook| hook.event == event && hook.matches_tool(tool_name))
            .collect()
    }
}
