use serde::{Deserialize, Serialize};

const ENVELOPE_PREFIX: &str = "subagent-envelope:v1:";

pub fn build_subagent_transcript_ref(child_run_id: &str) -> String {
    format!("subagent://{child_run_id}")
}

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
    /// Anthropic stop_reason in snake_case ("max_tokens" / "end_turn" /
    /// "tool_use" / "stop_sequence"). Only set when the loop exits via
    /// natural LLM termination (not iter-limit, not cancelled, not
    /// stream error).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_stop_reason: Option<String>,
    /// How many internal recovery attempts the worker made (injecting a
    /// hint user message and re-invoking the LLM) before settling on the
    /// final `output`. 0 means recovery never triggered.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub max_tokens_recovery_attempts: u32,
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_envelope_json_deserializes_with_defaults() {
        // schemaVersion=1 era payload without the new recovery audit fields.
        let old_json = r#"{
            "schemaVersion": 1,
            "output": "hello",
            "iterationsUsed": 3,
            "transcriptRef": "subagent://abc"
        }"#;
        let env: SubAgentResultEnvelope =
            serde_json::from_str(old_json).expect("parse legacy json");
        assert_eq!(env.output, "hello");
        assert_eq!(env.iterations_used, 3);
        assert_eq!(env.terminal_stop_reason, None);
        assert_eq!(env.max_tokens_recovery_attempts, 0);
    }

    #[test]
    fn new_envelope_roundtrip_preserves_recovery_fields() {
        let env = SubAgentResultEnvelope {
            schema_version: 1,
            output: "ok".to_string(),
            iterations_used: 1,
            generated_files: vec![],
            terminal_tool_results: vec![],
            transcript_snapshot: vec![],
            transcript_ref: None,
            terminal_stop_reason: Some("max_tokens".to_string()),
            max_tokens_recovery_attempts: 2,
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(
            json.contains("\"terminalStopReason\":\"max_tokens\""),
            "serialized json should expose camelCase terminalStopReason"
        );
        assert!(
            json.contains("\"maxTokensRecoveryAttempts\":2"),
            "serialized json should expose camelCase maxTokensRecoveryAttempts"
        );
        let back: SubAgentResultEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back.terminal_stop_reason, Some("max_tokens".to_string()));
        assert_eq!(back.max_tokens_recovery_attempts, 2);
    }

    #[test]
    fn zero_recovery_attempts_skipped_in_serialization() {
        // counter=0 should be skipped so old consumers don't see new fields
        let env = SubAgentResultEnvelope {
            schema_version: 1,
            output: "ok".to_string(),
            iterations_used: 1,
            generated_files: vec![],
            terminal_tool_results: vec![],
            transcript_snapshot: vec![],
            transcript_ref: None,
            terminal_stop_reason: None,
            max_tokens_recovery_attempts: 0,
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(
            !json.contains("maxTokensRecoveryAttempts"),
            "zero counter should be skipped"
        );
        assert!(
            !json.contains("terminalStopReason"),
            "None stop_reason should be skipped"
        );
    }

    #[test]
    fn storage_summary_accepts_legacy_payload() {
        let legacy = format!(
            "{ENVELOPE_PREFIX}{}",
            r#"{"schemaVersion":1,"output":"x","iterationsUsed":0}"#
        );
        let env = SubAgentResultEnvelope::from_storage_summary(&legacy)
            .expect("parse legacy storage summary");
        assert_eq!(env.terminal_stop_reason, None);
        assert_eq!(env.max_tokens_recovery_attempts, 0);
    }
}
