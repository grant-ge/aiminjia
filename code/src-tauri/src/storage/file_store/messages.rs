//! Message storage with sharding and append-only updates.
//!
//! Messages are stored in numbered JSONL shard files: `messages.1.jsonl`,
//! `messages.2.jsonl`, etc. Each shard holds up to `SHARD_CAPACITY` messages.
//!
//! The `_current` file tracks the active shard: `"{shard_num}:{next_seq}"`.
//!
//! Message updates use append-only semantics: a new record with the same `seq`
//! but a higher `_rev` is appended. On read, only the highest `_rev` per `seq`
//! is kept.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;
use log::warn;

use super::conversations::conv_dir;
use super::error::{StorageError, StorageResult};
use super::io::{append_jsonl, count_jsonl_lines, read_jsonl};
use super::types::StoredMessage;

/// Maximum messages per shard file.
const SHARD_CAPACITY: u64 = 100;

// ─── Shard metadata (_current file) ──────────────────────────────────────────

/// Shard metadata: `(shard_number, next_sequence_number)`.
#[derive(Debug, Clone)]
struct ShardMeta {
    shard: u64,
    next_seq: u64,
}

impl ShardMeta {
    fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.trim().split(':').collect();
        if parts.len() == 2 {
            let shard = parts[0].parse::<u64>().ok()?;
            let next_seq = parts[1].parse::<u64>().ok()?;
            Some(Self { shard, next_seq })
        } else {
            None
        }
    }

    fn to_string(&self) -> String {
        format!("{}:{}", self.shard, self.next_seq)
    }
}

fn current_path(base_dir: &Path, conversation_id: &str) -> PathBuf {
    conv_dir(base_dir, conversation_id).join("_current")
}

fn shard_path(base_dir: &Path, conversation_id: &str, shard_num: u64) -> PathBuf {
    conv_dir(base_dir, conversation_id).join(format!("messages.{}.jsonl", shard_num))
}

fn read_shard_meta(base_dir: &Path, conversation_id: &str) -> ShardMeta {
    let path = current_path(base_dir, conversation_id);
    match fs::read_to_string(&path) {
        Ok(content) => ShardMeta::parse(&content).unwrap_or(ShardMeta {
            shard: 1,
            next_seq: 1,
        }),
        Err(_) => ShardMeta {
            shard: 1,
            next_seq: 1,
        },
    }
}

