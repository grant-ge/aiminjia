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

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;
use log::warn;

use super::conversations::conv_dir;
use super::error::{StorageError, StorageResult};
use super::io::{append_jsonl, count_jsonl_lines, read_jsonl};
use super::types::StoredMessage;

use crate::llm::content_filter::strip_hallucinated_xml;

/// Maximum messages per shard file.
const SHARD_CAPACITY: u64 = 100;

fn messages_path(base_dir: &Path, conversation_id: &str) -> PathBuf {
    conv_dir(base_dir, conversation_id).join("messages.jsonl")
}

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
    // Try main _current file, then .tmp fallback (crash during atomic rename)
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => {
            let tmp = path.with_extension("tmp");
            match fs::read_to_string(&tmp) {
                Ok(c) => {
                    warn!("Recovered _current from .tmp for {}", conversation_id);
                    // Promote .tmp → _current
                    let _ = fs::rename(&tmp, &path);
                    c
                }
                Err(_) => String::new(),
            }
        }
    };
    match ShardMeta::parse(&content) {
        Some(meta) => meta,
        None => ShardMeta {
            shard: 1,
            next_seq: 1,
        },
    }
}

fn write_shard_meta(base_dir: &Path, conversation_id: &str, meta: &ShardMeta) -> io::Result<()> {
    let path = current_path(base_dir, conversation_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Atomic write: write to .tmp then rename to prevent data loss on crash
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, meta.to_string())?;
    fs::rename(&tmp, &path)?;
    Ok(())
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
        write_shard_meta(base_dir, conversation_id, &meta)?;
    }

    let shard_path = shard_path(base_dir, conversation_id, meta.shard);
    meta.next_seq += 1;

    let content: serde_json::Value = serde_json::from_str(content_json)?;

    let record = StoredMessage {
        seq: Some(meta.next_seq),
        rev: Some(1),
        tool_calls: None,
        tool_call_id: None,
        name: None,
        run_id: None,
        schema_version: None,
        sequence: None,
        id: id.to_string(),
        conversation_id: conversation_id.to_string(),
        role: role.to_string(),
        content,
        created_at: Utc::now().to_rfc3339(),
    };

    append_jsonl(&shard_path, &record)?;
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

/// Insert a message into the single-file transcript store.
///
/// Duplicate ids use append-only last-writer-wins semantics on read.
pub fn insert_message_v2(base_dir: &Path, msg: &StoredMessage) -> StorageResult<()> {
    let path = messages_path(base_dir, &msg.conversation_id);
    append_jsonl(&path, msg)?;

    let conv_meta_path = conv_dir(base_dir, &msg.conversation_id).join("conv.json");
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

/// Read all single-file messages using id-based last-writer-wins semantics.
pub fn get_messages_v2(base_dir: &Path, conversation_id: &str) -> StorageResult<Vec<StoredMessage>> {
    let path = messages_path(base_dir, conversation_id);
    let all: Vec<StoredMessage> = read_jsonl(&path)?;
    let mut by_id: HashMap<String, StoredMessage> = HashMap::new();

    for msg in all {
        by_id.insert(msg.id.clone(), msg);
    }

    let mut result: Vec<StoredMessage> = by_id.into_values().collect();
    result.sort_by(|a, b| {
        match (a.sequence, b.sequence) {
            (Some(a_seq), Some(b_seq)) => a_seq
                .cmp(&b_seq)
                .then_with(|| a.created_at.cmp(&b.created_at))
                .then_with(|| a.id.cmp(&b.id)),
            _ => a
                .created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id)),
        }
    });
    Ok(result)
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
        let unique_message_keys: HashSet<String> = all_msgs.iter().map(message_dedup_key).collect();
        if unique_message_keys.len() >= limit {
            break;
        }
    }

    let mut deduped = dedup_messages(all_msgs);
    // Take only the last `limit` messages
    let start = deduped.len().saturating_sub(limit);
    let recent: Vec<serde_json::Value> = deduped.drain(start..).map(message_to_json).collect();

    Ok(recent)
}

/// Read the most recent N single-file messages after id-based dedup.
pub fn get_recent_messages_v2(
    base_dir: &Path,
    conversation_id: &str,
    limit: usize,
) -> StorageResult<Vec<StoredMessage>> {
    let all = get_messages_v2(base_dir, conversation_id)?;
    let start = all.len().saturating_sub(limit);
    Ok(all.into_iter().skip(start).collect())
}

