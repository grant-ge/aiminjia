# Pending Message Queue P1 — Core Module Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `runtime/pending/` core module — types, persistence, queue manager — fully tested in isolation. No transport adapters, no entry-point integration yet.

**Architecture:** Single `Mutex<HashMap<SessionId, SessionPending>>` source of truth. Each session has items + drain debounce timer + recently_drained guard. `pending.json` is the persistence mirror (atomic write). Manager does NOT depend on Tauri.

**Tech Stack:** Rust, `tokio`, `serde_json`, existing `storage::file_store::io::atomic_write_json`, existing `RuntimeRunRegistry` (busy check) and `RuntimeEventBus`.

**Spec reference:** `docs/superpowers/specs/2026-05-11-pending-message-queue-design.md` §4–6, §10–12

---

## File Structure

Create:

- `src-tauri/src/runtime/pending/mod.rs` — module entry; pub use types and `PendingQueueManager`
- `src-tauri/src/runtime/pending/types.rs` — `PendingItem`, `PendingSource`, `PendingAttachment`, `PendingFileFormat`, `EnqueueOutcome`, `EnqueueRejection`, `PendingConfig`
- `src-tauri/src/runtime/pending/store.rs` — `pending.json` read/write helpers (pure, no async)
- `src-tauri/src/runtime/pending/queue_manager.rs` — `PendingQueueManager` with `enqueue_or_send` / `schedule_drain` / `drain_and_dispatch` / `remove_item` / `snapshot` / `restore_from_disk`

Modify:

- `src-tauri/src/runtime/mod.rs` — add `pub mod pending;`
- `src-tauri/src/runtime/events.rs` — add 4 new `RuntimeEventKind` variants (`PendingSnapshot` / `PendingQueued` / `PendingDrained` / `PendingRemoved`)

Tests:

- `src-tauri/src/runtime/pending/types_test.rs` — schema serde roundtrip
- `src-tauri/src/runtime/pending/store_test.rs` — atomic write, corruption fallback, schema version handling
- `src-tauri/src/runtime/pending/queue_manager_test.rs` — unit tests for all 8 manager scenarios

---

## Task 1: Module skeleton + RuntimeEventKind variants

**Files:**
- Create: `src-tauri/src/runtime/pending/mod.rs`
- Create: `src-tauri/src/runtime/pending/types.rs` (empty stub)
- Modify: `src-tauri/src/runtime/mod.rs` (add `pub mod pending;`)
- Modify: `src-tauri/src/runtime/events.rs` (add 4 variants)

- [ ] **Step 1: Add module declarations**

Add to `src-tauri/src/runtime/mod.rs` after the existing `pub mod` lines (alphabetical: after `path_auth`):

```rust
pub mod pending;
```

Create `src-tauri/src/runtime/pending/mod.rs`:

```rust
//! Pending message queue: per-session queue with debounced drain.
//!
//! See `docs/superpowers/specs/2026-05-11-pending-message-queue-design.md`.

pub mod types;
pub mod store;
pub mod queue_manager;

pub use types::{
    EnqueueOutcome, EnqueueRejection, PendingAttachment, PendingConfig, PendingFileFormat,
    PendingItem, PendingSource,
};
pub use queue_manager::PendingQueueManager;
```

Create stub `src-tauri/src/runtime/pending/types.rs`:

```rust
//! Pending queue data types — see spec §4.1.
```

Create stub `src-tauri/src/runtime/pending/store.rs`:

```rust
//! pending.json read/write — see spec §4.3.
```

Create stub `src-tauri/src/runtime/pending/queue_manager.rs`:

```rust
//! PendingQueueManager — see spec §5.
```

- [ ] **Step 2: Add 4 RuntimeEventKind variants**

In `src-tauri/src/runtime/events.rs`, add to `RuntimeEventKind` enum (after `MessagePersisted` variant, before `TurnCompleted`):

```rust
PendingSnapshot {
    items: Vec<crate::runtime::pending::PendingItem>,
},
PendingQueued {
    item: crate::runtime::pending::PendingItem,
},
PendingDrained {
    drained_ids: Vec<String>,
},
PendingRemoved {
    item_id: String,
},
```

- [ ] **Step 3: Verify it compiles**

Run: `cd src-tauri && cargo check --lib`

Expected: compiles with warnings only about unused module items.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/runtime/pending/ src-tauri/src/runtime/mod.rs src-tauri/src/runtime/events.rs
git commit -m "feat(pending): module skeleton + RuntimeEventKind variants"
```

---

## Task 2: PendingItem types + serde roundtrip test

**Files:**
- Modify: `src-tauri/src/runtime/pending/types.rs`
- Create: `src-tauri/src/runtime/pending/types_test.rs`
- Modify: `src-tauri/src/runtime/pending/mod.rs` (add `#[cfg(test)] mod types_test;`)

- [ ] **Step 1: Write the failing test**

Create `src-tauri/src/runtime/pending/types_test.rs`:

```rust
use super::types::*;

#[test]
fn pending_item_roundtrip_camel_case() {
    let item = PendingItem {
        id: "pend-abc".into(),
        source: PendingSource::ImDingtalk,
        text: "hello".into(),
        sender_nick: Some("张三".into()),
        attachments: vec![PendingAttachment {
            id: "att-1".into(),
            file_path: "/tmp/foo.png".into(),
            mime: Some("image/png".into()),
            size_bytes: Some(1024),
        }],
        received_at: "2026-05-11T03:21:00Z".into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains("\"senderNick\":\"张三\""));
    assert!(json.contains("\"imDingtalk\"") || json.contains("\"im-dingtalk\""));
    let back: PendingItem = serde_json::from_str(&json).unwrap();
    assert_eq!(back, item);
}

#[test]
fn pending_file_format_default_empty() {
    let f = PendingFileFormat::default();
    assert_eq!(f.schema_version, 0);
    assert!(f.items.is_empty());
}

#[test]
fn pending_file_format_v1_serializes() {
    let f = PendingFileFormat {
        schema_version: 1,
        items: vec![],
    };
    let json = serde_json::to_string(&f).unwrap();
    assert!(json.contains("\"schemaVersion\":1"));
}

#[test]
fn pending_source_kebab_case() {
    let s = serde_json::to_string(&PendingSource::ImDingtalk).unwrap();
    assert_eq!(s, "\"im-dingtalk\"");
    let s2 = serde_json::to_string(&PendingSource::App).unwrap();
    assert_eq!(s2, "\"app\"");
}
```

