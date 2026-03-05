//! Cognitive Memory System — multi-layer persistent memory.
//!
//! ## File structure
//!
//! ```text
//! {base_dir}/shared/cognitive/
//! ├── mem.md             # Core memory — always loaded (~200 lines)
//! ├── meta.json          # Core memory section metadata
//! ├── index.json         # Global memory index (hit counts, promotion state)
//! ├── daily/             # Daily memory JSONL files
//! │   ├── 2026-03-05.jsonl
//! │   └── ...
//! └── archive/           # Archived daily files (> 90 days)
//! ```

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use chrono::{NaiveDate, Utc};
use log::{info, warn};
use serde::{Deserialize, Serialize};

// ─── Constants ──────────────────────────────────────────────────────────────

/// Maximum lines in mem.md before capacity eviction kicks in.
const CORE_MAX_LINES: usize = 200;

/// Maximum daily save_memory calls per conversation turn (enforced by executor).
pub const SAVE_RATE_LIMIT: usize = 5;

/// Content length bounds.
pub const CONTENT_MIN_LEN: usize = 10;
pub const CONTENT_MAX_LEN: usize = 500;

/// Hit count threshold for promotion.
const PROMOTE_HIT_THRESHOLD: u32 = 3;
/// Minimum distinct conversations that hit the memory for promotion.
const PROMOTE_CONV_THRESHOLD: usize = 2;

/// Days without a hit before a core section is eligible for soft demotion.
const DECAY_SOFT_DAYS: i64 = 30;
/// Minimum hit count to survive soft demotion.
const DECAY_SOFT_MIN_HITS: u32 = 5;
/// Days without a hit before forced demotion (regardless of hit count).
const DECAY_HARD_DAYS: i64 = 60;

/// Daily files older than this many days are moved to archive/.
const ARCHIVE_AFTER_DAYS: i64 = 90;

/// Tag overlap threshold (Jaccard) for merging during promotion.
const TAG_MERGE_THRESHOLD: f64 = 0.6;

// ─── Data Models ────────────────────────────────────────────────────────────

/// Memory category.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MemoryCategory {
    Preference,
    Fact,
    Learning,
    Pattern,
    Observation,
}

impl MemoryCategory {
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "preference" => Ok(Self::Preference),
            "fact" => Ok(Self::Fact),
            "learning" => Ok(Self::Learning),
            "pattern" => Ok(Self::Pattern),
            "observation" => Ok(Self::Observation),
            _ => bail!("Invalid memory category: '{}'. Must be one of: preference, fact, learning, pattern, observation", s),
        }
    }

    /// Section heading in mem.md.
    pub fn section_heading(&self) -> &'static str {
        match self {
            Self::Preference => "## User Preferences",
            Self::Fact => "## Enterprise Facts",
            Self::Learning => "## Data Patterns",
            Self::Pattern => "## Analysis Patterns",
            Self::Observation => "## Observations",
        }
    }
}

/// Single memory entry stored in daily JSONL files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CognitiveEntry {
    pub id: String,
    pub content: String,
    pub category: MemoryCategory,
    #[serde(default)]
    pub tags: Vec<String>,
    pub source_conversation_id: String,
    #[serde(default = "default_daily")]
    pub source_mode: String,
    pub created_at: String,
    #[serde(default)]
    pub promoted: bool,
    #[serde(default)]
    pub deleted: bool,
}

fn default_daily() -> String {
    "daily".to_string()
}

/// Index entry tracking hits and promotion state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryMeta {
    pub memory_id: String,
    pub source_date: String,
    #[serde(default)]
    pub hit_count: u32,
    #[serde(default)]
    pub hit_conversations: Vec<String>,
    pub last_hit_at: Option<String>,
    #[serde(default)]
    pub promoted: bool,
    pub promoted_at: Option<String>,
    #[serde(default)]
    pub content_hash: u64,
}

/// Core memory metadata stored in meta.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreMemoryMeta {
    #[serde(default)]
    pub version: u32,
    pub updated_at: String,
    #[serde(default)]
    pub line_count: u32,
    #[serde(default)]
    pub sections: Vec<CoreSection>,
    /// Timestamp of last automatic distillation.
    pub last_distill_at: Option<String>,
}

impl Default for CoreMemoryMeta {
    fn default() -> Self {
        Self {
            version: 1,
            updated_at: Utc::now().to_rfc3339(),
            line_count: 0,
            sections: Vec::new(),
            last_distill_at: None,
        }
    }
}

/// Metadata for a single section in mem.md.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreSection {
    pub heading: String,
    #[serde(default)]
    pub source_memory_ids: Vec<String>,
    #[serde(default)]
    pub hit_count: u32,
    pub last_hit_at: Option<String>,
    pub added_at: String,
}

