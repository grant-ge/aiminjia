# App Data Governance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Govern AIjia desktop app data under `~/.renlijia/` by bounding logs, cleaning temporary files, migrating low-risk root artifacts into the default workspace, and producing a root layout review without breaking old-version upgrades.

**Architecture:** Add a focused `storage::app_data_governance` module with pure functions over explicit paths and `AiJiaHome`. Hook it into startup as best-effort after existing migrations. Preserve old data on failures and gate one-shot migrations through `global/state.json`.

**Tech Stack:** Rust, Tauri startup setup, `serde_json`, existing `storage::migration` state helpers, `tempfile` tests.

---

## Scope Update: 2026-06-01 Audit + Contract Guard Slice

The user approved the direction and expanded today's work from directory audit report enhancement to include a code-backed directory contract. Do not implement runtime cleanup or migration in this slice. First produce and review:

- updated references in the design doc
- `src-tauri/src/storage/app_data_contract.rs`

Implementation tasks below remain the next phase after the audit matrix is accepted.

## Completed Slice: Root Directory Contract

**Files:**
- Created: `src-tauri/src/storage/app_data_contract.rs`
- Modified: `src-tauri/src/storage/mod.rs`

Implemented behavior:

- Classify known root entries into `StableRoot`, `TransitionalRoot`, `WorkspaceArtifact`, `Temporary`, `DeprecatedArchiveCandidate`, and `ReviewOnly`.
- Keep unknown old-user entries non-blocking through runtime audit classification.
- Hard-fail tests when production code outside `storage/aijia_home.rs` adds a direct root join not declared in the contract.
- Register legacy `config.json` as `TransitionalRoot` because `data_version` still needs it for old-version `cloud_auth` recovery.

## File Structure

- Create: `src-tauri/src/storage/app_data_governance.rs`
  - Root entry classification.
  - Temporary file TTL cleanup.
  - Log bounding.
  - One-shot root artifact migration.
  - Startup governance report.
- Modify: `src-tauri/src/storage/mod.rs`
  - Export the new module.
- Modify: `src-tauri/src/lib.rs`
  - Call the best-effort governance pass during startup.
- Modify: `docs/superpowers/specs/2026-06-01-app-data-governance-design.md`
  - Keep design current if implementation changes.
- Modify: Lotus 服务端仓库 `docs/desktop/storage-conventions.md`
  - Add the new root artifact import and governance rules after implementation.

## Task 1: Root Entry Classification

**Files:**
- Create: `src-tauri/src/storage/app_data_governance.rs`
- Modify: `src-tauri/src/storage/mod.rs`

- [ ] **Step 1: Write the failing tests**

Add tests in `src-tauri/src/storage/app_data_governance.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_root_entries_without_touching_unknowns() {
        assert_eq!(classify_root_entry("global"), RootEntryClass::KeepRoot);
        assert_eq!(classify_root_entry("users"), RootEntryClass::KeepRoot);
        assert_eq!(classify_root_entry("analysis"), RootEntryClass::WorkspaceArtifact);
        assert_eq!(classify_root_entry("charts"), RootEntryClass::WorkspaceArtifact);
        assert_eq!(classify_root_entry("temp"), RootEntryClass::TemporaryLegacy);
        assert_eq!(classify_root_entry("tmpImage"), RootEntryClass::TemporaryLegacy);
        assert_eq!(classify_root_entry("logs"), RootEntryClass::KeepRoot);
        assert_eq!(
            classify_root_entry("expert-team-templates"),
            RootEntryClass::DeprecatedArchiveCandidate
        );
        assert_eq!(classify_root_entry("unknown-new-dir"), RootEntryClass::ReviewOnly);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd src-tauri && cargo test app_data_governance::tests::classifies_root_entries_without_touching_unknowns
```

Expected: FAIL because `app_data_governance` does not exist.

- [ ] **Step 3: Write minimal implementation**

