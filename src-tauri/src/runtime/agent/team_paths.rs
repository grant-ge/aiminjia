//! Per-team disk path derivation and team_name validation.
//!
//! Single source of truth for any path under a conversation directory
//! that relates to a team:
//! `<conv>/teams/{name}/{config.json, team-chat.jsonl, tasks/, teammates/}`.
//!
//! Callers MUST go through `TeamPaths` instead of raw `conv_dir.join(...)`.
//! See `docs/superpowers/specs/2026-05-14-per-team-disk-layout-design.md` §3.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TeamNameError {
    #[error("team_name must not be empty")]
    TooShort,
    #[error("team_name length {len} exceeds max 64")]
    TooLong { len: usize },
    #[error("team_name must match ^[a-zA-Z0-9_-]+$")]
    InvalidChars,
    #[error("team_name `{0}` is a Windows reserved name")]
    WindowsReserved(String),
    #[error("team_name `{0}` is degenerate (all dashes / dots)")]
    DegenerateName(String),
}

const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL",
    "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub fn validate_team_name(raw: &str) -> Result<(), TeamNameError> {
    if raw.is_empty() { return Err(TeamNameError::TooShort); }
    if raw.len() > 64 { return Err(TeamNameError::TooLong { len: raw.len() }); }
    if !raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(TeamNameError::InvalidChars);
    }
    let upper = raw.to_ascii_uppercase();
    if WINDOWS_RESERVED.iter().any(|r| *r == upper) {
        return Err(TeamNameError::WindowsReserved(raw.to_string()));
    }
    if !raw.chars().any(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(TeamNameError::DegenerateName(raw.to_string()));
    }
    Ok(())
}

pub struct TeamPaths<'a> {
    conv_dir: &'a Path,
    team_name: Option<&'a str>,
}

impl<'a> TeamPaths<'a> {
    pub fn for_conv(conv_dir: &'a Path) -> Self {
        Self { conv_dir, team_name: None }
    }

    pub fn for_team(conv_dir: &'a Path, team_name: &'a str) -> Self {
        Self { conv_dir, team_name: Some(team_name) }
    }

    pub fn team_root(&self) -> Option<PathBuf> {
        self.team_name.map(|n| self.conv_dir.join("teams").join(n))
    }

    pub fn config_json(&self) -> PathBuf {
        self.team_root().expect("config_json requires team-bound TeamPaths").join("config.json")
    }

    pub fn team_chat_jsonl(&self) -> PathBuf {
        self.team_root().expect("team_chat_jsonl requires team-bound TeamPaths").join("team-chat.jsonl")
    }

    pub fn tasks_dir(&self) -> PathBuf {
        match self.team_name {
            Some(n) => self.conv_dir.join("teams").join(n).join("tasks"),
            None => self.conv_dir.join("tasks"),
        }
    }

    pub fn teammates_dir(&self) -> PathBuf {
        self.team_root().expect("teammates_dir requires team-bound TeamPaths").join("teammates")
    }

    pub fn teammate_transcript(&self, agent_id: &str) -> PathBuf {
        self.teammates_dir().join(format!("{agent_id}.jsonl"))
    }

