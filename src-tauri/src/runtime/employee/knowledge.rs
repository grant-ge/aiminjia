//! Knowledge indexer: splits a knowledge source file into chunks
//! and writes each chunk into cognitive memory under
//! `category="knowledge:{employee_id}"` so the runtime can later
//! retrieve them via `memory_search` instead of stuffing the whole
//! FAQ into the LLM context.

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;

use crate::runtime::employee::store::{EmployeeStore, KnowledgeSourceStatus};
use crate::storage::file_store::AppStorage;

const MIN_CHUNK_CHARS: usize = 40;
const MAX_CHUNK_CHARS: usize = 1200;

#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeChunk {
    pub title: Option<String>,
    pub content: String,
}

/// Heuristic chunker: prefers H2 headings, then Q/A pairs, then double-newline paragraphs.
pub fn chunk_markdown(src: &str) -> Vec<KnowledgeChunk> {
    let by_h2 = split_by_h2(src);
    if by_h2.len() >= 2 {
        // H2 path: may have a short preamble; collapse it into the first section.
        return collapse_short(by_h2);
    }
    let by_qa = split_by_qa(src);
    if by_qa.len() >= 2 {
        // Q/A path: each pair is a complete unit; hard-split only oversized pairs.
        return hard_split_only(by_qa);
    }
    // Paragraph split: each paragraph is its own chunk.
    split_paragraphs(src)
}

fn split_by_h2(src: &str) -> Vec<KnowledgeChunk> {
    let mut out = Vec::new();
    let mut current_title: Option<String> = None;
    let mut buf = String::new();
    for line in src.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            if !buf.trim().is_empty() {
                out.push(KnowledgeChunk {
                    title: current_title.clone(),
                    content: buf.trim().to_string(),
                });
                buf.clear();
            }
            current_title = Some(title.trim().to_string());
        } else {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    if !buf.trim().is_empty() {
        out.push(KnowledgeChunk {
            title: current_title,
            content: buf.trim().to_string(),
        });
    }
    out
}

fn split_by_qa(src: &str) -> Vec<KnowledgeChunk> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for line in src.lines() {
        if (line.starts_with("Q:") || line.starts_with("Q：")) && !buf.trim().is_empty() {
            out.push(KnowledgeChunk {
                title: None,
                content: buf.trim().to_string(),
            });
            buf.clear();
        }
        buf.push_str(line);
        buf.push('\n');
    }
    if !buf.trim().is_empty() {
        out.push(KnowledgeChunk {
            title: None,
            content: buf.trim().to_string(),
        });
    }
    out
}

fn split_paragraphs(src: &str) -> Vec<KnowledgeChunk> {
    src.split("\n\n")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| KnowledgeChunk {
            title: None,
            content: s.to_string(),
        })
        .collect()
}