Add to `src-tauri/src/runtime/pending/mod.rs`:

```rust
#[cfg(test)]
mod types_test;
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib --package aijia pending::types_test`

Expected: FAIL — types don't exist yet.

- [ ] **Step 3: Implement the types**

Replace `src-tauri/src/runtime/pending/types.rs` content:

```rust
//! Pending queue data types — see spec §4.1.

use serde::{Deserialize, Serialize};

use crate::runtime::chat::ChatTurnRequest;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingItem {
    pub id: String,
    pub source: PendingSource,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_nick: Option<String>,
    #[serde(default)]
    pub attachments: Vec<PendingAttachment>,
    pub received_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingAttachment {
    pub id: String,
    pub file_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PendingSource {
    App,
    ImDingtalk,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingFileFormat {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub items: Vec<PendingItem>,
}

/// Result of `enqueue_or_send`.
#[derive(Debug)]
pub enum EnqueueOutcome {
    /// Session was idle — caller should consume the request (manager already
    /// queued the spawn).
    SentDirectly { request: ChatTurnRequest },
    /// Session was busy — item buffered.
    Queued { snapshot: Vec<PendingItem> },
    /// Item refused.
    Rejected { reason: EnqueueRejection },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnqueueRejection {
    QueueFull { limit: usize },
    SessionArchived,
}

#[derive(Debug, Clone)]
pub struct PendingConfig {
    pub debounce_window: std::time::Duration,
    pub max_queue_per_session: usize,
    pub recently_drained_ttl: std::time::Duration,
}

impl Default for PendingConfig {
    fn default() -> Self {
        Self {
            debounce_window: std::time::Duration::from_millis(1200),
            max_queue_per_session: 50,
            recently_drained_ttl: std::time::Duration::from_secs(600),
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --lib pending::types_test -- --nocapture`

Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/pending/
git commit -m "feat(pending): PendingItem types with camelCase serde + tests"
```

---

## Task 3: pending.json store helpers

**Files:**
- Modify: `src-tauri/src/runtime/pending/store.rs`
- Create: `src-tauri/src/runtime/pending/store_test.rs`
- Modify: `src-tauri/src/runtime/pending/mod.rs` (add test mod)

- [ ] **Step 1: Write the failing test**

Create `src-tauri/src/runtime/pending/store_test.rs`:

```rust
use std::path::PathBuf;
use tempfile::TempDir;

use super::store::*;
use super::types::*;

fn sample_item(id: &str) -> PendingItem {
    PendingItem {
        id: id.into(),
        source: PendingSource::App,
        text: "hi".into(),
        sender_nick: None,
        attachments: vec![],
        received_at: "2026-05-11T03:21:00Z".into(),
    }
}

#[test]
fn read_pending_returns_empty_when_file_missing() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("pending.json");
    let items = read_pending(&path).unwrap();
    assert!(items.is_empty());
}

#[test]
fn write_then_read_pending() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("pending.json");
    let items = vec![sample_item("a"), sample_item("b")];
    write_pending(&path, &items).unwrap();
    let back = read_pending(&path).unwrap();
    assert_eq!(back, items);
}

#[test]
fn write_empty_creates_v1_empty_file() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("pending.json");
    write_pending(&path, &[]).unwrap();
    assert!(path.exists());
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("\"schemaVersion\": 1"));
    assert!(content.contains("\"items\": []"));
}

#[test]
fn read_pending_corrupt_file_returns_empty_and_logs() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("pending.json");
    std::fs::write(&path, "{this is not json").unwrap();
    let items = read_pending(&path).unwrap();
    assert!(items.is_empty());
}

#[test]
fn read_pending_with_unknown_schema_returns_empty() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("pending.json");
    std::fs::write(&path, r#"{"schemaVersion":99,"items":[]}"#).unwrap();
    let items = read_pending(&path).unwrap();
    assert!(items.is_empty());
}

#[test]
fn scan_pending_files_under_dir() {
    let tmp = TempDir::new().unwrap();
    let conv_a = tmp.path().join("conv-a");
    let conv_b = tmp.path().join("conv-b");
    std::fs::create_dir_all(&conv_a).unwrap();
    std::fs::create_dir_all(&conv_b).unwrap();
    write_pending(&conv_a.join("pending.json"), &[sample_item("a1")]).unwrap();
    write_pending(&conv_b.join("pending.json"), &[]).unwrap();
    let found = scan_conversation_pending(tmp.path()).unwrap();
    // Only conv-a has non-empty pending
    let conv_a_id = "conv-a".to_string();
    let entry = found.iter().find(|(id, _)| *id == conv_a_id);
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().1.len(), 1);
}
```

Add to `src-tauri/src/runtime/pending/mod.rs`:

```rust
#[cfg(test)]
mod store_test;
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib pending::store_test`

Expected: FAIL — `read_pending` / `write_pending` / `scan_conversation_pending` not defined.

- [ ] **Step 3: Implement the store**

Replace `src-tauri/src/runtime/pending/store.rs` content:

```rust
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --lib pending::store_test -- --nocapture`

Expected: 6 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/pending/
git commit -m "feat(pending): pending.json read/write + scan helpers"
```

