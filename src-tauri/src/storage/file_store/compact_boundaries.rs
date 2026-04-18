use std::path::{Path, PathBuf};

use super::conversations::conv_dir;
use super::error::StorageResult;
use super::io::{append_jsonl, read_jsonl};
use crate::runtime::chat::compaction::CompactBoundaryRecord;

fn compact_boundaries_path(base_dir: &Path, conversation_id: &str) -> PathBuf {
    conv_dir(base_dir, conversation_id).join("compact_boundaries.jsonl")
}

pub fn append_compact_boundary(
    base_dir: &Path,
    record: &CompactBoundaryRecord,
) -> StorageResult<()> {
    append_jsonl(
        &compact_boundaries_path(base_dir, &record.conversation_id),
        record,
    )?;
    Ok(())
}

pub fn list_compact_boundaries(
    base_dir: &Path,
    conversation_id: &str,
) -> StorageResult<Vec<CompactBoundaryRecord>> {
    Ok(read_jsonl(&compact_boundaries_path(base_dir, conversation_id))?)
}
