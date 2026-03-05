//! Foundation I/O utilities for file-based storage.
//!
//! Provides atomic JSON writes, JSONL append/read with integrity markers,
//! auto-splitting for large JSONL files, and PID-based file locks.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Completion marker appended to each JSONL line to indicate a complete write.
/// Lines without this marker are discarded on read (incomplete/crashed writes).
const LINE_COMPLETE: &str = "\t\u{2713}";

// ─── Atomic JSON read/write ───────────────────────────────────────────────

/// Atomically write a JSON file.
///
/// 1. If the target file exists, copy it to `{path}.bak`
/// 2. Serialize data to `{path}.tmp`
/// 3. Rename `{path}.tmp` → `{path}` (atomic on most filesystems)
pub fn atomic_write_json<T: Serialize>(path: &Path, data: &T) -> io::Result<()> {
    let content = serde_json::to_string_pretty(data)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Backup existing file
    if path.exists() {
        let bak = append_ext(path, "bak");
        let _ = fs::copy(path, &bak);
    }

    // Write to temp file, then rename
    let tmp = append_ext(path, "tmp");
    fs::write(&tmp, content.as_bytes())?;
    fs::rename(&tmp, path)?;

    Ok(())
}

/// Read a JSON file with fallback to `.bak` if the main file is corrupted.
pub fn read_json_safe<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    // Try main file first
    match read_json_file(path) {
        Ok(data) => return Ok(data),
        Err(e) => {
            log::warn!("Failed to read {:?}: {}, trying .bak", path, e);
        }
    }

    // Fallback to .bak
    let bak = append_ext(path, "bak");
    if bak.exists() {
        match read_json_file(&bak) {
            Ok(data) => {
                log::info!("Recovered from .bak: {:?}", path);
                // Restore main file from backup
                let _ = fs::copy(&bak, path);
                return Ok(data);
            }
            Err(e) => {
                log::error!("Both main and .bak corrupted for {:?}: {}", path, e);
            }
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("File corrupted and no backup available: {:?}", path),
    ))
}

/// Read and deserialize a JSON file.
fn read_json_file<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    let content = fs::read_to_string(path)?;
    serde_json::from_str(&content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Read a JSON file, returning `None` if the file doesn't exist.
pub fn read_json_optional<T: DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    read_json_safe(path).map(Some)
}

// ─── JSONL read/write ─────────────────────────────────────────────────────

/// Append a single record to a JSONL file with the completion marker.
///
/// Each line is: `{json}\t✓\n`
/// The `\t✓` suffix marks the line as completely written. Lines without it
/// (e.g., from a crash mid-write) are discarded on read.
pub fn append_jsonl<T: Serialize>(path: &Path, record: &T) -> io::Result<()> {
    let json = serde_json::to_string(record)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let line = format!("{}{}\n", json, LINE_COMPLETE);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.as_bytes())?;
    file.flush()?;

    Ok(())
}

/// Read all valid records from a JSONL file.
///
/// Lines without the `\t✓` completion marker are silently discarded.
pub fn read_jsonl<T: DeserializeOwned>(path: &Path) -> io::Result<Vec<T>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if let Some(json_str) = strip_completion_marker(&line) {
            match serde_json::from_str::<T>(json_str) {
                Ok(record) => records.push(record),
                Err(e) => {
                    log::warn!("Skipping malformed JSONL line in {:?}: {}", path, e);
                }
            }
        }
        // Lines without marker are silently skipped (incomplete writes)
    }

    Ok(records)
}

/// Read the last N valid records from a JSONL file (reverse order read).
///
/// Reads from the end of the file for efficiency. Returns records in
/// chronological order (oldest first).
pub fn read_jsonl_tail<T: DeserializeOwned>(path: &Path, n: usize) -> io::Result<Vec<T>> {
    if !path.exists() || n == 0 {
        return Ok(Vec::new());
    }

    // Read all lines, take last N valid ones
    // For files under ~10MB this is simpler and fast enough.
    // For larger files we'd want a reverse-seek approach.
    let all = read_jsonl::<T>(path)?;
    let start = all.len().saturating_sub(n);
    Ok(all.into_iter().skip(start).collect())
}

