//! pending.json read/write — see spec §4.3.

use std::io;
use std::path::Path;

use crate::storage::file_store::io::atomic_write_json;

use super::types::{PendingFileFormat, PendingItem};

const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Read pending items from `pending.json`.
///
/// Missing file → `Ok(empty vec)`. Corrupt JSON or wrong schema → `Ok(empty vec)` + warn log.
pub fn read_pending(path: &Path) -> io::Result<Vec<PendingItem>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("[pending] cannot read {}: {}", path.display(), e);
            return Ok(Vec::new());
        }
    };
    match serde_json::from_str::<PendingFileFormat>(&content) {
        Ok(f) if f.schema_version == CURRENT_SCHEMA_VERSION => Ok(f.items),
        Ok(f) => {
            log::warn!(
                "[pending] schema {} != current {} at {}; ignoring",
                f.schema_version,
                CURRENT_SCHEMA_VERSION,
                path.display()
            );
            Ok(Vec::new())
        }
        Err(e) => {
            log::warn!("[pending] corrupt {}: {}; ignoring", path.display(), e);
            Ok(Vec::new())
        }
    }
}

/// Atomically write pending items.
pub fn write_pending(path: &Path, items: &[PendingItem]) -> io::Result<()> {
    let f = PendingFileFormat {
        schema_version: CURRENT_SCHEMA_VERSION,
        items: items.to_vec(),
    };
    atomic_write_json(path, &f)
}

/// Scan a conversations root dir and return `(conversation_id, items)` for
/// every conversation directory that has a non-empty `pending.json`.
///
/// Caller is responsible for filtering archived conversations.
pub fn scan_conversation_pending(
    conversations_root: &Path,
) -> io::Result<Vec<(String, Vec<PendingItem>)>> {
    let mut out = Vec::new();
    if !conversations_root.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(conversations_root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(conv_id) = path.file_name().and_then(|s| s.to_str()).map(String::from) else {
            continue;
        };
        let pending_path = path.join("pending.json");
        if !pending_path.exists() {
            continue;
        }
        let items = read_pending(&pending_path)?;
        if !items.is_empty() {
            out.push((conv_id, items));
        }
    }
    Ok(out)
}
