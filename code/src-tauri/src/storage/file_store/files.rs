//! File index management (uploaded + generated files).
//!
//! All file metadata is stored in `file_index.json` per conversation.
//! Physical files live in `uploads/` and `generated/` subdirectories.

use std::path::{Path, PathBuf};

use chrono::Utc;

use super::conversations::conv_dir;
use super::error::StorageResult;
use super::io::{atomic_write_json, read_json_optional};
use super::types::{FileEntry, FileIndex};

fn file_index_path(base_dir: &Path, conversation_id: &str) -> PathBuf {
    conv_dir(base_dir, conversation_id).join("file_index.json")
}

fn read_file_index(base_dir: &Path, conversation_id: &str) -> StorageResult<FileIndex> {
    Ok(read_json_optional(&file_index_path(base_dir, conversation_id))?.unwrap_or_default())
}

fn write_file_index(
    base_dir: &Path,
    conversation_id: &str,
    index: &FileIndex,
) -> StorageResult<()> {
    atomic_write_json(&file_index_path(base_dir, conversation_id), index)?;
    Ok(())
}

// ─── Uploaded Files ──────────────────────────────────────────────────────────

/// Insert an uploaded file record.
#[allow(clippy::too_many_arguments)]
pub fn insert_uploaded_file(
    base_dir: &Path,
    id: &str,
    conversation_id: &str,
    original_name: &str,
    stored_path: &str,
    file_type: &str,
    file_size: i64,
    parsed_summary: Option<&str>,
) -> StorageResult<()> {
    let mut index = read_file_index(base_dir, conversation_id)?;
    let now = Utc::now().to_rfc3339();

    index.files.push(FileEntry {
        id: id.to_string(),
        source: "upload".to_string(),
        file_name: original_name.to_string(),
        stored_path: stored_path.to_string(),
        file_type: file_type.to_string(),
        file_size,
        original_name: Some(original_name.to_string()),
        parsed_summary: parsed_summary.map(|s| s.to_string()),
        uploaded_at: Some(now.clone()),
        message_id: None,
        category: None,
        description: None,
        version: 1,
        is_latest: true,
        superseded_by: None,
        created_by_step: None,
        created_at: now,
        expires_at: None,
    });

    write_file_index(base_dir, conversation_id, &index)
}

/// Get a single uploaded file by ID.
pub fn get_uploaded_file(
    base_dir: &Path,
    id: &str,
) -> StorageResult<Option<serde_json::Value>> {
    // Need to scan all conversations since we don't know which one
    let conversations_dir = base_dir.join("conversations");
    if !conversations_dir.exists() {
        return Ok(None);
    }
    for entry in std::fs::read_dir(&conversations_dir)?.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let conv_id = entry.file_name().to_string_lossy().to_string();
        let index = read_file_index(base_dir, &conv_id)?;
        if let Some(file) = index.files.iter().find(|f| f.id == id && f.source == "upload") {
            return Ok(Some(upload_to_json(file)));
        }
    }
    Ok(None)
}

/// Get multiple uploaded files by their IDs.
pub fn get_uploaded_files_by_ids(
    base_dir: &Path,
    ids: &[String],
) -> StorageResult<Vec<serde_json::Value>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let conversations_dir = base_dir.join("conversations");
    if !conversations_dir.exists() {
        return Ok(Vec::new());
    }
    let id_set: std::collections::HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
    let mut results = Vec::new();

    for entry in std::fs::read_dir(&conversations_dir)?.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let conv_id = entry.file_name().to_string_lossy().to_string();
        let index = read_file_index(base_dir, &conv_id)?;
        for file in &index.files {
            if file.source == "upload" && id_set.contains(file.id.as_str()) {
                results.push(upload_to_json(file));
            }
        }
        if results.len() == ids.len() {
            break;
        }
    }
    Ok(results)
}

/// Get an uploaded file by ID, verified to belong to the given conversation.
pub fn get_uploaded_file_for_conversation(
    base_dir: &Path,
    id: &str,
    conversation_id: &str,
) -> StorageResult<Option<serde_json::Value>> {
    let index = read_file_index(base_dir, conversation_id)?;
    let file = index
        .files
        .iter()
        .find(|f| f.id == id && f.source == "upload");
    Ok(file.map(upload_to_json))
}

/// Get all uploaded files for a conversation.
pub fn get_uploaded_files_for_conversation(
    base_dir: &Path,
    conversation_id: &str,
) -> StorageResult<Vec<serde_json::Value>> {
    let index = read_file_index(base_dir, conversation_id)?;
    let mut results: Vec<serde_json::Value> = index
        .files
        .iter()
        .filter(|f| f.source == "upload")
        .map(upload_to_json)
        .collect();
    // Sort by uploaded_at descending
    results.sort_by(|a, b| {
        let a_t = a["uploadedAt"].as_str().unwrap_or("");
        let b_t = b["uploadedAt"].as_str().unwrap_or("");
        b_t.cmp(a_t)
    });
    Ok(results)
}