/// Count valid lines (with completion marker) in a JSONL file.
pub fn count_jsonl_lines(path: &Path) -> io::Result<usize> {
    if !path.exists() {
        return Ok(0);
    }

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut count = 0;
    for line in reader.lines() {
        let line = line?;
        if strip_completion_marker(&line).is_some() {
            count += 1;
        }
    }
    Ok(count)
}

/// Get file size in bytes, returning 0 if the file doesn't exist.
pub fn file_size_bytes(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

// ─── Auto-splitting JSONL ─────────────────────────────────────────────────

/// Append a record to a JSONL file with automatic splitting.
///
/// When the current file exceeds `max_bytes`, it is renamed to
/// `{name}.{N}.jsonl` (where N is the next shard number) and a new
/// empty file is created.
///
/// Shard naming: `memory.jsonl` → `memory.1.jsonl`, `memory.2.jsonl`, etc.
/// The unnumbered file is always the current (active) one.
pub fn append_jsonl_with_split<T: Serialize>(
    path: &Path,
    record: &T,
    max_bytes: u64,
) -> io::Result<()> {
    // Check if we need to split
    if path.exists() && file_size_bytes(path) >= max_bytes {
        rotate_jsonl_shard(path)?;
    }

    append_jsonl(path, record)
}

/// Read all records from a base path and all its numbered shards.
///
/// Reads in order: `{name}.1.jsonl`, `{name}.2.jsonl`, ..., `{name}.jsonl` (current).
pub fn read_all_jsonl_shards<T: DeserializeOwned>(base_path: &Path) -> io::Result<Vec<T>> {
    let shard_paths = list_shard_paths(base_path);
    let mut all_records = Vec::new();

    for shard_path in shard_paths {
        let records = read_jsonl::<T>(&shard_path)?;
        all_records.extend(records);
    }

    Ok(all_records)
}

/// List all shard paths for a given base JSONL file, in order.
///
/// Returns `[base.1.jsonl, base.2.jsonl, ..., base.jsonl]`.
fn list_shard_paths(base_path: &Path) -> Vec<PathBuf> {
    let parent = base_path.parent().unwrap_or(Path::new("."));
    let stem = base_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();

    let mut numbered: Vec<(u32, PathBuf)> = Vec::new();

    if let Ok(entries) = fs::read_dir(parent) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Match pattern: {stem}.{N}.jsonl
            if let Some(num) = parse_shard_number(&name, &stem) {
                numbered.push((num, entry.path()));
            }
        }
    }

    // Sort by shard number (ascending)
    numbered.sort_by_key(|(n, _)| *n);

    let mut result: Vec<PathBuf> = numbered.into_iter().map(|(_, p)| p).collect();

    // Append the current (unnumbered) file last
    if base_path.exists() {
        result.push(base_path.to_path_buf());
    }

    result
}

/// Parse the shard number from a filename like "memory.3.jsonl".
fn parse_shard_number(filename: &str, stem: &str) -> Option<u32> {
    let prefix = format!("{}.", stem);
    let suffix = ".jsonl";

    if !filename.starts_with(&prefix) || !filename.ends_with(suffix) {
        return None;
    }

    // Guard: base file itself (e.g., "memory.jsonl") matches prefix+suffix but has no middle
    if prefix.len() + suffix.len() >= filename.len() {
        return None;
    }

    let middle = &filename[prefix.len()..filename.len() - suffix.len()];
    middle.parse::<u32>().ok()
}

/// Rotate the current JSONL file to a numbered shard.
fn rotate_jsonl_shard(path: &Path) -> io::Result<()> {
    let next_num = find_max_shard_number(path) + 1;
    let parent = path.parent().unwrap_or(Path::new("."));
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let shard_name = format!("{}.{}.jsonl", stem, next_num);
    let shard_path = parent.join(shard_name);

    fs::rename(path, &shard_path)?;
    log::info!("Rotated {:?} → {:?}", path, shard_path);

    Ok(())
}