/// Global index file wrapping all MemoryMeta entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryIndex {
    pub entries: Vec<MemoryMeta>,
}

/// Result of a distill operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistillReport {
    pub promoted: usize,
    pub skipped_dup: usize,
    pub demoted: usize,
    pub archived: usize,
    pub core_lines: usize,
}

// ─── Path Helpers ───────────────────────────────────────────────────────────

fn cognitive_dir(base_dir: &Path) -> PathBuf {
    base_dir.join("shared").join("cognitive")
}

fn core_memory_path(base_dir: &Path) -> PathBuf {
    cognitive_dir(base_dir).join("mem.md")
}

fn meta_path(base_dir: &Path) -> PathBuf {
    cognitive_dir(base_dir).join("meta.json")
}

fn index_path(base_dir: &Path) -> PathBuf {
    cognitive_dir(base_dir).join("index.json")
}

fn daily_dir(base_dir: &Path) -> PathBuf {
    cognitive_dir(base_dir).join("daily")
}

fn archive_dir(base_dir: &Path) -> PathBuf {
    cognitive_dir(base_dir).join("archive")
}

fn daily_file(base_dir: &Path, date: &str) -> PathBuf {
    // Validate date format to prevent path traversal (e.g. "../../etc")
    debug_assert!(
        date.len() == 10 && date.chars().all(|c| c.is_ascii_digit() || c == '-'),
        "daily_file called with invalid date: {}", date
    );
    daily_dir(base_dir).join(format!("{}.jsonl", date))
}

// ─── Hashing ────────────────────────────────────────────────────────────────

/// FNV-1a 64-bit hash for content deduplication.
///
/// Uses a deterministic algorithm (no random seed) so hashes are stable
/// across process restarts — critical for cross-session dedup in index.json.
pub fn content_hash(content: &str) -> u64 {
    let normalized = content.trim().to_lowercase();
    // FNV-1a constants for 64-bit
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x00000100000001B3;
    let mut hash = FNV_OFFSET;
    for byte in normalized.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Jaccard similarity of two tag sets.
fn tag_jaccard(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let set_a: HashSet<&str> = a.iter().map(|s| s.as_str()).collect();
    let set_b: HashSet<&str> = b.iter().map(|s| s.as_str()).collect();
    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f64 / union as f64
}

// ─── Directory Initialization ───────────────────────────────────────────────

/// Ensure the cognitive memory directory structure exists.
pub fn ensure_dirs(base_dir: &Path) -> Result<()> {
    fs::create_dir_all(daily_dir(base_dir))?;
    fs::create_dir_all(archive_dir(base_dir))?;
    Ok(())
}

// ─── Index CRUD ─────────────────────────────────────────────────────────────

fn load_index(base_dir: &Path) -> Result<MemoryIndex> {
    let path = index_path(base_dir);
    if !path.exists() {
        return Ok(MemoryIndex::default());
    }
    let data = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read cognitive index: {:?}", path))?;
    let index: MemoryIndex = serde_json::from_str(&data)
        .with_context(|| "Failed to parse cognitive index.json")?;
    Ok(index)
}

fn save_index(base_dir: &Path, index: &MemoryIndex) -> Result<()> {
    let path = index_path(base_dir);
    let data = serde_json::to_string_pretty(index)?;
    atomic_write(&path, data.as_bytes())?;
    Ok(())
}

fn load_meta(base_dir: &Path) -> Result<CoreMemoryMeta> {
    let path = meta_path(base_dir);
    if !path.exists() {
        return Ok(CoreMemoryMeta::default());
    }
    let data = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read cognitive meta: {:?}", path))?;
    let meta: CoreMemoryMeta = serde_json::from_str(&data)
        .with_context(|| "Failed to parse cognitive meta.json")?;
    Ok(meta)
}

fn save_meta(base_dir: &Path, meta: &CoreMemoryMeta) -> Result<()> {
    let path = meta_path(base_dir);
    let data = serde_json::to_string_pretty(meta)?;
    atomic_write(&path, data.as_bytes())?;
    Ok(())
}

/// Atomic write: write to .tmp then rename.
fn atomic_write(target: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = target.with_extension("tmp");
    fs::write(&tmp, data)?;
    fs::rename(&tmp, target)?;
    Ok(())
}

// ─── Daily JSONL ────────────────────────────────────────────────────────────

/// Append a CognitiveEntry to the daily JSONL file.
fn append_daily(base_dir: &Path, date: &str, entry: &CognitiveEntry) -> Result<()> {
    let path = daily_file(base_dir, date);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_string(entry)?;
    line.push('\n');
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    f.write_all(line.as_bytes())?;
    Ok(())
}

/// Read all entries from a daily JSONL file.
fn read_daily(path: &Path) -> Result<Vec<CognitiveEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = fs::read_to_string(path)?;
    let mut entries = Vec::new();
    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<CognitiveEntry>(line) {
            Ok(entry) if !entry.deleted => entries.push(entry),
            Ok(_) => {} // deleted
            Err(e) => warn!("Skipping malformed cognitive entry: {}", e),
        }
    }
    Ok(entries)
}

