//! Inbox writers for the employee run lifecycle.
//!
//! Three entry kinds are appended over the course of one run:
//!
//! 1. `Running`  — written synchronously before the agent task starts, so the
//!    汇报中心 / today feed shows "正在执行" the moment the user clicks 派活.
//! 2. `Report`   — written when the agent completes successfully.
//! 3. `Error`    — written when the agent loop fails (transport / runtime error).
//!
//! The corresponding `Running` entry is marked as read once the run finishes
//! (success or failure) so the unread badge does not double-count.

use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::runtime::employee::inbox::{InboxEntry, InboxKind, InboxStore};

/// Append a `Running` entry. Returns the persisted entry so callers can mark
/// it read once the run completes.
pub fn push_running(
    employees_root: PathBuf,
    employee_id: &str,
    employee_name: &str,
    conversation_id: &str,
    catchup_info: Option<String>,
) -> Result<InboxEntry> {
    let store = InboxStore::new(employees_root);
    store.push(
        employee_id,
        InboxKind::Running,
        format!("{employee_name} 正在执行任务"),
        None,
        None,
        Some(conversation_id.to_string()),
        catchup_info,
    )
}

/// Append a `Report` entry and mark the matching `Running` entry as read.
pub fn push_report(
    employees_root: PathBuf,
    employee_id: &str,
    employee_name: &str,
    conversation_id: &str,
    title: Option<String>,
    summary: Option<String>,
    running_entry_id: Option<&str>,
) -> Result<InboxEntry> {
    let store = InboxStore::new(employees_root);
    if let Some(rid) = running_entry_id {
        let _ = store.mark_read(employee_id, rid);
    }
    let display_title = title.unwrap_or_else(|| format!("{employee_name} 已完成任务"));
    store.push(
        employee_id,
        InboxKind::Report,
        display_title,
        summary,
        None,
        Some(conversation_id.to_string()),
        None,
    )
}

/// Append an `Error` entry and mark the matching `Running` entry as read.
pub fn push_error(
    employees_root: PathBuf,
    employee_id: &str,
    employee_name: &str,
    conversation_id: &str,
    reason: &str,
    running_entry_id: Option<&str>,
) -> Result<InboxEntry> {
    let store = InboxStore::new(employees_root);
    if let Some(rid) = running_entry_id {
        let _ = store.mark_read(employee_id, rid);
    }
    let summary = if reason.is_empty() {
        None
    } else {
        // Cap to keep inbox readable; full error stays in agent transcript.
        let trimmed = reason.chars().take(280).collect::<String>();
        Some(trimmed)
    };
    store.push(
        employee_id,
        InboxKind::Error,
        format!("{employee_name} 任务失败"),
        summary,
        None,
        Some(conversation_id.to_string()),
        None,
    )
}

/// Returns the maximum `created_at` of entries newer than `since` for the
/// given employee. Used by the desktop notification path to decide whether
/// any new inbox entries appeared during a run.
pub fn count_entries_since(
    employees_root: PathBuf,
    employee_id: &str,
    since: DateTime<Utc>,
) -> usize {
    let store = InboxStore::new(employees_root);
    match store.list_for(employee_id, 100) {
        Ok(entries) => entries.into_iter().filter(|e| e.created_at >= since).count(),
        Err(_) => 0,
    }
}
