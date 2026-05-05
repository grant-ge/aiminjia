//! Append-only JSONL writer for async sub-agent transcripts.
//!
//! Each line is `{"role": "...", "content": "..."}` serializable as JSON.
//! Used by P6.2 launch_async lifecycle (follow-up wiring) to record sub-agent
//! results so the parent LLM can read them incrementally via the `task_output`
//! tool.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranscriptLine {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl TranscriptLine {
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            error: None,
        }
    }
    pub fn tool(content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: content.into(),
            error: None,
        }
    }
    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: String::new(),
            error: Some(error.into()),
        }
    }
}

/// Compute the transcript file path for an agent given the user's scoped subagent dir.
pub fn transcript_path(subagent_transcripts_dir: &Path, agent_id: &str) -> PathBuf {
    subagent_transcripts_dir.join(format!("{agent_id}.jsonl"))
}

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
}