---

## Task 4: PendingQueueManager — enqueue_or_send (idle path)

**Files:**
- Modify: `src-tauri/src/runtime/pending/queue_manager.rs`
- Create: `src-tauri/src/runtime/pending/queue_manager_test.rs`
- Modify: `src-tauri/src/runtime/pending/mod.rs` (add test mod)

- [ ] **Step 1: Write the failing test**

Create `src-tauri/src/runtime/pending/queue_manager_test.rs`:

```rust
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::ids::SessionId;
use crate::runtime::pending::queue_manager::*;
use crate::runtime::pending::types::*;
use crate::runtime::run_registry::RuntimeRunRegistry;

/// Test resolver that maps SessionId → tmp-dir/{session_id}/
struct TempConvDirResolver(PathBuf);

impl ConvDirResolver for TempConvDirResolver {
    fn conversation_dir(&self, session_id: &SessionId) -> Option<PathBuf> {
        let dir = self.0.join(session_id.as_str());
        std::fs::create_dir_all(&dir).ok()?;
        Some(dir)
    }

    fn is_archived(&self, _session_id: &SessionId) -> bool {
        false
    }

    fn conversations_root(&self) -> PathBuf {
        self.0.clone()
    }
}

fn build_manager(tmp: &TempDir) -> (Arc<PendingQueueManager>, Arc<RuntimeRunRegistry>) {
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(TempConvDirResolver(tmp.path().to_path_buf()));
    let mgr = Arc::new(PendingQueueManager::new(
        registry.clone(),
        bus,
        resolver,
        PendingConfig::default(),
    ));
    (mgr, registry)
}

fn sample_item(id: &str) -> PendingItem {
    PendingItem {
        id: id.into(),
        source: PendingSource::App,
        text: format!("text for {id}"),
        sender_nick: None,
        attachments: vec![],
        received_at: "2026-05-11T03:21:00Z".into(),
    }
}

#[tokio::test]
async fn enqueue_idle_session_returns_sent_directly() {
    let tmp = TempDir::new().unwrap();
    let (mgr, _registry) = build_manager(&tmp);
    let session = SessionId::new("conv-1");
    let item = sample_item("pend-a");
    let outcome = mgr.enqueue_or_send(session.clone(), item.clone()).await.unwrap();
    match outcome {
        EnqueueOutcome::SentDirectly { request } => {
            assert_eq!(request.conversation_id.as_str(), "conv-1");
            assert_eq!(request.content, "text for pend-a");
        }
        other => panic!("expected SentDirectly, got {:?}", other),
    }
    // Queue empty
    assert!(mgr.snapshot(&session).await.is_empty());
}
```

Add to `src-tauri/src/runtime/pending/mod.rs`:

```rust
#[cfg(test)]
mod queue_manager_test;
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib pending::queue_manager_test::enqueue_idle`

Expected: FAIL — `PendingQueueManager` / `ConvDirResolver` not defined.

- [ ] **Step 3: Implement minimal manager + ConvDirResolver trait**

Replace `src-tauri/src/runtime/pending/queue_manager.rs` content:

```rust
//! PendingQueueManager — see spec §5.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::Result;
use tokio::task::JoinHandle;

use crate::runtime::chat::{ChatAttachmentRef, ChatTurnRequest};
use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::ids::SessionId;
use crate::runtime::run_registry::RuntimeRunRegistry;

use super::types::{
    EnqueueOutcome, EnqueueRejection, PendingConfig, PendingItem,
};

/// Per-host abstraction over conversation directory layout.
pub trait ConvDirResolver: Send + Sync {
    fn conversation_dir(&self, session_id: &SessionId) -> Option<PathBuf>;
    fn is_archived(&self, session_id: &SessionId) -> bool;
    fn conversations_root(&self) -> PathBuf;
}

struct SessionPending {
    items: Vec<PendingItem>,
    drain_timer: Option<JoinHandle<()>>,
    recently_drained: VecDeque<(String, Instant)>,
}

impl SessionPending {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            drain_timer: None,
            recently_drained: VecDeque::new(),
        }
    }
}

pub struct PendingQueueManager {
    inner: Mutex<HashMap<SessionId, SessionPending>>,
    run_registry: Arc<RuntimeRunRegistry>,
    #[allow(dead_code)]
    event_bus: Arc<RuntimeEventBus>,
    resolver: Arc<dyn ConvDirResolver>,
    config: PendingConfig,
}

impl PendingQueueManager {
    pub fn new(
        run_registry: Arc<RuntimeRunRegistry>,
        event_bus: Arc<RuntimeEventBus>,
        resolver: Arc<dyn ConvDirResolver>,
        config: PendingConfig,
    ) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            run_registry,
            event_bus,
            resolver,
            config,
        }
    }

    /// Snapshot the current pending items for a session (UI / test helper).
    pub async fn snapshot(&self, session_id: &SessionId) -> Vec<PendingItem> {
        let guard = self.inner.lock().expect("pending mutex poisoned");
        guard
            .get(session_id)
            .map(|sp| sp.items.clone())
            .unwrap_or_default()
    }

    /// Enqueue the item if the session is busy; otherwise return a ChatTurnRequest
    /// for the caller to dispatch immediately.
    pub async fn enqueue_or_send(
        &self,
        session_id: SessionId,
        item: PendingItem,
    ) -> Result<EnqueueOutcome> {
        // 1. Archive check (outside lock — read-only resolver call)
        if self.resolver.is_archived(&session_id) {
            return Ok(EnqueueOutcome::Rejected {
                reason: EnqueueRejection::SessionArchived,
            });
        }

        // 2. Lock + busy check + push
        let busy = self.run_registry.is_session_busy(session_id.as_str());
        let mut guard = self.inner.lock().expect("pending mutex poisoned");
        let sp = guard.entry(session_id.clone()).or_insert_with(SessionPending::new);

        if !busy {
            // Idle path — caller will send immediately.
            // Make sure we don't leak a stale queue if any (shouldn't be here in idle path,
            // but defensive: snapshot is empty).
            drop(guard);
            let request = build_request_from_single(&session_id, item);
            return Ok(EnqueueOutcome::SentDirectly { request });
        }

        // Busy path: enforce queue limit.
        if sp.items.len() >= self.config.max_queue_per_session {
            return Ok(EnqueueOutcome::Rejected {
                reason: EnqueueRejection::QueueFull {
                    limit: self.config.max_queue_per_session,
                },
            });
        }

        sp.items.push(item);
        let snapshot = sp.items.clone();
        drop(guard);
        // Persistence + events + debounce scheduling happen in Task 5/6.
        Ok(EnqueueOutcome::Queued { snapshot })
    }
}

fn build_request_from_single(session_id: &SessionId, item: PendingItem) -> ChatTurnRequest {
    let attachments: Vec<ChatAttachmentRef> = item
        .attachments
        .iter()
        .map(|a| ChatAttachmentRef {
            id: a.id.clone(),
            file_name: file_name_of(&a.file_path),
            file_path: a.file_path.clone(),
            kind: "file".to_string(),
            file_size: a.size_bytes.unwrap_or(0),
            file_type: a
                .mime
                .clone()
                .unwrap_or_else(|| "application/octet-stream".to_string()),
            mime_type: a.mime.clone(),
        })
        .collect();
    ChatTurnRequest::new(session_id.clone(), item.text, attachments)
}

fn file_name_of(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .map(String::from)
        .unwrap_or_else(|| path.to_string())
}
```