// ─── Generated Files ─────────────────────────────────────────────────────────

/// Insert a generated file record.
#[allow(clippy::too_many_arguments)]
pub fn insert_generated_file(
    base_dir: &Path,
    id: &str,
    conversation_id: &str,
    message_id: Option<&str>,
    file_name: &str,
    stored_path: &str,
    file_type: &str,
    file_size: i64,
    category: &str,
    description: Option<&str>,
    version: i32,
    is_latest: bool,
    superseded_by: Option<&str>,
    created_by_step: Option<i32>,
    expires_at: Option<&str>,
) -> StorageResult<()> {
    let mut index = read_file_index(base_dir, conversation_id)?;
    let now = Utc::now().to_rfc3339();

    index.files.push(FileEntry {
        id: id.to_string(),
        source: "generated".to_string(),
        file_name: file_name.to_string(),
        stored_path: stored_path.to_string(),
        file_type: file_type.to_string(),
        file_size,
        original_name: None,
        parsed_summary: None,
        uploaded_at: None,
        message_id: message_id.map(|s| s.to_string()),
        category: Some(category.to_string()),
        description: description.map(|s| s.to_string()),
        version,
        is_latest,
        superseded_by: superseded_by.map(|s| s.to_string()),
        created_by_step,
        created_at: now,
        expires_at: expires_at.map(|s| s.to_string()),
    });

    write_file_index(base_dir, conversation_id, &index)
}

/// Get all latest generated files for a conversation.
pub fn get_generated_files_for_conversation(
    base_dir: &Path,
    conversation_id: &str,
) -> StorageResult<Vec<serde_json::Value>> {
    let index = read_file_index(base_dir, conversation_id)?;
    let mut results: Vec<serde_json::Value> = index
        .files
        .iter()
        .filter(|f| f.source == "generated" && f.is_latest)
        .map(generated_to_json)
        .collect();
    // Sort by created_at descending
    results.sort_by(|a, b| {
        let a_t = a["createdAt"].as_str().unwrap_or("");
        let b_t = b["createdAt"].as_str().unwrap_or("");
        b_t.cmp(a_t)
    });
    Ok(results)
}

/// Get generated files by a list of IDs.
pub fn get_generated_files_by_ids(
    base_dir: &Path,
    ids: &[String],
) -> StorageResult<Vec<serde_json::Value>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let id_set: std::collections::HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
    let conversations_dir = base_dir.join("conversations");
    if !conversations_dir.exists() {
        return Ok(Vec::new());
    }
    let mut results = Vec::new();

    for entry in std::fs::read_dir(&conversations_dir)?.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let conv_id = entry.file_name().to_string_lossy().to_string();
        let index = read_file_index(base_dir, &conv_id)?;
        for file in &index.files {
            if file.source == "generated" && id_set.contains(file.id.as_str()) {
                results.push(generated_to_json(file));
            }
        }
        if results.len() == ids.len() {
            break;
        }
    }

    // Sort by created_at ascending
    results.sort_by(|a, b| {
        let a_t = a["createdAt"].as_str().unwrap_or("");
        let b_t = b["createdAt"].as_str().unwrap_or("");
        a_t.cmp(b_t)
    });
    Ok(results)
}

/// Get a single generated file by ID, verified to belong to the given conversation.
pub fn get_generated_file_for_conversation(
    base_dir: &Path,
    id: &str,
    conversation_id: &str,
) -> StorageResult<Option<serde_json::Value>> {
    let index = read_file_index(base_dir, conversation_id)?;
    let file = index
        .files
        .iter()
        .find(|f| f.id == id && f.source == "generated");
    Ok(file.map(generated_to_json))
}

/// Mark an existing file as superseded by a newer version.
pub fn mark_file_superseded(
    base_dir: &Path,
    old_id: &str,
    new_id: &str,
) -> StorageResult<()> {
    // Scan all conversations to find the file
    let conversations_dir = base_dir.join("conversations");
    if !conversations_dir.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(&conversations_dir)?.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let conv_id = entry.file_name().to_string_lossy().to_string();
        let mut index = read_file_index(base_dir, &conv_id)?;
        let mut changed = false;

        for file in &mut index.files {
            if file.id == old_id {
                file.is_latest = false;
                file.superseded_by = Some(new_id.to_string());
                changed = true;
                break;
            }
        }

        if changed {
            write_file_index(base_dir, &conv_id, &index)?;
            return Ok(());
        }
    }

    Ok(())
}