/// Migrate legacy sharded message files into the single-file transcript.
pub fn migrate_shards_to_single_file(base_dir: &Path, conversation_id: &str) -> StorageResult<()> {
    let dir = conv_dir(base_dir, conversation_id);
    let mut shards: Vec<(u32, PathBuf)> = fs::read_dir(&dir)?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("messages.") || !name.ends_with(".jsonl") || name == "messages.jsonl" {
                return None;
            }
            let middle = &name["messages.".len()..name.len() - ".jsonl".len()];
            middle.parse::<u32>().ok().map(|n| (n, entry.path()))
        })
        .collect();

    if shards.is_empty() {
        return Ok(());
    }

    shards.sort_by_key(|(num, _)| *num);

    let target = messages_path(base_dir, conversation_id);
    let mut next_sequence = 0_u64;

    for (_, shard) in &shards {
        let records: Vec<StoredMessage> = read_jsonl(shard)?;
        for mut msg in records {
            next_sequence += 1;
            msg.sequence = Some(next_sequence);
            msg.seq = None;
            msg.rev = None;
            append_jsonl(&target, &msg)?;
        }
    }

    for (_, shard) in shards {
        let _ = fs::remove_file(shard);
    }
    let _ = fs::remove_file(dir.join("_current"));
    let _ = fs::remove_file(dir.join("_current.tmp"));

    Ok(())
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
    let mut all_records: Vec<StoredMessage> = Vec::new();

    for shard_num in 1..=meta.shard {
        let path = shard_path(base_dir, conversation_id, shard_num);
        match read_jsonl(&path) {
            Ok(records) => all_records.extend(records),
            Err(_) => continue,
        }
    }

    // Use the last persisted version across all shards so repeated updates on
    // transitional missing-seq records continue from the latest content/rev.
    let Some(original) = all_records.iter().rev().find(|m| m.id == id) else {
        return Err(StorageError::not_found(format!(
            "Message not found: {}",
            id
        )));
    };

    let content: serde_json::Value =
        serde_json::from_str(content_json).unwrap_or(serde_json::json!({}));
    let original_key = message_dedup_key(original);

    let max_rev = all_records
        .iter()
        .filter(|m| message_dedup_key(m) == original_key)
        .map(|m| m.rev_or(0))
        .max()
        .unwrap_or(0);

    let updated = StoredMessage {
        seq: original.seq,
        rev: Some(max_rev + 1),
        tool_calls: original.tool_calls.clone(),
        tool_call_id: original.tool_call_id.clone(),
        name: original.name.clone(),
        run_id: original.run_id.clone(),
        schema_version: original.schema_version,
        sequence: original.sequence,
        id: original.id.clone(),
        conversation_id: original.conversation_id.clone(),
        role: original.role.clone(),
        content,
        created_at: original.created_at.clone(),
    };

    // Append to the current active shard (not necessarily where original was).
    let active_path = shard_path(base_dir, conversation_id, meta.shard);
    append_jsonl(&active_path, &updated)?;

    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Deduplicate messages: keep only the highest `_rev` per `seq`.
fn dedup_messages(messages: Vec<StoredMessage>) -> Vec<StoredMessage> {
    // Prefer legacy shard seq for dedup; if missing during transition, fall back
    // to id so malformed records do not collapse into one bucket.
    let mut best: HashMap<String, StoredMessage> = HashMap::new();
    for msg in messages {
        let key = message_dedup_key(&msg);
        match best.entry(key) {
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(msg);
            }
            std::collections::hash_map::Entry::Occupied(mut e) => {
                if msg.rev_or(0) > e.get().rev_or(0) {
                    *e.get_mut() = msg;
                }
            }
        }
    }

    // Sort by legacy seq when available; for malformed transitional records
    // without seq, fall back to stable chronological fields instead of HashMap
    // iteration order.
    let mut result: Vec<StoredMessage> = best.into_values().collect();
    result.sort_by(|a, b| match (a.seq, b.seq) {
        (Some(a_seq), Some(b_seq)) => a_seq.cmp(&b_seq),
        _ => a
            .created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.cmp(&b.id)),
    });
    result
}