/// Load daily entries for the last N days.
fn load_recent_daily(base_dir: &Path, days: i64) -> Result<Vec<CognitiveEntry>> {
    let today = Utc::now().date_naive();
    let mut all = Vec::new();
    for offset in 0..days {
        let date = today - chrono::Duration::days(offset);
        let path = daily_file(base_dir, &date.format("%Y-%m-%d").to_string());
        let entries = read_daily(&path)?;
        all.extend(entries);
    }
    Ok(all)
}

// ─── Core Memory (mem.md) ───────────────────────────────────────────────────

/// Read the core memory text. Returns empty string if not found.
pub fn load_core_memory(base_dir: &Path) -> String {
    let path = core_memory_path(base_dir);
    fs::read_to_string(&path).unwrap_or_default()
}

/// Write the core memory text atomically.
fn write_core_memory(base_dir: &Path, content: &str) -> Result<()> {
    let path = core_memory_path(base_dir);
    atomic_write(&path, content.as_bytes())
}

/// Append an entry directly to the correct section in mem.md.
fn append_to_core(base_dir: &Path, category: &MemoryCategory, content: &str, memory_id: &str) -> Result<()> {
    let heading = category.section_heading();
    let mut text = load_core_memory(base_dir);

    let bullet = format!("- {}", content);

    if let Some(pos) = text.find(heading) {
        // Find the end of this section (next ## heading or EOF)
        let section_start = pos + heading.len();
        let next_section = text[section_start..].find("\n## ").map(|p| section_start + p);
        let insert_at = next_section.unwrap_or(text.len());

        // Insert before next section (with a newline)
        let line = format!("\n{}", bullet);
        text.insert_str(insert_at, &line);
    } else {
        // Section doesn't exist — append it
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!("\n{}\n{}\n", heading, bullet));
    }

    // Ensure header if empty file
    if !text.starts_with("# Core Memory") {
        text = format!("# Core Memory\n\n{}", text);
    }

    write_core_memory(base_dir, &text)?;

    // Update meta.json
    let mut meta = load_meta(base_dir)?;
    meta.version += 1;
    meta.updated_at = Utc::now().to_rfc3339();
    meta.line_count = text.lines().count() as u32;

    // Update or add section metadata
    if let Some(section) = meta.sections.iter_mut().find(|s| s.heading == heading) {
        if !section.source_memory_ids.contains(&memory_id.to_string()) {
            section.source_memory_ids.push(memory_id.to_string());
        }
    } else {
        meta.sections.push(CoreSection {
            heading: heading.to_string(),
            source_memory_ids: vec![memory_id.to_string()],
            hit_count: 0,
            last_hit_at: None,
            added_at: Utc::now().to_rfc3339(),
        });
    }

    save_meta(base_dir, &meta)?;
    Ok(())
}

// ─── Public API ─────────────────────────────────────────────────────────────

/// Save a new memory entry.
///
/// Returns (id, to_core) on success.
pub fn save_memory(
    base_dir: &Path,
    content: &str,
    category: &str,
    tags: &[String],
    conversation_id: &str,
    to_core: bool,
) -> Result<(String, bool)> {
    // Validate
    let trimmed = content.trim();
    if trimmed.len() < CONTENT_MIN_LEN {
        bail!("Memory content too short (min {} characters)", CONTENT_MIN_LEN);
    }
    if trimmed.len() > CONTENT_MAX_LEN {
        bail!("Memory content too long (max {} characters)", CONTENT_MAX_LEN);
    }
    let cat = MemoryCategory::from_str(category)?;

    // to_core only allowed for preference and fact
    if to_core && !matches!(cat, MemoryCategory::Preference | MemoryCategory::Fact) {
        bail!("to_core=true is only allowed for 'preference' and 'fact' categories");
    }

    ensure_dirs(base_dir)?;

    let id = uuid::Uuid::new_v4().to_string();
    let hash = content_hash(trimmed);
    let today = Utc::now().format("%Y-%m-%d").to_string();
    let now = Utc::now().to_rfc3339();

    // Dedup: check if same content hash exists today
    let mut index = load_index(base_dir)?;
    for meta in &index.entries {
        if meta.content_hash == hash && meta.source_date == today {
            return Err(anyhow!("Duplicate memory already saved today"));
        }
    }

    let entry = CognitiveEntry {
        id: id.clone(),
        content: trimmed.to_string(),
        category: cat.clone(),
        tags: tags.to_vec(),
        source_conversation_id: conversation_id.to_string(),
        source_mode: "daily".to_string(),
        created_at: now.clone(),
        promoted: to_core,
        deleted: false,
    };

    if to_core {
        // Write directly to core memory
        append_to_core(base_dir, &cat, trimmed, &id)?;
    }

    // Always write to daily file (for audit trail)
    append_daily(base_dir, &today, &entry)?;

    // Add to index
    index.entries.push(MemoryMeta {
        memory_id: id.clone(),
        source_date: today,
        hit_count: 0,
        hit_conversations: vec![conversation_id.to_string()],
        last_hit_at: None,
        promoted: to_core,
        promoted_at: if to_core { Some(now) } else { None },
        content_hash: hash,
    });
    save_index(base_dir, &index)?;

    Ok((id, to_core))
}