    pub fn teammate_meta(&self, agent_id: &str) -> PathBuf {
        self.teammates_dir().join(format!("{agent_id}.meta.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn conv() -> PathBuf {
        PathBuf::from("/home/u/.renlijia/users/u1/conversations/conv-1")
    }

    #[test] fn validate_accepts_simple_ascii() {
        assert!(validate_team_name("alpha").is_ok());
        assert!(validate_team_name("research-team").is_ok());
        assert!(validate_team_name("team_01").is_ok());
        assert!(validate_team_name("A").is_ok());
    }
    #[test] fn validate_rejects_empty() { assert_eq!(validate_team_name(""), Err(TeamNameError::TooShort)); }
    #[test] fn validate_rejects_too_long() {
        let s = "a".repeat(65);
        assert_eq!(validate_team_name(&s), Err(TeamNameError::TooLong { len: 65 }));
    }
    #[test] fn validate_accepts_max_length() {
        assert!(validate_team_name(&"a".repeat(64)).is_ok());
    }
    #[test] fn validate_rejects_chinese() {
        assert_eq!(validate_team_name("市场调研"), Err(TeamNameError::InvalidChars));
    }
    #[test] fn validate_rejects_emoji() {
        assert_eq!(validate_team_name("team-🔥"), Err(TeamNameError::InvalidChars));
    }
    #[test] fn validate_rejects_space() {
        assert_eq!(validate_team_name("research team"), Err(TeamNameError::InvalidChars));
    }
    #[test] fn validate_rejects_path_separator() {
        assert_eq!(validate_team_name("team/alpha"), Err(TeamNameError::InvalidChars));
        assert_eq!(validate_team_name("team\\alpha"), Err(TeamNameError::InvalidChars));
    }
    #[test] fn validate_rejects_dot_and_dotdot() {
        assert_eq!(validate_team_name("."), Err(TeamNameError::InvalidChars));
        assert_eq!(validate_team_name(".."), Err(TeamNameError::InvalidChars));
    }
    #[test] fn validate_rejects_windows_reserved() {
        for name in &["CON","con","Con","PRN","prn","AUX","NUL","COM1","com9","LPT5"] {
            assert!(matches!(validate_team_name(name), Err(TeamNameError::WindowsReserved(_))));
        }
    }
    #[test] fn validate_accepts_reserved_prefix() {
        assert!(validate_team_name("CONFIG").is_ok());
        assert!(validate_team_name("PRINTER").is_ok());
        assert!(validate_team_name("COM10").is_ok());
    }
    #[test] fn validate_rejects_all_dashes() {
        assert_eq!(validate_team_name("---"), Err(TeamNameError::DegenerateName("---".to_string())));
        assert_eq!(validate_team_name("-"), Err(TeamNameError::DegenerateName("-".to_string())));
    }
    #[test] fn team_root_for_conv_returns_none() {
        assert_eq!(TeamPaths::for_conv(&conv()).team_root(), None);
    }
    #[test] fn team_root_for_team() {
        let dir = conv();
        assert_eq!(TeamPaths::for_team(&dir, "alpha").team_root(), Some(dir.join("teams").join("alpha")));
    }
    #[test] fn config_json_for_team() {
        let dir = conv();
        assert_eq!(TeamPaths::for_team(&dir, "alpha").config_json(), dir.join("teams/alpha/config.json"));
    }
    #[test] #[should_panic] fn config_json_for_conv_panics() {
        let _ = TeamPaths::for_conv(&conv()).config_json();
    }
    #[test] fn team_chat_for_team() {
        let dir = conv();
        assert_eq!(TeamPaths::for_team(&dir, "alpha").team_chat_jsonl(), dir.join("teams/alpha/team-chat.jsonl"));
    }
    #[test] fn tasks_dir_for_conv() {
        let dir = conv();
        assert_eq!(TeamPaths::for_conv(&dir).tasks_dir(), dir.join("tasks"));
    }
    #[test] fn tasks_dir_for_team() {
        let dir = conv();
        assert_eq!(TeamPaths::for_team(&dir, "alpha").tasks_dir(), dir.join("teams/alpha/tasks"));
    }
    #[test] fn teammates_dir_for_team() {
        let dir = conv();
        assert_eq!(TeamPaths::for_team(&dir, "alpha").teammates_dir(), dir.join("teams/alpha/teammates"));
    }
    #[test] fn teammate_transcript() {
        let dir = conv();
        assert_eq!(TeamPaths::for_team(&dir, "alpha").teammate_transcript("agent-42"), dir.join("teams/alpha/teammates/agent-42.jsonl"));
    }
    #[test] fn teammate_meta() {
        let dir = conv();
        assert_eq!(TeamPaths::for_team(&dir, "alpha").teammate_meta("agent-42"), dir.join("teams/alpha/teammates/agent-42.meta.json"));
    }
}