fn write_shard_meta(
    base_dir: &Path,
    conversation_id: &str,
    meta: &ShardMeta,
) -> io::Result<()> {
    let path = current_path(base_dir, conversation_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, meta.to_string())
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Insert a new message into the conversation.
///
/// Automatically creates new shards when the current one reaches capacity.
pub fn insert_message(
    base_dir: &Path,
    id: &str,
    conversation_id: &str,
    role: &str,
    content_json: &str,
) -> StorageResult<()> {
    let mut meta = read_shard_meta(base_dir, conversation_id);

    // Check if current shard is full
    let current_shard_path = shard_path(base_dir, conversation_id, meta.shard);
    let current_count = count_jsonl_lines(&current_shard_path).unwrap_or(0) as u64;
    if current_count >= SHARD_CAPACITY {
        meta.shard += 1;
    }

    // Parse content JSON
    let content: serde_json::Value =
        serde_json::from_str(content_json).unwrap_or(serde_json::json!({}));

    let msg = StoredMessage {
        seq: meta.next_seq,
        rev: 0,
        id: id.to_string(),
        conversation_id: conversation_id.to_string(),
        role: role.to_string(),
        content,
        created_at: Utc::now().to_rfc3339(),
    };

    let path = shard_path(base_dir, conversation_id, meta.shard);
    append_jsonl(&path, &msg)?;

    // Update _current
    meta.next_seq += 1;
    write_shard_meta(base_dir, conversation_id, &meta)?;

    // Update conversation's updatedAt
    let conv_meta_path = conv_dir(base_dir, conversation_id).join("conv.json");
    if conv_meta_path.exists() {
        if let Ok(mut conv) =
            super::io::read_json_safe::<super::types::ConversationMeta>(&conv_meta_path)
        {
            conv.updated_at = Utc::now().to_rfc3339();
            let _ = super::io::atomic_write_json(&conv_meta_path, &conv);
        }
    }

    Ok(())
}

/// Get all messages in a conversation, ordered chronologically.
///
/// Reads all shards and deduplicates (keeping highest `_rev` per `seq`).
pub fn get_messages(
    base_dir: &Path,
    conversation_id: &str,
) -> StorageResult<Vec<serde_json::Value>> {
    let meta = read_shard_meta(base_dir, conversation_id);
    let mut all_msgs: Vec<StoredMessage> = Vec::new();

    // Read all shards from 1 to current
    for shard_num in 1..=meta.shard {
        let path = shard_path(base_dir, conversation_id, shard_num);
        match read_jsonl::<StoredMessage>(&path) {
            Ok(records) => all_msgs.extend(records),
            Err(e) => {
                warn!(
                    "Failed to read shard {} for {}: {}",
                    shard_num, conversation_id, e
                );
            }
        }
    }

    let deduped = dedup_messages(all_msgs);
    Ok(deduped.into_iter().map(message_to_json).collect())
}

/// Get the most recent N messages for a conversation.
///
/// Reads shards in reverse order, stopping once we have enough messages.
pub fn get_recent_messages(
    base_dir: &Path,
    conversation_id: &str,
    limit: u32,
) -> StorageResult<Vec<serde_json::Value>> {
    let meta = read_shard_meta(base_dir, conversation_id);
    let limit = limit as usize;
    let mut all_msgs: Vec<StoredMessage> = Vec::new();

    // Read shards in reverse order until we have enough unique seqs
    for shard_num in (1..=meta.shard).rev() {
        let path = shard_path(base_dir, conversation_id, shard_num);
        match read_jsonl::<StoredMessage>(&path) {
            Ok(records) => all_msgs.extend(records),
            Err(_) => continue,
        }

        // Count unique seqs to check if we have enough (avoid full dedup)
        let unique_seqs: std::collections::HashSet<u64> =
            all_msgs.iter().map(|m| m.seq).collect();
        if unique_seqs.len() >= limit {
            break;
        }
    }

    let mut deduped = dedup_messages(all_msgs);
    // Take only the last `limit` messages
    let start = deduped.len().saturating_sub(limit);
    let recent: Vec<serde_json::Value> = deduped
        .drain(start..)
        .map(message_to_json)
        .collect();

    Ok(recent)
}

/// Update the content of an existing message (append-only).
///
/// Finds the message by ID within the specified conversation, then appends
/// a new record with the same `seq` but incremented `_rev`.
pub fn update_message_content(
    base_dir: &Path,
    id: &str,
    conversation_id: &str,
    content_json: &str,
) -> StorageResult<()> {
    let meta = read_shard_meta(base_dir, conversation_id);

    for shard_num in 1..=meta.shard {
        let path = shard_path(base_dir, conversation_id, shard_num);
        let records: Vec<StoredMessage> = match read_jsonl(&path) {
            Ok(r) => r,
            Err(_) => continue,
        };

        // Find the message
        if let Some(original) = records.iter().rev().find(|m| m.id == id) {
            let content: serde_json::Value =
                serde_json::from_str(content_json).unwrap_or(serde_json::json!({}));

            // Find the max rev for this seq
            let max_rev = records
                .iter()
                .filter(|m| m.seq == original.seq)
                .map(|m| m.rev)
                .max()
                .unwrap_or(0);

            let updated = StoredMessage {
                seq: original.seq,
                rev: max_rev + 1,
                id: original.id.clone(),
                conversation_id: original.conversation_id.clone(),
                role: original.role.clone(),
                content,
                created_at: original.created_at.clone(),
            };

            // Append to the current active shard (not the one where original is)
            let active_path = shard_path(base_dir, conversation_id, meta.shard);
            append_jsonl(&active_path, &updated)?;

            return Ok(());
        }
    }

    Err(StorageError::not_found(format!(
        "Message not found: {}",
        id
    )))
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Deduplicate messages: keep only the highest `_rev` per `seq`.
fn dedup_messages(messages: Vec<StoredMessage>) -> Vec<StoredMessage> {
    // Build a map: seq → message with highest rev
    let mut best: HashMap<u64, StoredMessage> = HashMap::new();
    for msg in messages {
        match best.entry(msg.seq) {
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(msg);
            }
            std::collections::hash_map::Entry::Occupied(mut e) => {
                if msg.rev > e.get().rev {
                    *e.get_mut() = msg;
                }
            }
        }
    }

    // Sort by seq ascending (chronological order)
    let mut result: Vec<StoredMessage> = best.into_values().collect();
    result.sort_by_key(|m| m.seq);
    result
}

/// Convert a StoredMessage to the JSON format expected by the frontend.
fn message_to_json(msg: StoredMessage) -> serde_json::Value {
    serde_json::json!({
        "id": msg.id,
        "conversationId": msg.conversation_id,
        "role": msg.role,
        "content": msg.content,
        "createdAt": msg.created_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (PathBuf, TempDir) {
        let dir = TempDir::new().unwrap();
        let base = dir.path().to_path_buf();
        fs::create_dir_all(base.join("conversations")).unwrap();
        // Create a conversation directory
        super::super::conversations::create_conversation(&base, "c1", "Test").unwrap();
        (base, dir)
    }

    #[test]
    fn test_insert_and_get_messages() {
        let (base, _dir) = setup();

        insert_message(&base, "m1", "c1", "user", r#"{"text":"hello"}"#).unwrap();
        insert_message(&base, "m2", "c1", "assistant", r#"{"text":"hi"}"#).unwrap();

        let msgs = get_messages(&base, "c1").unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"]["text"], "hello");
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["content"]["text"], "hi");
    }

    #[test]
    fn test_message_sharding() {
        let (base, _dir) = setup();

        // Insert more than SHARD_CAPACITY messages
        for i in 0..150 {
            insert_message(
                &base,
                &format!("m{}", i),
                "c1",
                "user",
                &format!(r#"{{"text":"msg {}"}}"#, i),
            )
            .unwrap();
        }

        // Should have created 2 shards
        assert!(shard_path(&base, "c1", 1).exists());
        assert!(shard_path(&base, "c1", 2).exists());

        let msgs = get_messages(&base, "c1").unwrap();
        assert_eq!(msgs.len(), 150);
    }

    #[test]
    fn test_recent_messages() {
        let (base, _dir) = setup();

        for i in 0..10 {
            insert_message(
                &base,
                &format!("m{}", i),
                "c1",
                "user",
                &format!(r#"{{"text":"msg {}"}}"#, i),
            )
            .unwrap();
        }

        let recent = get_recent_messages(&base, "c1", 3).unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0]["content"]["text"], "msg 7");
        assert_eq!(recent[2]["content"]["text"], "msg 9");
    }

    #[test]
    fn test_update_message_dedup() {
        let (base, _dir) = setup();

        insert_message(&base, "m1", "c1", "user", r#"{"text":"original"}"#).unwrap();
        insert_message(&base, "m2", "c1", "assistant", r#"{"text":"reply"}"#).unwrap();

        // Update m1's content
        update_message_content(&base, "m1", "c1", r#"{"text":"updated"}"#).unwrap();

        let msgs = get_messages(&base, "c1").unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["content"]["text"], "updated"); // Updated content
        assert_eq!(msgs[1]["content"]["text"], "reply");
    }

    #[test]
    fn test_shard_meta_parse() {
        let meta = ShardMeta::parse("3:247").unwrap();
        assert_eq!(meta.shard, 3);
        assert_eq!(meta.next_seq, 247);

        assert!(ShardMeta::parse("invalid").is_none());
        assert!(ShardMeta::parse("").is_none());
    }
}