/// Find temporary files that have expired.
pub fn find_expired_temp_files(base_dir: &Path) -> StorageResult<Vec<serde_json::Value>> {
    let now = Utc::now().to_rfc3339();
    let conversations_dir = base_dir.join("conversations");
    if !conversations_dir.exists() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    for entry in std::fs::read_dir(&conversations_dir)?.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let conv_id = entry.file_name().to_string_lossy().to_string();
        let index = read_file_index(base_dir, &conv_id)?;

        for file in &index.files {
            if file.category.as_deref() == Some("temp") {
                if let Some(expires) = &file.expires_at {
                    if expires.as_str() < now.as_str() {
                        results.push(serde_json::json!({
                            "id": file.id,
                            "conversationId": conv_id,
                            "fileName": file.file_name,
                            "storedPath": file.stored_path,
                            "fileType": file.file_type,
                            "fileSize": file.file_size,
                            "category": file.category,
                            "expiresAt": file.expires_at,
                        }));
                    }
                }
            }
        }
    }
    Ok(results)
}

/// Delete a generated file record.
pub fn delete_generated_file(
    base_dir: &Path,
    id: &str,
) -> StorageResult<()> {
    let conversations_dir = base_dir.join("conversations");
    if !conversations_dir.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(&conversations_dir)?.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let conv_id = entry.file_name().to_string_lossy().to_string();
        let mut index = read_file_index(base_dir, &conv_id)?;
        let before = index.files.len();
        index.files.retain(|f| f.id != id);
        if index.files.len() < before {
            write_file_index(base_dir, &conv_id, &index)?;
            return Ok(());
        }
    }
    Ok(())
}

// ─── JSON conversion helpers ─────────────────────────────────────────────────

fn upload_to_json(file: &FileEntry) -> serde_json::Value {
    serde_json::json!({
        "id": file.id,
        "conversationId": "", // Not stored in file entry; set by caller if needed
        "originalName": file.original_name.as_deref().unwrap_or(&file.file_name),
        "storedPath": file.stored_path,
        "fileType": file.file_type,
        "fileSize": file.file_size,
        "parsedSummary": file.parsed_summary,
        "uploadedAt": file.uploaded_at.as_deref().unwrap_or(&file.created_at),
    })
}

fn generated_to_json(file: &FileEntry) -> serde_json::Value {
    serde_json::json!({
        "id": file.id,
        "conversationId": "", // Set by caller if needed
        "messageId": file.message_id,
        "fileName": file.file_name,
        "storedPath": file.stored_path,
        "fileType": file.file_type,
        "fileSize": file.file_size,
        "category": file.category,
        "description": file.description,
        "version": file.version,
        "isLatest": file.is_latest,
        "supersededBy": file.superseded_by,
        "createdByStep": file.created_by_step,
        "createdAt": file.created_at,
        "expiresAt": file.expires_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (PathBuf, TempDir) {
        let dir = TempDir::new().unwrap();
        let base = dir.path().to_path_buf();
        std::fs::create_dir_all(base.join("conversations")).unwrap();
        super::super::conversations::create_conversation(&base, "c1", "Test").unwrap();
        (base, dir)
    }

    #[test]
    fn test_uploaded_file_crud() {
        let (base, _dir) = setup();

        insert_uploaded_file(
            &base, "uf1", "c1", "data.csv", "/tmp/data.csv", "csv", 512,
            Some("100 rows"),
        )
        .unwrap();

        let file = get_uploaded_file_for_conversation(&base, "uf1", "c1").unwrap();
        assert!(file.is_some());
        assert_eq!(file.as_ref().unwrap()["originalName"], "data.csv");
    }

    #[test]
    fn test_generated_file_crud() {
        let (base, _dir) = setup();

        insert_generated_file(
            &base, "gf1", "c1", None, "report.pdf", "/tmp/report.pdf", "pdf",
            1024, "report", Some("Monthly report"), 1, true, None, Some(3), None,
        )
        .unwrap();

        let files = get_generated_files_for_conversation(&base, "c1").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["fileName"], "report.pdf");
    }

    #[test]
    fn test_file_supersession() {
        let (base, _dir) = setup();

        insert_generated_file(
            &base, "f1", "c1", None, "data.csv", "/tmp/v1.csv", "csv",
            100, "data", None, 1, true, None, None, None,
        )
        .unwrap();
        insert_generated_file(
            &base, "f2", "c1", None, "data.csv", "/tmp/v2.csv", "csv",
            200, "data", None, 2, true, None, None, None,
        )
        .unwrap();
        mark_file_superseded(&base, "f1", "f2").unwrap();

        let files = get_generated_files_for_conversation(&base, "c1").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["id"], "f2");
    }

    #[test]
    fn test_conversation_isolation() {
        let (base, _dir) = setup();
        super::super::conversations::create_conversation(&base, "c2", "Test 2").unwrap();

        insert_uploaded_file(
            &base, "uf1", "c1", "data1.csv", "/tmp/d1.csv", "csv", 100, None,
        )
        .unwrap();
        insert_uploaded_file(
            &base, "uf2", "c2", "data2.csv", "/tmp/d2.csv", "csv", 200, None,
        )
        .unwrap();

        // c1 should only see uf1
        let f1 = get_uploaded_file_for_conversation(&base, "uf1", "c1").unwrap();
        assert!(f1.is_some());
        let f2 = get_uploaded_file_for_conversation(&base, "uf1", "c2").unwrap();
        assert!(f2.is_none());
    }
}