Create `src-tauri/src/storage/app_data_governance.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootEntryClass {
    KeepRoot,
    TransitionalRoot,
    WorkspaceArtifact,
    TemporaryLegacy,
    DeprecatedArchiveCandidate,
    ReviewOnly,
}

pub fn classify_root_entry(name: &str) -> RootEntryClass {
    match name {
        "global" | "crypto" | "users" | "skills" | "employee-templates-cache"
        | "expert-team-templates-cache" | "runtimes" | "tmp" | "defaultFolder" | "logs"
        | "device_id" | "data_version" | ".migrated" => {
            RootEntryClass::KeepRoot
        }
        name if name.starts_with(".archived-legacy-") => RootEntryClass::KeepRoot,
        "expert-team-templates" => RootEntryClass::DeprecatedArchiveCandidate,
        "analysis" | "charts" | "generated" | "exports" | "reports" | "uploads" => {
            RootEntryClass::WorkspaceArtifact
        }
        "temp" | "tmpImage" => RootEntryClass::TemporaryLegacy,
        _ => RootEntryClass::ReviewOnly,
    }
}
```

Modify `src-tauri/src/storage/mod.rs`:

```rust
pub mod app_data_governance;
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cd src-tauri && cargo test app_data_governance::tests::classifies_root_entries_without_touching_unknowns
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/storage/mod.rs src-tauri/src/storage/app_data_governance.rs
git commit -m "feat(storage): classify app data root entries"
```

## Task 2: Temporary File TTL Cleanup

**Files:**
- Modify: `src-tauri/src/storage/app_data_governance.rs`

- [ ] **Step 1: Write the failing test**

Add:

```rust
#[test]
fn cleans_temp_files_older_than_ttl_and_keeps_recent_files() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("temp");
    std::fs::create_dir_all(&dir).unwrap();
    let old = dir.join("old.txt");
    let recent = dir.join("recent.txt");
    std::fs::write(&old, b"old").unwrap();
    std::fs::write(&recent, b"recent").unwrap();

    let now = std::time::SystemTime::now();
    set_file_mtime_for_test(&old, now - std::time::Duration::from_secs(9 * 86400));
    set_file_mtime_for_test(&recent, now);

    let report = cleanup_dir_by_ttl(&dir, std::time::Duration::from_secs(7 * 86400), now);

    assert_eq!(report.removed_files, 1);
    assert!(!old.exists());
    assert!(recent.exists());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd src-tauri && cargo test app_data_governance::tests::cleans_temp_files_older_than_ttl_and_keeps_recent_files
```

Expected: FAIL because cleanup helpers are missing.

- [ ] **Step 3: Write minimal implementation**

Implement:

```rust
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CleanupReport {
    pub removed_files: usize,
    pub skipped_files: usize,
}

pub fn cleanup_dir_by_ttl(
    dir: &std::path::Path,
    ttl: std::time::Duration,
    now: std::time::SystemTime,
) -> CleanupReport {
    let mut report = CleanupReport::default();
    let cutoff = now.checked_sub(ttl).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return report;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let nested = cleanup_dir_by_ttl(&path, ttl, now);
            report.removed_files += nested.removed_files;
            report.skipped_files += nested.skipped_files;
            let _ = std::fs::remove_dir(&path);
            continue;
        }
        let should_remove = path
            .metadata()
            .and_then(|meta| meta.modified())
            .map(|modified| modified < cutoff)
            .unwrap_or(false);
        if should_remove {
            if std::fs::remove_file(&path).is_ok() {
                report.removed_files += 1;
            } else {
                report.skipped_files += 1;
            }
        }
    }
    report
}
```

Add `filetime = "0.2"` under `[dev-dependencies]` in `src-tauri/Cargo.toml` and implement this test helper:

```rust
#[cfg(test)]
fn set_file_mtime_for_test(path: &std::path::Path, time: std::time::SystemTime) {
    let ft = filetime::FileTime::from_system_time(time);
    filetime::set_file_mtime(path, ft).unwrap();
}
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cd src-tauri && cargo test app_data_governance::tests::cleans_temp_files_older_than_ttl_and_keeps_recent_files
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/storage/app_data_governance.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(storage): clean legacy temp files by ttl"
```

