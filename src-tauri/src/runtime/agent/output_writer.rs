//! Append-only JSONL writer for async sub-agent and Teammate transcripts.
//!
//! Each line is `{"role": "...", "content": "..."}` serializable as JSON.
//! Used by P6.2 launch_async lifecycle (follow-up wiring) to record sub-agent
//! results so the parent LLM can read them incrementally via the `task_output`
//! tool.
//!
//! ## Path routing (P1.6)
//!
//! | WorkerMode      | transcript path                                  |
//! |-----------------|--------------------------------------------------|
//! | `AsyncOneShot`  | `conversations/{conv_id}/subagents/{id}.jsonl`   |
//! | `TeammateIdle`  | `conversations/{conv_id}/teammates/{id}.jsonl`   |
//!
//! A `.meta.json` sidecar is written once at spawn time alongside the JSONL.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

// ─── TranscriptLine ───────────────────────────────────────────────────────────

/// Lightweight tool-call record for transcript serialization.
///
/// Mirrors `crate::llm::streaming::ToolCall` but with `arguments` typed as
/// `serde_json::Value` for transport simplicity.  Stored on `TranscriptLine`
/// for assistant rows so future transcript-replay can reconstruct
/// Anthropic-compliant `tool_use` blocks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranscriptToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranscriptLine {
    pub role: String,
    pub content: String,
    /// Tool calls issued by an assistant message.  Populated when the LLM
    /// requested tool use on this turn; `None` for plain assistant text or
    /// for user/tool rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<TranscriptToolCall>>,
    /// `tool_use_id` this row is responding to.  Populated only for tool
    /// result rows (`role == "tool"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Tool name this row is responding to.  Populated only for tool result
    /// rows (`role == "tool"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Originating sender name for `role == "user"` rows on Teammate
    /// transcripts.  `"team-lead"` when the message came from the Lead, the
    /// teammate name (e.g. `"con-debater"`) when it came from another peer,
    /// `"system"` for system-injected rows (initial role brief, scheduler
    /// pings).  `None` for assistant / tool rows or for legacy entries written
    /// before this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl TranscriptLine {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
            from: None,
            error: None,
        }
    }
    /// Same as [`Self::user`] but tags the row with an explicit sender name
    /// (e.g. `"team-lead"` / `"con-debater"` / `"system"`).
    pub fn user_from(content: impl Into<String>, from: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
            from: Some(from.into()),
            error: None,
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
            from: None,
            error: None,
        }
    }
    pub fn assistant_with_tool_calls(
        content: impl Into<String>,
        tool_calls: Vec<TranscriptToolCall>,
    ) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            tool_call_id: None,
            tool_name: None,
            from: None,
            error: None,
        }
    }
    pub fn tool(content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
            from: None,
            error: None,
        }
    }
    pub fn tool_result(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: "tool".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            tool_name: Some(tool_name.into()),
            from: None,
            error: None,
        }
    }
    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: String::new(),
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
            from: None,
            error: Some(error.into()),
        }
    }

    /// Build a transcript line from a `ChatMessage`.  Used by Teammate idle
    /// loop's `on_message_appended` callback to mirror in-memory messages to
    /// the JSONL transcript so future transcript-replay can reconstruct
    /// Anthropic-compliant messages.
    pub fn from_chat_message(message: &crate::llm::streaming::ChatMessage) -> Self {
        let tool_calls = message.tool_calls.as_ref().map(|calls| {
            calls
                .iter()
                .map(|tc| TranscriptToolCall {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                })
                .collect::<Vec<_>>()
        });
        Self {
            role: message.role.clone(),
            content: message.content.clone(),
            tool_calls: tool_calls.filter(|v| !v.is_empty()),
            tool_call_id: message.tool_call_id.clone(),
            tool_name: message.name.clone(),
            from: None,
            error: None,
        }
    }
}

// ─── TranscriptKind (P1.6) ────────────────────────────────────────────────────

/// Discriminates between the two worker modes for path routing and sidecar
/// metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptKind {
    /// One-shot async sub-agent launched via `run_in_background=true`.
    Subagent,
    /// Long-lived Teammate in idle loop.
    Teammate,
}

impl TranscriptKind {
    /// Returns the subdirectory name under the conversation directory.
    pub fn dir_name(&self) -> &'static str {
        match self {
            Self::Subagent => "subagents",
            Self::Teammate => "teammates",
        }
    }

    /// JSON string value stored in `.meta.json`.
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::Subagent => "subagent",
            Self::Teammate => "teammate",
        }
    }
}

// ─── AgentTranscriptMeta (P1.6 sidecar) ──────────────────────────────────────

/// Metadata written as a `.meta.json` sidecar alongside the JSONL transcript.
///
/// Written once at spawn time; never appended to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTranscriptMeta {
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    pub kind: TranscriptKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_id: Option<String>,
    /// The conversation id — used as the team scope id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawned_by: Option<String>,
    pub spawned_at: chrono::DateTime<chrono::Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub is_async: bool,
    pub tool_whitelist: Vec<String>,
    /// LTR (P2.3): the fully-composed boot system prompt the Teammate's LLM
    /// will see on its first turn (= Employee.system_prompt_extra +
    /// TEAMMATE_ADDENDUM with team_name / teammate_name substituted in).
    /// `None` for AsyncOneShot / legacy paths.  Recorded in the sidecar so
    /// it is auditable without rerunning the boot machinery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boot_system_prompt: Option<String>,
}