Note: `ConvDirResolver` is exported via mod.rs:

```rust
// add to src-tauri/src/runtime/pending/mod.rs pub uses:
pub use queue_manager::{ConvDirResolver, PendingQueueManager};
```

Add `tempfile` to `[dev-dependencies]` in `src-tauri/Cargo.toml` if not already present:

```bash
grep -q "^tempfile" src-tauri/Cargo.toml || echo "Already present? double-check"
```

(It is already present per existing tests.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --lib pending::queue_manager_test::enqueue_idle -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/pending/
git commit -m "feat(pending): PendingQueueManager skeleton + idle enqueue path"
```

---

## Task 5: enqueue_or_send (busy path) + persistence + events

**Files:**
- Modify: `src-tauri/src/runtime/pending/queue_manager.rs`
- Modify: `src-tauri/src/runtime/pending/queue_manager_test.rs`

- [ ] **Step 1: Write the failing test**

Append to `src-tauri/src/runtime/pending/queue_manager_test.rs`:

```rust
use crate::runtime::ids::RunId;

#[tokio::test]
async fn enqueue_busy_session_queues_and_persists() {
    let tmp = TempDir::new().unwrap();
    let (mgr, registry) = build_manager(&tmp);
    let session = SessionId::new("conv-busy");

    // Reserve a run to mark session busy.
    registry
        .reserve(session.as_str(), RunId::new("run-1"))
        .unwrap();

    let item = sample_item("pend-1");
    let outcome = mgr
        .enqueue_or_send(session.clone(), item.clone())
        .await
        .unwrap();

    match outcome {
        EnqueueOutcome::Queued { snapshot } => {
            assert_eq!(snapshot.len(), 1);
            assert_eq!(snapshot[0].id, "pend-1");
        }
        other => panic!("expected Queued, got {:?}", other),
    }

    // Item still in memory
    let snap = mgr.snapshot(&session).await;
    assert_eq!(snap.len(), 1);

    // pending.json on disk has it
    let pending_path = tmp.path().join("conv-busy").join("pending.json");
    // wait briefly for the spawned write task
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(pending_path.exists(), "pending.json should exist");
    let content = std::fs::read_to_string(&pending_path).unwrap();
    assert!(content.contains("pend-1"));
}

#[tokio::test]
async fn enqueue_busy_full_queue_rejects() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(TempConvDirResolver(tmp.path().to_path_buf()));
    let mut config = PendingConfig::default();
    config.max_queue_per_session = 2;
    let mgr = Arc::new(PendingQueueManager::new(
        registry.clone(),
        bus,
        resolver,
        config,
    ));
    let session = SessionId::new("conv-full");
    registry.reserve(session.as_str(), RunId::new("run-1")).unwrap();

    mgr.enqueue_or_send(session.clone(), sample_item("a")).await.unwrap();
    mgr.enqueue_or_send(session.clone(), sample_item("b")).await.unwrap();
    let outcome = mgr.enqueue_or_send(session.clone(), sample_item("c")).await.unwrap();

    match outcome {
        EnqueueOutcome::Rejected {
            reason: EnqueueRejection::QueueFull { limit: 2 },
        } => {}
        other => panic!("expected QueueFull(2), got {:?}", other),
    }
}

struct ArchivedResolver(PathBuf);
impl ConvDirResolver for ArchivedResolver {
    fn conversation_dir(&self, sid: &SessionId) -> Option<PathBuf> {
        let d = self.0.join(sid.as_str());
        std::fs::create_dir_all(&d).ok()?;
        Some(d)
    }
    fn is_archived(&self, _sid: &SessionId) -> bool {
        true
    }
    fn conversations_root(&self) -> PathBuf {
        self.0.clone()
    }
}

