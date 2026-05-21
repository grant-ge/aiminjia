//! review test：锁死 memory KV 设施所有符号 / 字符串字面量从生产代码（src-tauri/src/）消失。
//!
//! 这是反模式护栏——未来如果有人想再引入"借用 KV 设施做单 key upsert"的反模式，CI 会立即报错。
//!
//! 注意：本扫描**不包含 src-tauri/tests/**。`user_scope_migration_test.rs` 等历史 fixture
//! 可能仍然用到 "shared/memory" 字符串构造路径（是允许的，老 fixture 留作回归案例）。
//!
//! Pattern refinements (false-positive avoidance):
//! - `MemoryEntry` narrowed to `" MemoryEntry"` (space-prefixed): avoids matching
//!   `ProjectMemoryEntry` which is a different, legitimate project-memory system.
//! - `"memory.jsonl"` excluded from comment-only lines (lines whose trimmed form starts
//!   with `//`): the sharding helper in `file_store/io.rs` legitimately mentions this
//!   filename in a `// Guard:` comment as a generic sharding example.

use std::path::Path;

fn collect_rs_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, files);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            files.push(path);
        }
    }
}

/// Check that `pattern` does not appear in any non-comment line of any `.rs` file under
/// `src-tauri/src/`.  Lines whose trimmed text begins with `//` are skipped — they are
/// doc/inline comments and do not represent live production code.
fn assert_no_match(pattern: &str, label: &str) {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs_files(&src_dir, &mut files);

    let mut offenders = Vec::new();
    for file in &files {
        let content = std::fs::read_to_string(file).unwrap();
        for (i, line) in content.lines().enumerate() {
            // Skip pure comment lines — they are annotations/docs, not live code.
            if line.trim_start().starts_with("//") {
                continue;
            }
            if line.contains(pattern) {
                offenders.push(format!("{}:{} {}", file.display(), i + 1, line.trim()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "{}: found {} occurrences in src/:\n{}",
        label,
        offenders.len(),
        offenders.join("\n")
    );
}

#[test]
fn no_memory_kv_api_calls() {
    for pat in &[
        "set_memory(",
        "get_memory(",
        "get_memories_by_prefix(",
        "delete_memories_by_prefix(",
    ] {
        assert_no_match(pat, pat);
    }
}

#[test]
fn no_memory_kv_types() {
    // Use " MemoryEntry" (space-prefixed) so that legitimate `ProjectMemoryEntry` in
    // `runtime/project_memory.rs` is not flagged — the old KV type was bare `MemoryEntry`
    // and would always appear with a leading space in declarations and type positions.
    for pat in &[
        " MemoryEntry",
        "FileMemoryStore",
        " MemoryStore",
        "InMemoryMemoryStore",
        "FileAuthorizedWorkspaceStore",
    ] {
        assert_no_match(pat, pat);
    }
}

#[test]
fn no_dead_loaded_helpers() {
    for pat in &["loaded_key", "loaded_prefix", "load_failed_key"] {
        assert_no_match(pat, pat);
    }
}

#[test]
fn no_memory_string_literals() {
    // "memory.jsonl" is skipped on comment lines (see assert_no_match); the only
    // occurrence in src/ is a `// Guard:` comment in file_store/io.rs that uses it as
    // a generic sharding example — it is not a KV-path literal.
    for pat in &[
        "\"memory.jsonl\"",
        "\"shared/memory\"",
        "\"loaded:",
        "\"note:",
    ] {
        assert_no_match(pat, pat);
    }
}
