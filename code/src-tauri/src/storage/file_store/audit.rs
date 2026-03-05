//! Audit log with auto-splitting.
//!
//! Stored in `audit/audit.jsonl` with automatic splitting at 2 MB.

use std::path::{Path, PathBuf};

use chrono::Utc;

use super::error::StorageResult;
use super::io::append_jsonl_with_split;
use super::types::AuditEntry;

/// Audit log auto-split threshold: 2 MB.
const AUDIT_MAX_BYTES: u64 = 2_097_152;

fn audit_path(base_dir: &Path) -> PathBuf {
    base_dir.join("audit").join("audit.jsonl")
}

/// Append an entry to the audit log.
pub fn log_action(
    base_dir: &Path,
    action: &str,
    detail: Option<&str>,
) -> StorageResult<()> {
    let entry = AuditEntry {
        action: action.to_string(),
        detail: detail.map(|s| s.to_string()),
        created_at: Utc::now().to_rfc3339(),
    };

    let path = audit_path(base_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    append_jsonl_with_split(&path, &entry, AUDIT_MAX_BYTES)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_log_action() {
        let dir = TempDir::new().unwrap();
        let base = dir.path().to_path_buf();

        log_action(&base, "conversation_created", Some("id=c1")).unwrap();
        log_action(&base, "file_deleted", None).unwrap();

        // Verify the file exists and has content
        assert!(audit_path(&base).exists());
    }
}