#[tokio::test]
async fn enqueue_archived_session_rejects() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(ArchivedResolver(tmp.path().to_path_buf()));
    let mgr = Arc::new(PendingQueueManager::new(
        registry,
        bus,
        resolver,
        PendingConfig::default(),
    ));
    let session = SessionId::new("conv-archived");
    let outcome = mgr.enqueue_or_send(session, sample_item("x")).await.unwrap();
    assert!(matches!(
        outcome,
        EnqueueOutcome::Rejected {
            reason: EnqueueRejection::SessionArchived
        }
    ));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test --lib pending::queue_manager_test`

Expected: 3 new tests FAIL — persistence + reject paths not wired.

- [ ] **Step 3: Implement persistence on busy enqueue**

Modify `enqueue_or_send` in `queue_manager.rs` — after the `sp.items.push(item);` line, before returning Queued:

```rust
        sp.items.push(item.clone());
        let snapshot = sp.items.clone();
        drop(guard);

        // Persist (spawn — never block enqueue on disk IO)
        if let Some(dir) = self.resolver.conversation_dir(&session_id) {
            let path = dir.join("pending.json");
            let items_for_write = snapshot.clone();
            tokio::task::spawn_blocking(move || {
                if let Err(e) = super::store::write_pending(&path, &items_for_write) {
                    log::warn!("[pending] write_pending failed: {:#}", e);
                }
            });
        }

        // Emit event for UI
        let event = crate::runtime::events::RuntimeEvent::new(
            session_id.clone(),
            crate::runtime::ids::RunId::new("pending"),
            crate::runtime::events::RuntimeEventKind::PendingQueued { item },
        );
        if let Err(e) = self.event_bus.emit(event).await {
            log::warn!("[pending] emit PendingQueued failed: {:#}", e);
        }

        Ok(EnqueueOutcome::Queued { snapshot })
```

Replace the `#[allow(dead_code)]` on `event_bus` field — it's now used.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --lib pending::queue_manager_test`

Expected: all 4 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/pending/
git commit -m "feat(pending): busy-path queueing + pending.json persistence + event emit"
```

---

## Task 6: schedule_drain + drain_and_dispatch (without ChatAdapter)

**Files:**
- Modify: `src-tauri/src/runtime/pending/queue_manager.rs`
- Modify: `src-tauri/src/runtime/pending/queue_manager_test.rs`

This task introduces the drain debounce timer and the drain logic. Dispatching to a real `ChatAdapter` is wired in Task 8; here we abstract it behind a trait so we can unit-test drain in isolation.

- [ ] **Step 1: Add ChatTurnDispatcher trait + write the failing test**

Append to `src-tauri/src/runtime/pending/queue_manager.rs` (at module top, after imports):

```rust
/// Abstraction over "send a ChatTurnRequest" — production wires to
/// `TauriChatCommandAdapter::send_chat_request` (Task 8), tests use a fake.
#[async_trait::async_trait]
pub trait ChatTurnDispatcher: Send + Sync {
    async fn dispatch(&self, request: ChatTurnRequest) -> Result<()>;
}
```

Also import `async_trait` at top:

```rust
use async_trait::async_trait;
```

(`async-trait` is already in `Cargo.toml` per existing runtime code.)

Append to `src-tauri/src/runtime/pending/queue_manager_test.rs`:

```rust
use std::sync::atomic::{AtomicUsize, Ordering};

struct CountingDispatcher {
    pub count: AtomicUsize,
    pub last_text: tokio::sync::Mutex<Option<String>>,
}

#[async_trait::async_trait]
impl ChatTurnDispatcher for CountingDispatcher {
    async fn dispatch(&self, request: ChatTurnRequest) -> Result<()> {
        self.count.fetch_add(1, Ordering::SeqCst);
        *self.last_text.lock().await = Some(request.content);
        Ok(())
    }
}

#[tokio::test]
async fn drain_dispatches_after_debounce() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(TempConvDirResolver(tmp.path().to_path_buf()));
    let mut config = PendingConfig::default();
    config.debounce_window = std::time::Duration::from_millis(50);
    let dispatcher = Arc::new(CountingDispatcher {
        count: AtomicUsize::new(0),
        last_text: tokio::sync::Mutex::new(None),
    });
    let mgr = Arc::new(PendingQueueManager::new(
        registry.clone(),
        bus,
        resolver,
        config,
    ));
    mgr.set_dispatcher(dispatcher.clone()).await;

    let session = SessionId::new("conv-drain");
    registry.reserve(session.as_str(), RunId::new("run-1")).unwrap();
    mgr.enqueue_or_send(session.clone(), sample_item("a")).await.unwrap();
    mgr.enqueue_or_send(session.clone(), sample_item("b")).await.unwrap();

    // Release busy + schedule_drain
    registry.clear(session.as_str());
    mgr.schedule_drain(session.clone()).await;

    // Wait > debounce
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    assert_eq!(dispatcher.count.load(Ordering::SeqCst), 1, "dispatched once");
    let text = dispatcher.last_text.lock().await.clone().unwrap();
    assert!(text.contains("text for a"));
    assert!(text.contains("text for b"));

    // Queue empty after drain
    assert!(mgr.snapshot(&session).await.is_empty());

    // pending.json empty
    let path = tmp.path().join("conv-drain").join("pending.json");
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("\"items\": []"));
}