/// Search memories by keywords (read-only).
///
/// Returns matching entries (up to 10) sorted by relevance score.
/// Call `record_search_hits` separately to update hit counts.
pub fn search_memory_readonly(
    base_dir: &Path,
    query: &str,
    category: Option<&str>,
    days: i64,
) -> Result<Vec<serde_json::Value>> {
    ensure_dirs(base_dir)?;

    let keywords: Vec<String> = query
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();

    if keywords.is_empty() {
        return Ok(Vec::new());
    }

    let cat_filter = category.and_then(|c| MemoryCategory::from_str(c).ok());

    // 1. Search daily entries
    let daily_entries = load_recent_daily(base_dir, days)?;
    let mut scored: Vec<(f64, serde_json::Value, String)> = Vec::new();

    for entry in &daily_entries {
        // Category filter
        if let Some(ref cf) = cat_filter {
            if entry.category != *cf {
                continue;
            }
        }

        let score = score_entry(&entry.content, &entry.tags, &keywords);
        if score > 0.0 {
            scored.push((score, serde_json::json!({
                "id": entry.id,
                "content": entry.content,
                "category": entry.category,
                "tags": entry.tags,
                "source": "daily",
                "created_at": entry.created_at,
                "promoted": entry.promoted,
            }), entry.id.clone()));
        }
    }

    // 2. Search core memory lines
    let core_text = load_core_memory(base_dir);
    if !core_text.is_empty() {
        for line in core_text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let content = line.strip_prefix("- ").unwrap_or(line);
            let score = score_text(content, &keywords);
            if score > 0.0 {
                scored.push((score + 5.0, serde_json::json!({
                    "content": content,
                    "source": "core",
                }), String::new())); // core lines don't have individual IDs in index
            }
        }
    }

    // Sort by score descending, take top 10
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(10);

    Ok(scored.into_iter().map(|(score, mut val, _)| {
        val.as_object_mut().unwrap().insert("score".to_string(), serde_json::json!(score));
        val
    }).collect())
}

/// Record hit counts for search results (write operation).
///
/// Call after `search_memory_readonly` with the returned results.
pub fn record_search_hits(
    base_dir: &Path,
    results: &[serde_json::Value],
    query: &str,
    conversation_id: &str,
) -> Result<()> {
    let hit_ids: Vec<String> = results.iter()
        .filter_map(|v| v.get("id").and_then(|id| id.as_str()).map(String::from))
        .collect();

    if !hit_ids.is_empty() {
        update_hit_counts(base_dir, &hit_ids, conversation_id)?;
    }

    // Update core section hit counts for core matches
    let has_core = results.iter()
        .any(|v| v.get("source").and_then(|s| s.as_str()) == Some("core"));
    if has_core {
        let keywords: Vec<String> = query.split_whitespace()
            .map(|w| w.to_lowercase())
            .collect();
        update_core_section_hits(base_dir, &keywords)?;
    }

    Ok(())
}

/// Score an entry against keywords.
fn score_entry(content: &str, tags: &[String], keywords: &[String]) -> f64 {
    let content_lower = content.to_lowercase();
    let mut score = 0.0;
    for kw in keywords {
        if content_lower.contains(kw.as_str()) {
            score += 2.0;
        }
        for tag in tags {
            if tag.to_lowercase().contains(kw.as_str()) {
                score += 1.5;
            }
        }
    }
    score
}

/// Score plain text against keywords.
fn score_text(text: &str, keywords: &[String]) -> f64 {
    let lower = text.to_lowercase();
    let mut score = 0.0;
    for kw in keywords {
        if lower.contains(kw.as_str()) {
            score += 2.0;
        }
    }
    score
}