fn hard_split_only(chunks: Vec<KnowledgeChunk>) -> Vec<KnowledgeChunk> {
    let mut out = Vec::new();
    for c in chunks {
        if c.content.chars().count() > MAX_CHUNK_CHARS {
            for piece in hard_split(&c.content, MAX_CHUNK_CHARS) {
                out.push(KnowledgeChunk {
                    title: c.title.clone(),
                    content: piece,
                });
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn collapse_short(chunks: Vec<KnowledgeChunk>) -> Vec<KnowledgeChunk> {
    // First pass: hard-split oversized chunks.
    let mut expanded: Vec<KnowledgeChunk> = Vec::new();
    for c in chunks {
        if c.content.chars().count() > MAX_CHUNK_CHARS {
            for piece in hard_split(&c.content, MAX_CHUNK_CHARS) {
                expanded.push(KnowledgeChunk {
                    title: c.title.clone(),
                    content: piece,
                });
            }
        } else {
            expanded.push(c);
        }
    }

    // Second pass: drop or prepend short untitled preamble chunks into the next chunk.
    // Titled chunks (real sections) are always kept as-is.
    let mut out: Vec<KnowledgeChunk> = Vec::new();
    let mut pending_prefix: Option<String> = None;
    for c in expanded {
        if c.title.is_none() && c.content.chars().count() < MIN_CHUNK_CHARS {
            // Short untitled fragment: hold as prefix for next chunk.
            let prev = pending_prefix.take().unwrap_or_default();
            pending_prefix = Some(if prev.is_empty() {
                c.content.clone()
            } else {
                format!("{}\n\n{}", prev, c.content)
            });
            continue;
        }
        // Normal chunk: prepend any pending prefix into it.
        if let Some(prefix) = pending_prefix.take() {
            out.push(KnowledgeChunk {
                title: c.title.clone(),
                content: format!("{}\n\n{}", prefix, c.content),
            });
        } else {
            out.push(c);
        }
    }
    // If there's a dangling prefix with nothing following, attach it to the last chunk.
    if let Some(prefix) = pending_prefix {
        if let Some(last) = out.last_mut() {
            last.content.push_str("\n\n");
            last.content.push_str(&prefix);
        } else {
            // Edge case: only short content, emit it anyway.
            out.push(KnowledgeChunk {
                title: None,
                content: prefix,
            });
        }
    }
    out
}

fn hard_split(s: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for ch in s.chars() {
        buf.push(ch);
        if buf.chars().count() >= max && (ch == '\n' || ch == '。' || ch == '.') {
            out.push(buf.trim().to_string());
            buf.clear();
        }
    }
    if !buf.trim().is_empty() {
        out.push(buf.trim().to_string());
    }
    out
}

/// Index one knowledge source file. Updates employee record status as it progresses.
/// Designed to be called inside a `tokio::task::spawn_blocking` (it does sync IO + mutex).
pub fn index_one(
    store: &EmployeeStore,
    app_storage: &AppStorage,
    employee_id: &str,
    file_path: &Path,
    original_name: &str,
) -> Result<u64> {
    let path_str = file_path.to_string_lossy().to_string();

    store.update_knowledge_source_status(
        employee_id,
        &path_str,
        KnowledgeSourceStatus::Indexing,
        0,
        None,
    )?;

    let result = (|| -> Result<u64> {
        let raw = std::fs::read_to_string(file_path)
            .with_context(|| format!("read {}", file_path.display()))?;
        let chunks = chunk_markdown(&raw);
        let mut written = 0u64;
        for chunk in chunks {
            let content = match &chunk.title {
                Some(t) => format!("【{}】\n{}", t, chunk.content),
                None => chunk.content.clone(),
            };
            let category = format!("knowledge:{}", employee_id);
            let tags = vec!["faq".to_string(), original_name.to_string()];
            let _ = app_storage.save_cognitive_memory(
                &content,
                &category,
                &tags,
                employee_id,
                false,
            )?;
            written += 1;
        }
        Ok(written)
    })();

    match result {
        Ok(count) => {
            store.update_knowledge_source_status(
                employee_id,
                &path_str,
                KnowledgeSourceStatus::Done,
                count,
                None,
            )?;
            Ok(count)
        }
        Err(e) => {
            let msg = format!("{:#}", e);
            let _ = store.update_knowledge_source_status(
                employee_id,
                &path_str,
                KnowledgeSourceStatus::Failed,
                0,
                Some(msg.clone()),
            );
            Err(e)
        }
    }
}

/// Async entry: spawns blocking task per file. Returns immediately.
pub fn spawn_index_all(
    store: Arc<EmployeeStore>,
    app_storage: Arc<AppStorage>,
    employee_id: String,
    sources: Vec<(std::path::PathBuf, String)>,
) {
    for (path, original_name) in sources {
        let store = Arc::clone(&store);
        let app_storage = Arc::clone(&app_storage);
        let id = employee_id.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = index_one(&store, &app_storage, &id, &path, &original_name) {
                log::warn!("knowledge index failed for {}: {:#}", path.display(), e);
            }
        });
    }
}