#[tokio::test]
async fn drain_skipped_when_session_busy() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(TempConvDirResolver(tmp.path().to_path_buf()));
    let mut config = PendingConfig::default();
    config.debounce_window = std::time::Duration::from_millis(50);
    let dispatcher = Arc::new(CountingDispatcher {
        count: AtomicUsize::new(0),
        last_text: tokio::sync::Mutex::new(None),
    });
    let mgr = Arc::new(PendingQueueManager::new(
        registry.clone(),
        bus,
        resolver,
        config,
    ));
    mgr.set_dispatcher(dispatcher.clone()).await;

    let session = SessionId::new("conv-busy-drain");
    registry.reserve(session.as_str(), RunId::new("run-1")).unwrap();
    mgr.enqueue_or_send(session.clone(), sample_item("a")).await.unwrap();

    // Don't clear busy. schedule_drain anyway.
    mgr.schedule_drain(session.clone()).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    assert_eq!(dispatcher.count.load(Ordering::SeqCst), 0);
    assert_eq!(mgr.snapshot(&session).await.len(), 1, "still queued");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test --lib pending::queue_manager_test::drain`

Expected: FAIL — `schedule_drain` / `set_dispatcher` not defined.

- [ ] **Step 3: Implement drain logic**

Modify `PendingQueueManager` struct in `queue_manager.rs` to add dispatcher:

```rust
pub struct PendingQueueManager {
    inner: Mutex<HashMap<SessionId, SessionPending>>,
    run_registry: Arc<RuntimeRunRegistry>,
    event_bus: Arc<RuntimeEventBus>,
    resolver: Arc<dyn ConvDirResolver>,
    config: PendingConfig,
    dispatcher: tokio::sync::RwLock<Option<Arc<dyn ChatTurnDispatcher>>>,
    self_arc: std::sync::OnceLock<std::sync::Weak<Self>>,
}
```

Update `new`:

```rust
pub fn new(
    run_registry: Arc<RuntimeRunRegistry>,
    event_bus: Arc<RuntimeEventBus>,
    resolver: Arc<dyn ConvDirResolver>,
    config: PendingConfig,
) -> Arc<Self> {
    let mgr = Arc::new(Self {
        inner: Mutex::new(HashMap::new()),
        run_registry,
        event_bus,
        resolver,
        config,
        dispatcher: tokio::sync::RwLock::new(None),
        self_arc: std::sync::OnceLock::new(),
    });
    let _ = mgr.self_arc.set(Arc::downgrade(&mgr));
    mgr
}
```

Note: this changes `new` return type from `Self` to `Arc<Self>`. Update call sites in tests accordingly (remove `Arc::new(...)` wrapper).

Add methods:

```rust
pub async fn set_dispatcher(&self, dispatcher: Arc<dyn ChatTurnDispatcher>) {
    *self.dispatcher.write().await = Some(dispatcher);
}

/// Start (or reset) the debounce timer for a session. Called after StreamDone
/// and after busy-path enqueue.
pub async fn schedule_drain(&self, session_id: SessionId) {
    let debounce = self.config.debounce_window;
    let weak = self.self_arc.get().cloned().unwrap_or_default();

    let mut guard = self.inner.lock().expect("pending mutex poisoned");
    let Some(sp) = guard.get_mut(&session_id) else {
        return;
    };
    if sp.items.is_empty() {
        return;
    }
    if let Some(old) = sp.drain_timer.take() {
        old.abort();
    }
    let sid_clone = session_id.clone();
    let handle = tokio::spawn(async move {
        tokio::time::sleep(debounce).await;
        if let Some(mgr) = weak.upgrade() {
            mgr.drain_and_dispatch(sid_clone).await;
        }
    });
    sp.drain_timer = Some(handle);
}

async fn drain_and_dispatch(&self, session_id: SessionId) {
    // 1. Take items (with re-check is_busy)
    let items_opt: Option<Vec<PendingItem>> = {
        let mut guard = self.inner.lock().expect("pending mutex poisoned");
        let Some(sp) = guard.get_mut(&session_id) else {
            return;
        };
        if self.run_registry.is_session_busy(session_id.as_str()) {
            log::info!(
                "[pending] drain skipped — session {} still busy",
                session_id.as_str()
            );
            return;
        }
        if sp.items.is_empty() {
            return;
        }
        let taken = std::mem::take(&mut sp.items);
        sp.drain_timer = None;
        let now = Instant::now();
        for it in &taken {
            sp.recently_drained.push_back((it.id.clone(), now));
        }
        Self::trim_recently_drained(sp, self.config.recently_drained_ttl);
        Some(taken)
    };

    let Some(items) = items_opt else { return };
    let drained_ids: Vec<String> = items.iter().map(|i| i.id.clone()).collect();

    // 2. Persist empty file
    if let Some(dir) = self.resolver.conversation_dir(&session_id) {
        let path = dir.join("pending.json");
        if let Err(e) = tokio::task::spawn_blocking(move || super::store::write_pending(&path, &[]))
            .await
            .unwrap_or_else(|join_err| Err(std::io::Error::new(std::io::ErrorKind::Other, join_err)))
        {
            log::warn!("[pending] clearing pending.json failed: {:#}", e);
        }
    }

    // 3. Emit drained event
    let event = crate::runtime::events::RuntimeEvent::new(
        session_id.clone(),
        crate::runtime::ids::RunId::new("pending"),
        crate::runtime::events::RuntimeEventKind::PendingDrained {
            drained_ids: drained_ids.clone(),
        },
    );
    if let Err(e) = self.event_bus.emit(event).await {
        log::warn!("[pending] emit PendingDrained failed: {:#}", e);
    }

    // 4. Build merged request and dispatch
    let request = build_request_from_batch(&session_id, items);
    let dispatcher = self.dispatcher.read().await.clone();
    let Some(dispatcher) = dispatcher else {
        log::warn!("[pending] no dispatcher set; drained items dropped");
        return;
    };
    if let Err(e) = dispatcher.dispatch(request).await {
        log::error!("[pending] dispatcher failed for session {}: {:#}", session_id.as_str(), e);
    }
}