## Task 3: Log Bounding

**Files:**
- Modify: `src-tauri/src/storage/app_data_governance.rs`

- [ ] **Step 1: Write the failing test**

Add:

```rust
#[test]
fn bounds_metrics_logs_without_deleting_active_files() {
    let tmp = tempfile::tempdir().unwrap();
    let logs = tmp.path().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::write(logs.join("renlijia.log"), b"active").unwrap();
    std::fs::write(logs.join("metrics.jsonl"), b"active").unwrap();
    for i in 0..25 {
        std::fs::write(logs.join(format!("metrics.{i}.jsonl")), b"old").unwrap();
    }

    let report = bound_metrics_logs(&logs, 20);

    assert_eq!(report.removed_files, 5);
    assert!(logs.join("renlijia.log").exists());
    assert!(logs.join("metrics.jsonl").exists());
    let remaining_metrics = std::fs::read_dir(&logs)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("metrics."))
        .count();
    assert_eq!(remaining_metrics, 20);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd src-tauri && cargo test app_data_governance::tests::bounds_metrics_logs_without_deleting_active_files
```

Expected: FAIL because `bound_metrics_logs` does not exist.

- [ ] **Step 3: Write minimal implementation**

Implement:

```rust
pub fn bound_metrics_logs(logs_dir: &std::path::Path, keep_count: usize) -> CleanupReport {
    let mut report = CleanupReport::default();
    let Ok(entries) = std::fs::read_dir(logs_dir) else {
        return report;
    };

    let mut numbered: Vec<(u64, std::path::Path)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_string_lossy();
            let suffix = name
                .strip_prefix("metrics.")
                .and_then(|s| s.strip_suffix(".jsonl"))?;
            let n = suffix.parse::<u64>().ok()?;
            Some((n, path))
        })
        .collect();

    numbered.sort_by_key(|(n, _)| *n);
    let remove_count = numbered.len().saturating_sub(keep_count);
    for (_, path) in numbered.into_iter().take(remove_count) {
        if std::fs::remove_file(&path).is_ok() {
            report.removed_files += 1;
        } else {
            report.skipped_files += 1;
        }
    }
    report
}
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cd src-tauri && cargo test app_data_governance::tests::bounds_metrics_logs_without_deleting_active_files
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/storage/app_data_governance.rs
git commit -m "feat(storage): bound app diagnostic log files"
```

## Task 4: One-Shot Root Artifact Migration

**Files:**
- Modify: `src-tauri/src/storage/app_data_governance.rs`

- [ ] **Step 1: Write the failing test**

Add:

```rust
#[test]
fn migrates_root_artifacts_to_legacy_import_once() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("analysis")).unwrap();
    std::fs::write(root.join("analysis/report.md"), b"report").unwrap();
    std::fs::create_dir_all(root.join("charts")).unwrap();
    std::fs::write(root.join("charts/chart.html"), b"chart").unwrap();

    let home = crate::storage::AiJiaHome::from_path(root.to_path_buf());
    let report = migrate_root_artifacts_once(&home, "20260601").unwrap();

    assert_eq!(report.moved_paths.len(), 2);
    assert!(!root.join("analysis").exists());
    assert!(!root.join("charts").exists());
    assert!(root
        .join("defaultFolder/legacy-root-import-20260601/analysis/report.md")
        .exists());
    assert!(root
        .join("defaultFolder/legacy-root-import-20260601/charts/chart.html")
        .exists());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd src-tauri && cargo test app_data_governance::tests::migrates_root_artifacts_to_legacy_import_once
```

Expected: FAIL because `migrate_root_artifacts_once` does not exist.

- [ ] **Step 3: Write minimal implementation**

Implement:

