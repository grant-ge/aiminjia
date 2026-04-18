use serde::{Deserialize, Serialize};

const ENVELOPE_PREFIX: &str = "subagent-envelope:v1:";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubAgentTerminalToolResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub success: bool,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generated_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubAgentTranscriptEntry {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubAgentResultEnvelope {
    pub schema_version: u32,
    pub output: String,
    pub iterations_used: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generated_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terminal_tool_results: Vec<SubAgentTerminalToolResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transcript_snapshot: Vec<SubAgentTranscriptEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_ref: Option<String>,
}

impl SubAgentResultEnvelope {
    pub fn to_storage_summary(&self) -> String {
        match serde_json::to_string(self) {
            Ok(json) => format!("{ENVELOPE_PREFIX}{json}"),
            Err(_) => format!(
                "{ENVELOPE_PREFIX}{{\"schemaVersion\":1,\"output\":\"serialization_failed\"}}"
            ),
        }
    }

    pub fn from_storage_summary(summary: &str) -> Option<Self> {
        let payload = summary.strip_prefix(ENVELOPE_PREFIX)?;
        serde_json::from_str(payload).ok()
    }
}