fn trim_recently_drained(sp: &mut SessionPending, ttl: std::time::Duration) {
    let cutoff = Instant::now().checked_sub(ttl);
    if let Some(cutoff) = cutoff {
        while sp
            .recently_drained
            .front()
            .map(|(_, t)| *t < cutoff)
            .unwrap_or(false)
        {
            sp.recently_drained.pop_front();
        }
    }
}
```

Add merger:

```rust
fn build_request_from_batch(session_id: &SessionId, items: Vec<PendingItem>) -> ChatTurnRequest {
    let n = items.len();
    let mut content = String::new();
    if n > 1 {
        content.push_str(&format!("[以下是 {} 条新消息]\n", n));
    }
    let mut all_atts: Vec<ChatAttachmentRef> = Vec::new();
    for (idx, it) in items.iter().enumerate() {
        let prefix = match &it.sender_nick {
            Some(nick) if !nick.is_empty() => format!("[{}]: ", nick),
            _ => String::new(),
        };
        content.push_str(&prefix);
        content.push_str(&it.text);
        if idx + 1 < n {
            content.push('\n');
        }
        for a in &it.attachments {
            all_atts.push(ChatAttachmentRef {
                id: a.id.clone(),
                file_name: file_name_of(&a.file_path),
                file_path: a.file_path.clone(),
                kind: "file".to_string(),
                file_size: a.size_bytes.unwrap_or(0),
                file_type: a
                    .mime
                    .clone()
                    .unwrap_or_else(|| "application/octet-stream".to_string()),
                mime_type: a.mime.clone(),
            });
        }
    }
    ChatTurnRequest::new(session_id.clone(), content, all_atts)
}
```

Update test build_manager helper to remove the `Arc::new(...)` wrap since `new()` now returns `Arc<Self>`. Modify `queue_manager_test.rs::build_manager`:

```rust
fn build_manager(tmp: &TempDir) -> (Arc<PendingQueueManager>, Arc<RuntimeRunRegistry>) {
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(TempConvDirResolver(tmp.path().to_path_buf()));
    let mgr = PendingQueueManager::new(registry.clone(), bus, resolver, PendingConfig::default());
    (mgr, registry)
}
```

(Also remove `Arc::new(...)` from the two later builds in busy_full_queue and enqueue_archived tests; just use `PendingQueueManager::new(...)`.)

- [ ] **Step 4: Run tests**

Run: `cd src-tauri && cargo test --lib pending::queue_manager_test -- --nocapture`

Expected: all tests PASS (6 total: idle, busy_queues, busy_full, archived, drain_dispatches, drain_skipped_busy).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/pending/
git commit -m "feat(pending): schedule_drain + drain_and_dispatch with debounce timer"
```

---

## Task 7: remove_item + snapshot + recently_drained guard

**Files:**
- Modify: `src-tauri/src/runtime/pending/queue_manager.rs`
- Modify: `src-tauri/src/runtime/pending/queue_manager_test.rs`

- [ ] **Step 1: Write the failing test**

Append to `queue_manager_test.rs`:

```rust
#[tokio::test]
async fn remove_item_removes_from_memory_disk_and_emits_event() {
    let tmp = TempDir::new().unwrap();
    let (mgr, registry) = build_manager(&tmp);
    let session = SessionId::new("conv-remove");
    registry.reserve(session.as_str(), RunId::new("run-1")).unwrap();

    mgr.enqueue_or_send(session.clone(), sample_item("keep")).await.unwrap();
    mgr.enqueue_or_send(session.clone(), sample_item("drop")).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let removed = mgr.remove_item(&session, "drop").await.unwrap();
    assert!(removed);

    let snap = mgr.snapshot(&session).await;
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].id, "keep");

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let path = tmp.path().join("conv-remove").join("pending.json");
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(!content.contains("\"id\": \"drop\""));
    assert!(content.contains("\"id\": \"keep\""));
}

#[tokio::test]
async fn remove_item_missing_returns_false() {
    let tmp = TempDir::new().unwrap();
    let (mgr, _) = build_manager(&tmp);
    let session = SessionId::new("conv-missing");
    let removed = mgr.remove_item(&session, "nope").await.unwrap();
    assert!(!removed);
}

#[tokio::test]
async fn restore_from_disk_loads_pending_items() {
    let tmp = TempDir::new().unwrap();
    let conv_dir = tmp.path().join("conv-restore");
    std::fs::create_dir_all(&conv_dir).unwrap();
    let path = conv_dir.join("pending.json");
    crate::runtime::pending::store::write_pending(
        &path,
        &[sample_item("loaded-1"), sample_item("loaded-2")],
    )
    .unwrap();

    let (mgr, _) = build_manager(&tmp);
    mgr.restore_from_disk().await.unwrap();

    let session = SessionId::new("conv-restore");
    let snap = mgr.snapshot(&session).await;
    assert_eq!(snap.len(), 2);
}

#[tokio::test]
async fn drain_recently_drained_blocks_replay_after_restore() {
    let tmp = TempDir::new().unwrap();
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(TempConvDirResolver(tmp.path().to_path_buf()));
    let mut config = PendingConfig::default();
    config.debounce_window = std::time::Duration::from_millis(30);
    let dispatcher = Arc::new(CountingDispatcher {
        count: AtomicUsize::new(0),
        last_text: tokio::sync::Mutex::new(None),
    });
    let mgr = PendingQueueManager::new(registry.clone(), bus, resolver, config);
    mgr.set_dispatcher(dispatcher.clone()).await;
    let session = SessionId::new("conv-recent");
    registry.reserve(session.as_str(), RunId::new("run-1")).unwrap();
    mgr.enqueue_or_send(session.clone(), sample_item("once")).await.unwrap();
    registry.clear(session.as_str());
    mgr.schedule_drain(session.clone()).await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(dispatcher.count.load(Ordering::SeqCst), 1);

    // Simulate disk still has the item (drain wrote empty but pretend crash)
    let conv_dir = tmp.path().join("conv-recent");
    crate::runtime::pending::store::write_pending(
        &conv_dir.join("pending.json"),
        &[sample_item("once")],
    )
    .unwrap();

    mgr.restore_from_disk().await.unwrap();
    let snap = mgr.snapshot(&session).await;
    assert!(snap.is_empty(), "recently_drained should suppress replay");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test --lib pending::queue_manager_test::remove_item`