// ─── Path helpers (P1.6) ─────────────────────────────────────────────────────

/// Compute the per-kind transcript directory inside a conversation directory.
///
/// `conv_dir` should be `<aijia_home>/users/{scope}/conversations/{conv_id}`.
pub fn kind_dir(conv_dir: &Path, kind: &TranscriptKind) -> PathBuf {
    conv_dir.join(kind.dir_name())
}

/// Compute the JSONL transcript path.
///
/// `conv_dir` is the conversation root (NOT a sub-directory).
/// `team_name` is required for `TranscriptKind::Teammate` (per-team disk
/// layout v2 §3 — Teammates always live under a team).  Pass `""` for
/// `TranscriptKind::Subagent` callers; the value is ignored.
/// `agent_id` is the agent's unique ID string.
pub fn transcript_path_for_kind(
    conv_dir: &Path,
    kind: &TranscriptKind,
    team_name: &str,
    agent_id: &str,
) -> PathBuf {
    use crate::runtime::agent::team_paths::TeamPaths;
    match kind {
        TranscriptKind::Teammate => {
            TeamPaths::for_team(conv_dir, team_name).teammate_transcript(agent_id)
        }
        TranscriptKind::Subagent => conv_dir.join("subagents").join(format!("{agent_id}.jsonl")),
    }
}

/// Compute the `.meta.json` sidecar path.  See `transcript_path_for_kind`
/// for the `team_name` contract.
pub fn meta_path_for_kind(
    conv_dir: &Path,
    kind: &TranscriptKind,
    team_name: &str,
    agent_id: &str,
) -> PathBuf {
    use crate::runtime::agent::team_paths::TeamPaths;
    match kind {
        TranscriptKind::Teammate => {
            TeamPaths::for_team(conv_dir, team_name).teammate_meta(agent_id)
        }
        TranscriptKind::Subagent => conv_dir
            .join("subagents")
            .join(format!("{agent_id}.meta.json")),
    }
}

/// Write the `.meta.json` sidecar once at spawn time.  Creates parent dirs.
/// Overwrites any existing sidecar (idempotent for retries).
///
/// For Teammate kind, `meta.team_id` must hold the team_name (per-team disk
/// layout v2 §3); empty/missing returns an error.  `Subagent` kind ignores
/// `team_id`.
pub fn write_meta(conv_dir: &Path, meta: &AgentTranscriptMeta) -> Result<()> {
    let team_name = match meta.kind {
        TranscriptKind::Teammate => meta
            .team_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Teammate AgentTranscriptMeta.team_id must hold the team_name (agent_id={})",
                    meta.agent_id
                )
            })?,
        TranscriptKind::Subagent => "",
    };
    let path = meta_path_for_kind(conv_dir, &meta.kind, team_name, &meta.agent_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(meta)?;
    std::fs::write(&path, json)?;
    Ok(())
}

// ─── Legacy helper (kept for callers not yet migrated) ───────────────────────

/// Compute the transcript file path for an agent given the user's scoped
/// sub-agent directory.
///
/// **Legacy**: prefer [`transcript_path_for_kind`] with an explicit `conv_dir`
/// for new code.
pub fn transcript_path(subagent_transcripts_dir: &Path, agent_id: &str) -> PathBuf {
    subagent_transcripts_dir.join(format!("{agent_id}.jsonl"))
}

// ─── I/O helpers ─────────────────────────────────────────────────────────────

/// Append a single line. Creates parent dir if missing. Atomic per-line.
pub fn append_line(path: &Path, line: &TranscriptLine) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut json = serde_json::to_string(line)?;
    json.push('\n');
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(json.as_bytes())?;
    Ok(())
}