/// Find the highest shard number for a base path.
fn find_max_shard_number(base_path: &Path) -> u32 {
    let parent = base_path.parent().unwrap_or(Path::new("."));
    let stem = base_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();

    let mut max = 0u32;

    if let Ok(entries) = fs::read_dir(parent) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(num) = parse_shard_number(&name, &stem) {
                max = max.max(num);
            }
        }
    }

    max
}

// ─── PID-based file lock ──────────────────────────────────────────────────

/// A PID-based file lock for agent concurrency control.
///
/// The lock file contains the process PID. On acquire, if the lock file exists:
/// - If the PID is alive → ConversationLocked error
/// - If the PID is dead → orphan lock, remove and re-lock
pub struct FileLock {
    path: PathBuf,
}

impl FileLock {
    /// Acquire a lock at the given path.
    ///
    /// Returns `Err` if the lock is held by a live process.
    pub fn acquire(path: &Path) -> io::Result<Self> {
        if path.exists() {
            // Check if the existing lock is from a live process
            let content = fs::read_to_string(path).unwrap_or_default();
            if let Ok(pid) = content.trim().parse::<u32>() {
                if process_alive(pid) {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        format!("Lock held by PID {}", pid),
                    ));
                }
                // Orphan lock — remove it
                log::warn!("Removing orphan lock {:?} (PID {} dead)", path, pid);
                let _ = fs::remove_file(path);
            } else {
                // Corrupted lock file — remove
                log::warn!("Removing corrupted lock file {:?}", path);
                let _ = fs::remove_file(path);
            }
        }

        // Ensure parent exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Write our PID
        let pid = std::process::id();
        fs::write(path, pid.to_string())?;

        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    /// Release the lock (delete the lock file).
    pub fn release(self) {
        let _ = fs::remove_file(&self.path);
    }

    /// Get the lock file path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Check if a process with the given PID is alive.
#[cfg(unix)]
pub fn process_alive(pid: u32) -> bool {
    // On Unix, kill(pid, 0) checks if the process exists without sending a signal
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Check if a process with the given PID is alive (non-Unix fallback).
#[cfg(not(unix))]
pub fn process_alive(pid: u32) -> bool {
    // On Windows, we could use OpenProcess, but for simplicity assume dead
    // if we can't verify. This is safe because the worst case is re-acquiring
    // a lock that's actually still held, which the gateway prevents anyway.
    use std::process::Command;
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid)])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

// ─── Helpers ──────────────────────────────────────────────────────────────

/// Strip the completion marker from a JSONL line.
/// Returns the JSON portion if the marker is present, `None` otherwise.
fn strip_completion_marker(line: &str) -> Option<&str> {
    let trimmed = line.trim_end();
    if trimmed.ends_with(LINE_COMPLETE) {
        Some(&trimmed[..trimmed.len() - LINE_COMPLETE.len()])
    } else {
        None
    }
}