/// Convert a StoredMessage to the JSON format expected by the frontend.
fn message_to_json(msg: StoredMessage) -> serde_json::Value {
    // Extract sender from content if present (for user messages)
    let sender = msg.content.get("sender").cloned();

    if msg.role == "tool" {
        // Tool records may come from legacy content-embedded fields or the new
        // top-level schema fields; the frontend still expects `toolResult`.
        let tool_call_id = msg
            .tool_call_id
            .clone()
            .map(serde_json::Value::String)
            .or_else(|| msg.content.get("toolCallId").cloned())
            .unwrap_or(serde_json::Value::Null);
        let name = msg
            .name
            .clone()
            .map(serde_json::Value::String)
            .or_else(|| msg.content.get("name").cloned())
            .unwrap_or(serde_json::Value::Null);
        let content_text = msg
            .content
            .get("content")
            .cloned()
            .or_else(|| msg.content.get("text").cloned())
            .unwrap_or(serde_json::Value::Null);
        let is_error = msg.content.get("isError").cloned().unwrap_or(serde_json::json!(false));
        let duration_ms = msg.content.get("durationMs").cloned();
        let mut tool_result = serde_json::json!({
            "toolCallId": tool_call_id,
            "name": name,
            "content": content_text,
            "isError": is_error,
        });
        if let Some(d) = duration_ms {
            tool_result["durationMs"] = d;
        }
        return serde_json::json!({
            "id": msg.id,
            "conversationId": msg.conversation_id,
            "role": "tool",
            "content": {},
            "toolResult": tool_result,
            "createdAt": msg.created_at,
        });
    }

    // Sanitize assistant message text: strip hallucinated XML tags from historical data
    let content = if msg.role == "assistant" {
        sanitize_assistant_content(msg.content)
    } else {
        msg.content
    };

    // Assistant tool calls may also come from the new top-level schema fields.
    let tool_calls = if msg.role == "assistant" {
        msg.tool_calls.clone().map(serde_json::Value::Array).or_else(|| content.get("toolCalls").cloned())
    } else {
        None
    };

    let mut out = serde_json::json!({
        "id": msg.id,
        "conversationId": msg.conversation_id,
        "role": msg.role,
        "content": content,
        "createdAt": msg.created_at,
        "sender": sender,
    });
    if let Some(tcs) = tool_calls {
        if tcs.as_array().map_or(false, |a| !a.is_empty()) {
            out["toolCalls"] = tcs;
        }
    }
    out
}

fn message_dedup_key(msg: &StoredMessage) -> String {
    if let Some(seq) = msg.seq {
        format!("seq:{seq}")
    } else {
        format!("id:{}", msg.id)
    }
}

/// Strip hallucinated XML from assistant message content.text field.
fn sanitize_assistant_content(mut content: serde_json::Value) -> serde_json::Value {
    if let Some(text) = content.get("text").and_then(|t| t.as_str()) {
        let cleaned = strip_hallucinated_xml(text);
        if cleaned.len() != text.len() {
            content["text"] = serde_json::Value::String(cleaned);
        }
    }
    content
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
    fn shard_storage_keeps_seq_and_rev_on_disk_for_dedup() {
        let (base, _dir) = setup();

        insert_message(&base, "m1", "c1", "user", r#"{"text":"original"}"#).unwrap();
        update_message_content(&base, "m1", "c1", r#"{"text":"updated"}"#).unwrap();

        let msgs = get_messages(&base, "c1").unwrap();
        assert_eq!(msgs.len(), 1, "dedup should still collapse updated shard records");
        assert_eq!(msgs[0]["content"]["text"], "updated");

        let shard_raw = fs::read_to_string(shard_path(&base, "c1", 1)).unwrap();
        let lines: Vec<&str> = shard_raw.lines().collect();
        assert_eq!(lines.len(), 2, "append-only shard should keep original and updated record");

        for line in lines {
            let json_line = line.trim_end_matches("\t\u{2713}");
            let json: serde_json::Value = serde_json::from_str(json_line).unwrap();
            assert!(json.get("seq").is_some(), "shard record must persist seq");
            assert!(json.get("_rev").is_some(), "shard record must persist _rev");
        }
    }

    #[test]
    fn test_shard_meta_parse() {
        let meta = ShardMeta::parse("3:247").unwrap();
        assert_eq!(meta.shard, 3);
        assert_eq!(meta.next_seq, 247);

        assert!(ShardMeta::parse("invalid").is_none());
        assert!(ShardMeta::parse("").is_none());
    }

    #[test]
    fn insert_message_returns_error_when_rollover_shard_meta_write_fails() {
        let (base, _dir) = setup();

        for i in 0..SHARD_CAPACITY {
            insert_message(
                &base,
                &format!("m{}", i),
                "c1",
                "user",
                &format!(r#"{{"text":"msg {}"}}"#, i),
            )
            .unwrap();
        }

        let blocked_tmp = current_path(&base, "c1").with_extension("tmp");
        fs::create_dir_all(&blocked_tmp).unwrap();

        let err = insert_message(&base, "m-overflow", "c1", "user", r#"{"text":"overflow"}"#)
            .expect_err("rollover must surface shard metadata write failure");
        assert!(matches!(err, StorageError::Io(_)));
        assert!(
            !shard_path(&base, "c1", 2).exists(),
            "next shard should not be created when rollover metadata write fails"
        );
    }

    #[test]
    fn insert_message_returns_error_when_final_shard_meta_write_fails() {
        let (base, _dir) = setup();

        let blocked_tmp = current_path(&base, "c1").with_extension("tmp");
        fs::create_dir_all(&blocked_tmp).unwrap();

        let err = insert_message(&base, "m1", "c1", "user", r#"{"text":"hello"}"#)
            .expect_err("final shard metadata write failure must be surfaced");
        assert!(matches!(err, StorageError::Io(_)));
    }
}