/// Read all transcript lines, return slice from `offset` and the new offset
/// (= total line count). Lines are returned as raw JSON strings — caller
/// decides whether to parse.
pub fn read_from(path: &Path, offset: usize) -> Result<(Vec<String>, usize)> {
    if !path.exists() {
        return Ok((Vec::new(), 0));
    }
    let content = std::fs::read_to_string(path)?;
    let all: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let total = all.len();
    if offset >= total {
        return Ok((Vec::new(), total));
    }
    let slice: Vec<String> = all.into_iter().skip(offset).collect();
    Ok((slice, total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn append_line_creates_file_and_dir() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nested").join("agent-1.jsonl");
        append_line(&path, &TranscriptLine::assistant("hello")).unwrap();
        assert!(path.exists());
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("\"role\":\"assistant\""));
        assert!(body.contains("\"content\":\"hello\""));
        assert!(body.ends_with('\n'));
    }

    #[test]
    fn append_line_appends_in_order() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a.jsonl");
        append_line(&path, &TranscriptLine::assistant("first")).unwrap();
        append_line(&path, &TranscriptLine::tool("second")).unwrap();
        append_line(&path, &TranscriptLine::failed("oops")).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("first"));
        assert!(lines[1].contains("second"));
        assert!(lines[2].contains("\"error\":\"oops\""));
    }

    #[test]
    fn read_from_zero_returns_all_with_total_offset() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a.jsonl");
        append_line(&path, &TranscriptLine::assistant("a")).unwrap();
        append_line(&path, &TranscriptLine::assistant("b")).unwrap();
        let (lines, new_off) = read_from(&path, 0).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(new_off, 2);
    }

    #[test]
    fn read_from_offset_returns_tail() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a.jsonl");
        for i in 0..5 {
            append_line(&path, &TranscriptLine::assistant(format!("msg-{i}"))).unwrap();
        }
        let (lines, new_off) = read_from(&path, 3).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(new_off, 5);
        assert!(lines[0].contains("msg-3"));
        assert!(lines[1].contains("msg-4"));
    }

    #[test]
    fn read_from_past_end_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a.jsonl");
        append_line(&path, &TranscriptLine::assistant("only")).unwrap();
        let (lines, new_off) = read_from(&path, 10).unwrap();
        assert!(lines.is_empty());
        assert_eq!(new_off, 1);
    }

    #[test]
    fn read_nonexistent_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("never.jsonl");
        let (lines, new_off) = read_from(&path, 0).unwrap();
        assert!(lines.is_empty());
        assert_eq!(new_off, 0);
    }

    #[test]
    fn transcript_path_format() {
        let dir = std::path::PathBuf::from("/tmp/sub");
        let p = transcript_path(&dir, "agent-xyz");
        assert_eq!(p, dir.join("agent-xyz.jsonl"));
    }

    // ── P1.6: kind-routing tests ──────────────────────────────────────────────

    #[test]
    fn subagent_kind_uses_subagents_dir() {
        let conv_dir = std::path::PathBuf::from("/tmp/conv-abc");
        let p = transcript_path_for_kind(&conv_dir, &TranscriptKind::Subagent, "", "agent-1");
        assert_eq!(p, conv_dir.join("subagents/agent-1.jsonl"));
        let mp = meta_path_for_kind(&conv_dir, &TranscriptKind::Subagent, "", "agent-1");
        assert_eq!(mp, conv_dir.join("subagents/agent-1.meta.json"));
    }

    #[test]
    fn teammate_kind_uses_team_scoped_teammates_dir() {
        let conv_dir = std::path::PathBuf::from("/tmp/conv-abc");
        let p = transcript_path_for_kind(&conv_dir, &TranscriptKind::Teammate, "alpha", "agent-2");
        assert_eq!(p, conv_dir.join("teams/alpha/teammates/agent-2.jsonl"));
        let mp = meta_path_for_kind(&conv_dir, &TranscriptKind::Teammate, "alpha", "agent-2");
        assert_eq!(mp, conv_dir.join("teams/alpha/teammates/agent-2.meta.json"));
    }

    #[test]
    fn write_meta_creates_sidecar_with_correct_fields() {
        let tmp = TempDir::new().unwrap();
        let conv_dir = tmp.path().join("conversations/conv-test");
        let meta = AgentTranscriptMeta {
            agent_id: "agent-999".to_string(),
            agent_name: Some("researcher".to_string()),
            kind: TranscriptKind::Teammate,
            employee_id: Some("emp-42".to_string()),
            team_id: Some("alpha".to_string()),
            spawned_by: Some("lead-agent-id".to_string()),
            spawned_at: chrono::Utc::now(),
            model: Some("sonnet".to_string()),
            is_async: true,
            boot_system_prompt: None,
            tool_whitelist: vec!["Read".to_string(), "SendMessage".to_string()],
        };
        write_meta(&conv_dir, &meta).unwrap();
        let path = conv_dir.join("teams/alpha/teammates/agent-999.meta.json");
        assert!(path.exists());
        let body = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["kind"].as_str(), Some("teammate"));
        assert_eq!(parsed["agent_name"].as_str(), Some("researcher"));
        assert_eq!(parsed["employee_id"].as_str(), Some("emp-42"));
        assert_eq!(parsed["team_id"].as_str(), Some("alpha"));
    }

    #[test]
    fn write_meta_subagent_kind_str() {
        let tmp = TempDir::new().unwrap();
        let conv_dir = tmp.path().join("conversations/conv-sub");
        let meta = AgentTranscriptMeta {
            agent_id: "agent-sub-1".to_string(),
            agent_name: None,
            kind: TranscriptKind::Subagent,
            employee_id: None,
            team_id: None,
            spawned_by: None,
            spawned_at: chrono::Utc::now(),
            model: None,
            is_async: true,
            boot_system_prompt: None,
            tool_whitelist: vec![],
        };
        write_meta(&conv_dir, &meta).unwrap();
        let path = conv_dir.join("subagents/agent-sub-1.meta.json");
        assert!(path.exists());
        let body = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["kind"].as_str(), Some("subagent"));
    }
}