/// Update hit counts in index.json for matched entries.
fn update_hit_counts(base_dir: &Path, ids: &[String], conversation_id: &str) -> Result<()> {
    let mut index = load_index(base_dir)?;
    let now = Utc::now().to_rfc3339();
    let id_set: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();

    for meta in &mut index.entries {
        if id_set.contains(meta.memory_id.as_str()) {
            meta.hit_count += 1;
            meta.last_hit_at = Some(now.clone());
            if !meta.hit_conversations.contains(&conversation_id.to_string()) {
                meta.hit_conversations.push(conversation_id.to_string());
            }
        }
    }

    save_index(base_dir, &index)?;
    Ok(())
}

/// Update core section hit counts when core memory lines match a search.
fn update_core_section_hits(base_dir: &Path, keywords: &[String]) -> Result<()> {
    let mut meta = load_meta(base_dir)?;
    let core_text = load_core_memory(base_dir);
    let now = Utc::now().to_rfc3339();

    // Find which sections have matching lines
    let mut current_heading: Option<String> = None;
    for line in core_text.lines() {
        if line.starts_with("## ") {
            current_heading = Some(line.to_string());
        } else if let Some(ref heading) = current_heading {
            let content = line.trim().strip_prefix("- ").unwrap_or(line.trim());
            if !content.is_empty() && score_text(content, keywords) > 0.0 {
                if let Some(section) = meta.sections.iter_mut().find(|s| s.heading == *heading) {
                    section.hit_count += 1;
                    section.last_hit_at = Some(now.clone());
                }
            }
        }
    }

    save_meta(base_dir, &meta)?;
    Ok(())
}

/// Distill memories: promote high-hit entries to core, apply decay, archive old files.
pub fn distill_memories(base_dir: &Path, days: i64, dry_run: bool) -> Result<DistillReport> {
    ensure_dirs(base_dir)?;

    let mut index = load_index(base_dir)?;
    let daily_entries = load_recent_daily(base_dir, days.max(90))?; // Load enough for decay check
    let mut report = DistillReport {
        promoted: 0,
        skipped_dup: 0,
        demoted: 0,
        archived: 0,
        core_lines: 0,
    };

    // Build a lookup from memory_id to CognitiveEntry
    let entry_map: HashMap<String, &CognitiveEntry> = daily_entries.iter()
        .map(|e| (e.id.clone(), e))
        .collect();

    // 1. Find promotion candidates
    let existing_hashes: HashSet<u64> = index.entries.iter()
        .filter(|m| m.promoted)
        .map(|m| m.content_hash)
        .collect();

    let candidates: Vec<&MemoryMeta> = index.entries.iter()
        .filter(|m| {
            !m.promoted
                && m.hit_count >= PROMOTE_HIT_THRESHOLD
                && m.hit_conversations.len() >= PROMOTE_CONV_THRESHOLD
        })
        .collect();

    let now = Utc::now().to_rfc3339();

    for candidate in &candidates {
        // Content hash dedup
        if existing_hashes.contains(&candidate.content_hash) {
            report.skipped_dup += 1;
            continue;
        }

        // Find the actual entry
        if let Some(entry) = entry_map.get(&candidate.memory_id) {
            // Tag overlap check — merge if similar entry already promoted
            let should_skip = index.entries.iter()
                .filter(|m| m.promoted && m.memory_id != candidate.memory_id)
                .any(|m| {
                    if let Some(other_entry) = entry_map.get(&m.memory_id) {
                        tag_jaccard(&entry.tags, &other_entry.tags) > TAG_MERGE_THRESHOLD
                    } else {
                        false
                    }
                });

            if should_skip {
                report.skipped_dup += 1;
                continue;
            }

            if !dry_run {
                append_to_core(base_dir, &entry.category, &entry.content, &entry.id)?;
            }
            report.promoted += 1;
        }
    }

    // Mark promoted entries in index
    if !dry_run {
        let promoted_ids: HashSet<String> = candidates.iter()
            .filter(|c| !existing_hashes.contains(&c.content_hash))
            .map(|c| c.memory_id.clone())
            .collect();

        for meta in &mut index.entries {
            if promoted_ids.contains(&meta.memory_id) && !meta.promoted {
                meta.promoted = true;
                meta.promoted_at = Some(now.clone());
            }
        }
    }

    // 2. Decay: remove stale core sections
    if !dry_run {
        let demoted = apply_decay(base_dir)?;
        report.demoted = demoted;
    }

    // 3. Capacity check
    if !dry_run {
        let evicted = enforce_capacity(base_dir)?;
        report.demoted += evicted;
    }

    // 4. Archive old daily files
    if !dry_run {
        report.archived = archive_old_daily(base_dir)?;
    }

    // Update distill timestamp
    if !dry_run {
        let mut meta = load_meta(base_dir)?;
        meta.last_distill_at = Some(now);
        save_meta(base_dir, &meta)?;
        save_index(base_dir, &index)?;
    }

    // Count final core lines
    let core = load_core_memory(base_dir);
    report.core_lines = core.lines().count();

    Ok(report)
}