Expected: FAIL — `remove_item` / `restore_from_disk` not defined.

- [ ] **Step 3: Implement remove_item, restore_from_disk**

Append to `impl PendingQueueManager`:

```rust
/// Remove one item from the queue (UI × button).
pub async fn remove_item(&self, session_id: &SessionId, item_id: &str) -> Result<bool> {
    let (removed, snapshot_opt) = {
        let mut guard = self.inner.lock().expect("pending mutex poisoned");
        let Some(sp) = guard.get_mut(session_id) else {
            return Ok(false);
        };
        let before = sp.items.len();
        sp.items.retain(|i| i.id != item_id);
        let removed = sp.items.len() < before;
        (removed, removed.then(|| sp.items.clone()))
    };
    if !removed {
        return Ok(false);
    }

    // Persist new state
    if let (Some(snap), Some(dir)) = (snapshot_opt, self.resolver.conversation_dir(session_id)) {
        let path = dir.join("pending.json");
        tokio::task::spawn_blocking(move || {
            if let Err(e) = super::store::write_pending(&path, &snap) {
                log::warn!("[pending] write_pending on remove failed: {:#}", e);
            }
        });
    }

    let event = crate::runtime::events::RuntimeEvent::new(
        session_id.clone(),
        crate::runtime::ids::RunId::new("pending"),
        crate::runtime::events::RuntimeEventKind::PendingRemoved {
            item_id: item_id.to_string(),
        },
    );
    if let Err(e) = self.event_bus.emit(event).await {
        log::warn!("[pending] emit PendingRemoved failed: {:#}", e);
    }
    Ok(true)
}

/// Load pending.json from all conversations into memory. Items previously
/// drained (within TTL) are filtered.
pub async fn restore_from_disk(&self) -> Result<()> {
    let root = self.resolver.conversations_root();
    let scanned = tokio::task::spawn_blocking(move || super::store::scan_conversation_pending(&root))
        .await
        .map_err(|e| anyhow::anyhow!("join: {e}"))??;

    let mut guard = self.inner.lock().expect("pending mutex poisoned");
    for (conv_id, items) in scanned {
        let session_id = SessionId::new(conv_id);
        if self.resolver.is_archived(&session_id) {
            continue;
        }
        let sp = guard.entry(session_id).or_insert_with(SessionPending::new);
        for item in items {
            // Skip if recently drained
            let drained = sp.recently_drained.iter().any(|(id, _)| id == &item.id);
            if !drained {
                sp.items.push(item);
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Run tests**

Run: `cd src-tauri && cargo test --lib pending::queue_manager_test`

Expected: all 10 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/pending/
git commit -m "feat(pending): remove_item + restore_from_disk with recently_drained guard"
```

---

## Task 8: Whole-module check + integration with cargo workspace

**Files:**
- Verify: `src-tauri/src/runtime/pending/` compiles cleanly with `cargo check`
- Verify: all 10 unit tests pass
- Verify: no `use tauri::*` in `runtime/pending/` (architecture constraint)

- [ ] **Step 1: Run full module test**

Run: `cd src-tauri && cargo test --lib pending`

Expected: all pending tests pass (4 types + 6 store + 10 queue_manager = 20 total approximately).

- [ ] **Step 2: Architecture review — no Tauri dependency**

Run: `grep -rn "use tauri" src-tauri/src/runtime/pending/`

Expected: NO output (the module must be Tauri-free per spec §11.2).

- [ ] **Step 3: Run review_ tests to ensure existing constraints still hold**

Run: `cd src-tauri && cargo test review_ --tests --no-fail-fast`

Expected: same results as before this PR (no new failures).

- [ ] **Step 4: Final commit (no-op if no changes)**

```bash
git status
# If clean, skip. Otherwise:
git add -A && git commit -m "test(pending): P1 module complete"
```

---

## Self-Review

After completing all tasks, verify against spec §4–6:

1. **§4.1 PendingItem types** → Task 2 ✓
2. **§4.3 pending.json schema** → Task 3 ✓
3. **§5.2 PendingQueueManager API** → Tasks 4/5/6/7 (enqueue_or_send / schedule_drain / remove_item / snapshot / restore_from_disk all implemented; drain_and_dispatch internal)
4. **§5.3 enqueue lock invariant** → Task 4 (is_busy check inside same mutex section)
5. **§5.4 debounce reset** → Task 6 (drain_timer aborted on schedule_drain re-call)
6. **§5.5 drain re-checks busy** → Task 6 (drain_and_dispatch step 1 re-checks)
7. **§5.6 restore_from_disk** → Task 7 ✓
8. **§5.7 recently_drained** → Task 7 ✓
9. **§6.1 落库形态 N 条独立** → **Deferred to P3/P4** (this plan only builds the in-memory queue and a single merged `ChatTurnRequest`; persisting N独立 user messages to messages.jsonl is in P3/P4 when entries integrate with ConversationStore)
10. **§11.2 no `use tauri::*`** → Task 8 ✓

**Not covered in P1 (intentional):**
- ConvDirResolver production impl backed by `AiJiaHome` (P3 wires it)
- `messages.jsonl` writes on drain (P3/P4)
- non-Anthropic pre-merge (P5)
- multimodal cross-message budget (P5)
- Tauri event adapter + frontend (P2)
- Entry-point integrations (P3 / P4)