/// Append an extension to a path (e.g., "/foo/bar.json" + "bak" → "/foo/bar.json.bak").
fn append_ext(path: &Path, ext: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".");
    s.push(ext);
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::TempDir;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestRecord {
        id: String,
        value: i32,
    }

    #[test]
    fn test_atomic_write_and_read_json() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.json");

        let data = serde_json::json!({"name": "test", "count": 42});
        atomic_write_json(&path, &data).unwrap();

        let read: serde_json::Value = read_json_safe(&path).unwrap();
        assert_eq!(read["name"], "test");
        assert_eq!(read["count"], 42);
    }

    #[test]
    fn test_atomic_write_creates_backup() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.json");

        // First write
        atomic_write_json(&path, &serde_json::json!({"v": 1})).unwrap();

        // Second write should create .bak
        atomic_write_json(&path, &serde_json::json!({"v": 2})).unwrap();

        let bak_path = dir.path().join("data.json.bak");
        assert!(bak_path.exists());

        let bak_data: serde_json::Value = read_json_file(&bak_path).unwrap();
        assert_eq!(bak_data["v"], 1);
    }

    #[test]
    fn test_read_json_safe_fallback_to_bak() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.json");
        let bak = dir.path().join("data.json.bak");

        // Write backup only (simulating corrupted main file)
        let data = serde_json::json!({"recovered": true});
        fs::write(&bak, serde_json::to_string(&data).unwrap()).unwrap();
        fs::write(&path, "corrupted{{{").unwrap();

        let read: serde_json::Value = read_json_safe(&path).unwrap();
        assert_eq!(read["recovered"], true);
    }

    #[test]
    fn test_jsonl_append_and_read() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.jsonl");

        let r1 = TestRecord { id: "a".into(), value: 1 };
        let r2 = TestRecord { id: "b".into(), value: 2 };

        append_jsonl(&path, &r1).unwrap();
        append_jsonl(&path, &r2).unwrap();

        let records: Vec<TestRecord> = read_jsonl(&path).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0], r1);
        assert_eq!(records[1], r2);
    }

    #[test]
    fn test_jsonl_discards_incomplete_lines() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.jsonl");

        // Write one valid line
        append_jsonl(&path, &TestRecord { id: "ok".into(), value: 1 }).unwrap();

        // Manually append an incomplete line (no marker)
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"{\"id\":\"bad\",\"value\":99}\n").unwrap();

        // Write another valid line
        append_jsonl(&path, &TestRecord { id: "ok2".into(), value: 2 }).unwrap();

        let records: Vec<TestRecord> = read_jsonl(&path).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, "ok");
        assert_eq!(records[1].id, "ok2");
    }

    #[test]
    fn test_jsonl_tail() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.jsonl");

        for i in 0..10 {
            append_jsonl(&path, &TestRecord { id: format!("r{}", i), value: i }).unwrap();
        }

        let tail: Vec<TestRecord> = read_jsonl_tail(&path, 3).unwrap();
        assert_eq!(tail.len(), 3);
        assert_eq!(tail[0].value, 7);
        assert_eq!(tail[2].value, 9);
    }

    #[test]
    fn test_auto_split() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("log.jsonl");

        // Use tiny max_bytes to trigger split quickly
        for i in 0..20 {
            append_jsonl_with_split(
                &path,
                &TestRecord { id: format!("r{}", i), value: i },
                100, // 100 bytes threshold
            ).unwrap();
        }

        // Should have split into multiple shards
        let all: Vec<TestRecord> = read_all_jsonl_shards(&path).unwrap();
        assert_eq!(all.len(), 20);
        assert_eq!(all[0].value, 0);
        assert_eq!(all[19].value, 19);
    }

    #[test]
    fn test_file_lock_acquire_release() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.lock");

        let lock = FileLock::acquire(&path).unwrap();
        assert!(path.exists());

        // Read the PID from the lock file
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content.trim().parse::<u32>().unwrap(), std::process::id());

        lock.release();
        assert!(!path.exists());
    }

    #[test]
    fn test_file_lock_detects_orphan() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.lock");

        // Write a lock file with a dead PID
        fs::write(&path, "999999999").unwrap();

        // Should succeed because PID 999999999 is (almost certainly) dead
        let lock = FileLock::acquire(&path);
        assert!(lock.is_ok());
    }

    #[test]
    fn test_count_jsonl_lines() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.jsonl");

        assert_eq!(count_jsonl_lines(&path).unwrap(), 0);

        for i in 0..5 {
            append_jsonl(&path, &TestRecord { id: format!("r{}", i), value: i }).unwrap();
        }

        assert_eq!(count_jsonl_lines(&path).unwrap(), 5);
    }

    #[test]
    fn test_strip_completion_marker() {
        assert_eq!(
            strip_completion_marker("{\"a\":1}\t\u{2713}"),
            Some("{\"a\":1}")
        );
        assert_eq!(strip_completion_marker("{\"a\":1}"), None);
        assert_eq!(strip_completion_marker(""), None);
    }
}