/// Apply decay rules to core memory sections.
fn apply_decay(base_dir: &Path) -> Result<usize> {
    let mut meta = load_meta(base_dir)?;
    let now = Utc::now();
    let mut to_remove: Vec<String> = Vec::new();

    for section in &meta.sections {
        // Grace period: don't decay sections added within the last 7 days
        let days_since_added = chrono::DateTime::parse_from_rfc3339(&section.added_at)
            .ok()
            .map(|dt| (now - dt.with_timezone(&Utc)).num_days())
            .unwrap_or(999);
        if days_since_added < 7 {
            continue;
        }

        let last_hit = section.last_hit_at.as_ref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let days_since_hit = last_hit
            .map(|lh| (now - lh).num_days())
            .unwrap_or(999); // Never hit → treat as very old

        // Hard decay: 60 days without any hit
        if days_since_hit >= DECAY_HARD_DAYS {
            to_remove.push(section.heading.clone());
            continue;
        }

        // Soft decay: 30 days without hit AND low hit count
        if days_since_hit >= DECAY_SOFT_DAYS && section.hit_count < DECAY_SOFT_MIN_HITS {
            to_remove.push(section.heading.clone());
        }
    }

    if to_remove.is_empty() {
        return Ok(0);
    }

    // Remove sections from mem.md
    let mut core = load_core_memory(base_dir);
    for heading in &to_remove {
        core = remove_section_from_core(&core, heading);
    }
    write_core_memory(base_dir, &core)?;

    // Remove from meta
    meta.sections.retain(|s| !to_remove.contains(&s.heading));
    meta.line_count = core.lines().count() as u32;
    meta.updated_at = Utc::now().to_rfc3339();
    save_meta(base_dir, &meta)?;

    let count = to_remove.len();
    info!("Cognitive memory decay: removed {} sections", count);
    Ok(count)
}

/// Remove a section (heading + content until next heading) from core text.
fn remove_section_from_core(text: &str, heading: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut result = Vec::new();
    let mut in_section = false;

    for line in &lines {
        if *line == heading {
            in_section = true;
            continue;
        }
        if in_section && line.starts_with("## ") {
            in_section = false;
        }
        if !in_section {
            result.push(*line);
        }
    }

    result.join("\n")
}

/// Enforce 200-line capacity on core memory by removing lowest-scoring sections.
fn enforce_capacity(base_dir: &Path) -> Result<usize> {
    let core = load_core_memory(base_dir);
    let line_count = core.lines().count();
    if line_count <= CORE_MAX_LINES {
        return Ok(0);
    }

    let meta = load_meta(base_dir)?;
    let now = Utc::now();
    let mut evicted = 0;

    // Score each section
    let mut section_scores: Vec<(&CoreSection, f64)> = meta.sections.iter()
        .map(|s| {
            let score = compute_section_score(s, &now);
            (s, score)
        })
        .collect();

    // Sort ascending (weakest first)
    section_scores.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // Remove weakest sections until under limit
    let mut current_text = core.clone();
    for (section, _score) in &section_scores {
        if current_text.lines().count() <= CORE_MAX_LINES {
            break;
        }
        current_text = remove_section_from_core(&current_text, &section.heading);
        evicted += 1;
    }

    if evicted > 0 {
        write_core_memory(base_dir, &current_text)?;
        let mut meta = load_meta(base_dir)?;
        let removed_headings: Vec<String> = section_scores.iter()
            .take(evicted)
            .map(|(s, _)| s.heading.clone())
            .collect();
        meta.sections.retain(|s| !removed_headings.contains(&s.heading));
        meta.line_count = current_text.lines().count() as u32;
        meta.updated_at = Utc::now().to_rfc3339();
        save_meta(base_dir, &meta)?;
        info!("Cognitive memory capacity: evicted {} sections", evicted);
    }

    Ok(evicted)
}

/// Compute section score = hit_count × recency_weight.
fn compute_section_score(section: &CoreSection, now: &chrono::DateTime<Utc>) -> f64 {
    let last_hit = section.last_hit_at.as_ref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    let days_since = last_hit
        .map(|lh| (*now - lh).num_days())
        .unwrap_or(999);

    let recency_weight = if days_since <= 7 {
        10.0
    } else if days_since <= 14 {
        5.0
    } else if days_since <= 30 {
        2.0
    } else {
        1.0
    };

    section.hit_count as f64 * recency_weight
}