- `const ROOT_ARTIFACT_DIRS: &[&str] = &["analysis", "charts", "generated", "exports", "reports", "uploads"];`
- `migrate_root_artifacts_once(home, date_key)` moves each existing root artifact dir into `defaultFolder/legacy-root-import-{date_key}/`.
- If target exists, skip that source and record it as skipped to avoid overwriting.
- Write a `manifest.json` in the import dir.

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cd src-tauri && cargo test app_data_governance::tests::migrates_root_artifacts_to_legacy_import_once
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/storage/app_data_governance.rs
git commit -m "feat(storage): migrate legacy root artifacts to workspace"
```

## Task 5: Startup Governance Orchestration

**Files:**
- Modify: `src-tauri/src/storage/app_data_governance.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Add:

```rust
#[test]
fn startup_governance_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let home = crate::storage::AiJiaHome::from_path(tmp.path().to_path_buf());
    std::fs::create_dir_all(tmp.path().join("analysis")).unwrap();
    std::fs::write(tmp.path().join("analysis/report.md"), b"report").unwrap();

    let first = run_startup_governance_for_test(&home, "20260601").unwrap();
    let second = run_startup_governance_for_test(&home, "20260601").unwrap();

    assert_eq!(first.artifact_report.moved_paths.len(), 1);
    assert_eq!(second.artifact_report.moved_paths.len(), 0);
    assert!(tmp
        .path()
        .join("defaultFolder/legacy-root-import-20260601/analysis/report.md")
        .exists());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd src-tauri && cargo test app_data_governance::tests::startup_governance_is_idempotent
```

Expected: FAIL because orchestration does not exist.

- [ ] **Step 3: Write minimal implementation**

Implement `run_startup_governance(home: &AiJiaHome) -> GovernanceReport` and test variant with date injection. Then call it in `lib.rs::setup` after existing migration calls:

```rust
let governance = storage::app_data_governance::run_startup_governance(&aijia_home);
if !governance.warnings.is_empty() {
    log::warn!("[app-data-governance] warnings: {:?}", governance.warnings);
}
```

The function must catch per-step failures and return warnings instead of bubbling errors to startup.

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cd src-tauri && cargo test app_data_governance::tests::startup_governance_is_idempotent
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/storage/app_data_governance.rs src-tauri/src/lib.rs
git commit -m "feat(storage): run app data governance at startup"
```

## Task 6: Documentation And Final Verification

**Files:**
- Modify: `docs/superpowers/specs/2026-06-01-app-data-governance-design.md`
- Modify: Lotus 服务端仓库 `docs/desktop/storage-conventions.md`

- [ ] **Step 1: Update docs**

Update storage conventions with:

- Root artifact import path.
- Temporary TTL policy.
- Log bounding policy.
- Review-only items that remain technical debt.

- [ ] **Step 2: Run focused tests**

Run:

```bash
cd src-tauri && cargo test app_data_governance
```

Expected: PASS.

- [ ] **Step 3: Run startup-adjacent storage tests**

Run:

```bash
cd src-tauri && cargo test migration_root_cleanup migration_user_scope data_version --tests
```

Expected: PASS.

- [ ] **Step 4: Review git diff**

Run:

```bash
git diff --stat
git diff -- src-tauri/src/storage/app_data_governance.rs src-tauri/src/lib.rs
```

Expected: Diff only touches app data governance, startup hook, and docs.

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/specs/2026-06-01-app-data-governance-design.md
# In the Lotus server repo:
git add docs/desktop/storage-conventions.md
git commit -m "docs(storage): document app data governance"
```

## Self-Review

- Spec coverage: root layout review, old-version upgrade safety, low-risk automatic governance, and review-only sensitive items are covered by tasks.
- Placeholder scan: implementation details are concrete and no task relies on unspecified helper behavior.
- Type consistency: `RootEntryClass`, `CleanupReport`, `migrate_root_artifacts_once`, and `run_startup_governance` are introduced before later tasks reference them.