/// Move daily files older than 90 days to archive/.
fn archive_old_daily(base_dir: &Path) -> Result<usize> {
    let daily = daily_dir(base_dir);
    let archive = archive_dir(base_dir);
    let cutoff = Utc::now().date_naive() - chrono::Duration::days(ARCHIVE_AFTER_DAYS);
    let mut count = 0;

    if !daily.exists() {
        return Ok(0);
    }

    for entry in fs::read_dir(&daily)?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".jsonl") {
            continue;
        }
        let date_str = name.trim_end_matches(".jsonl");
        if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
            if date < cutoff {
                let dest = archive.join(&name);
                fs::rename(entry.path(), &dest)?;
                count += 1;
            }
        }
    }

    if count > 0 {
        info!("Archived {} old daily memory files", count);
    }
    Ok(count)
}

/// Check if auto-distillation is needed (last distill > 24 hours ago).
pub fn needs_auto_distill(base_dir: &Path) -> bool {
    let meta = load_meta(base_dir).unwrap_or_default();
    match meta.last_distill_at {
        None => {
            // Only trigger if there are actually memory entries
            let index = load_index(base_dir).unwrap_or_default();
            !index.entries.is_empty()
        }
        Some(ref ts) => {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
                let elapsed = Utc::now() - dt.with_timezone(&Utc);
                elapsed.num_hours() >= 24
            } else {
                true
            }
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_base() -> (PathBuf, TempDir) {
        let dir = TempDir::new().unwrap();
        let base = dir.path().to_path_buf();
        ensure_dirs(&base).unwrap();
        (base, dir)
    }

    #[test]
    fn test_save_memory_basic() {
        let (base, _dir) = test_base();
        let (id, to_core) = save_memory(
            &base, "User prefers box plots for salary distribution",
            "preference", &["boxplot".to_string(), "salary".to_string()],
            "conv1", false,
        ).unwrap();
        assert!(!id.is_empty());
        assert!(!to_core);

        // Verify it's in the index
        let index = load_index(&base).unwrap();
        assert_eq!(index.entries.len(), 1);
        assert_eq!(index.entries[0].memory_id, id);
        assert_eq!(index.entries[0].hit_count, 0);
    }

    #[test]
    fn test_save_memory_to_core() {
        let (base, _dir) = test_base();
        let (id, to_core) = save_memory(
            &base, "Company has 500 employees",
            "fact", &["company".to_string(), "headcount".to_string()],
            "conv1", true,
        ).unwrap();
        assert!(to_core);

        // Verify it's in mem.md
        let core = load_core_memory(&base);
        assert!(core.contains("Company has 500 employees"));
        assert!(core.contains("## Enterprise Facts"));

        // Verify meta.json
        let meta = load_meta(&base).unwrap();
        assert!(!meta.sections.is_empty());
        assert!(meta.sections[0].source_memory_ids.contains(&id));
    }

    #[test]
    fn test_save_memory_validation() {
        let (base, _dir) = test_base();

        // Too short
        let result = save_memory(&base, "short", "fact", &[], "conv1", false);
        assert!(result.is_err());

        // Invalid category
        let result = save_memory(&base, "valid content here", "invalid", &[], "conv1", false);
        assert!(result.is_err());

        // to_core with wrong category
        let result = save_memory(&base, "some learning content", "learning", &[], "conv1", true);
        assert!(result.is_err());
    }

    #[test]
    fn test_save_memory_dedup() {
        let (base, _dir) = test_base();

        save_memory(&base, "User prefers box plots", "preference", &[], "conv1", false).unwrap();
        let result = save_memory(&base, "User prefers box plots", "preference", &[], "conv1", false);
        assert!(result.is_err()); // Duplicate
    }

    #[test]
    fn test_search_memory_basic() {
        let (base, _dir) = test_base();

        save_memory(&base, "User prefers box plots for distribution", "preference",
            &["boxplot".to_string()], "conv1", false).unwrap();
        save_memory(&base, "Company fiscal year is January to December", "fact",
            &["fiscal".to_string(), "year".to_string()], "conv1", false).unwrap();

        let results = search_memory_readonly(&base, "box plot", None, 30).unwrap();
        assert!(!results.is_empty());
        assert!(results[0]["content"].as_str().unwrap().contains("box plots"));
    }

    #[test]
    fn test_search_memory_with_category() {
        let (base, _dir) = test_base();

        save_memory(&base, "User prefers box plots for distribution", "preference",
            &[], "conv1", false).unwrap();
        save_memory(&base, "Company uses box plots extensively", "fact",
            &[], "conv1", false).unwrap();

        let results = search_memory_readonly(&base, "box plots", Some("preference"), 30).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_memory_hit_counting() {
        let (base, _dir) = test_base();

        save_memory(&base, "User prefers box plots for distribution", "preference",
            &["boxplot".to_string()], "conv1", false).unwrap();

        // Search then record hits
        let results = search_memory_readonly(&base, "box plots", None, 30).unwrap();
        record_search_hits(&base, &results, "box plots", "conv2").unwrap();

        let index = load_index(&base).unwrap();
        assert_eq!(index.entries[0].hit_count, 1);
        assert!(index.entries[0].hit_conversations.contains(&"conv2".to_string()));
    }

    #[test]
    fn test_core_memory_load() {
        let (base, _dir) = test_base();

        // No file yet
        assert!(load_core_memory(&base).is_empty());

        // After saving to core
        save_memory(&base, "Company has 500 employees", "fact",
            &["headcount".to_string()], "conv1", true).unwrap();
        let core = load_core_memory(&base);
        assert!(!core.is_empty());
        assert!(core.contains("500 employees"));
    }

    #[test]
    fn test_distill_promotion() {
        let (base, _dir) = test_base();

        // Save a memory
        save_memory(&base, "P50 is the default benchmark percentile", "pattern",
            &["p50".to_string(), "benchmark".to_string()], "conv1", false).unwrap();

        // Manually set hit counts to meet promotion threshold
        let mut index = load_index(&base).unwrap();
        index.entries[0].hit_count = 4;
        index.entries[0].hit_conversations = vec!["conv1".to_string(), "conv2".to_string(), "conv3".to_string()];
        save_index(&base, &index).unwrap();

        let report = distill_memories(&base, 7, false).unwrap();
        assert_eq!(report.promoted, 1);

        // Verify it's in core memory
        let core = load_core_memory(&base);
        assert!(core.contains("P50 is the default benchmark percentile"));
    }

    #[test]
    fn test_distill_dry_run() {
        let (base, _dir) = test_base();

        save_memory(&base, "P50 is the default benchmark percentile", "pattern",
            &["p50".to_string()], "conv1", false).unwrap();

        let mut index = load_index(&base).unwrap();
        index.entries[0].hit_count = 4;
        index.entries[0].hit_conversations = vec!["conv1".to_string(), "conv2".to_string()];
        save_index(&base, &index).unwrap();

        let report = distill_memories(&base, 7, true).unwrap();
        assert_eq!(report.promoted, 1);

        // Core memory should NOT be modified in dry_run
        let core = load_core_memory(&base);
        assert!(!core.contains("P50"));
    }

    #[test]
    fn test_content_hash_consistency() {
        let h1 = content_hash("hello world");
        let h2 = content_hash("  Hello World  ");
        assert_eq!(h1, h2); // normalized
    }

    #[test]
    fn test_tag_jaccard() {
        let a = vec!["salary".to_string(), "boxplot".to_string()];
        let b = vec!["salary".to_string(), "boxplot".to_string(), "chart".to_string()];
        let j = tag_jaccard(&a, &b);
        assert!(j > 0.6); // 2/3 = 0.667

        let c = vec!["python".to_string(), "code".to_string()];
        let j2 = tag_jaccard(&a, &c);
        assert!(j2 < 0.1); // 0/4 = 0.0
    }

    #[test]
    fn test_remove_section_from_core() {
        let text = "# Core Memory\n\n## Preferences\n- likes boxplots\n\n## Facts\n- 500 employees\n\n## Patterns\n- P50 default\n";
        let result = remove_section_from_core(text, "## Facts");
        assert!(!result.contains("500 employees"));
        assert!(result.contains("likes boxplots"));
        assert!(result.contains("P50 default"));
    }

    #[test]
    fn test_needs_auto_distill_empty() {
        let (base, _dir) = test_base();
        // No entries → no distill needed
        assert!(!needs_auto_distill(&base));
    }

    #[test]
    fn test_needs_auto_distill_with_entries() {
        let (base, _dir) = test_base();
        save_memory(&base, "Some important fact here", "fact", &[], "conv1", false).unwrap();
        // Has entries but never distilled → needs distill
        assert!(needs_auto_distill(&base));
    }

    #[test]
    fn test_multiple_sections_core() {
        let (base, _dir) = test_base();

        save_memory(&base, "User prefers Excel exports", "preference",
            &[], "conv1", true).unwrap();
        save_memory(&base, "Company has 500 employees", "fact",
            &[], "conv1", true).unwrap();

        let core = load_core_memory(&base);
        assert!(core.contains("## User Preferences"));
        assert!(core.contains("## Enterprise Facts"));
        assert!(core.contains("Excel exports"));
        assert!(core.contains("500 employees"));
    }
}
