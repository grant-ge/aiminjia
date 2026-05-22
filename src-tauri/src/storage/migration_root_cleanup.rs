//! Legacy-root cleanup pass (P0-1 of 2026-05-21 storage review).
//!
//! `migrate_legacy_to_user_scope_if_needed` (in `migration_user_scope.rs`)
//! copies legacy root-level data into `users/{scope}/` and records ownership
//! in `global/state.json::migrations.legacyRootClaim`, but **never deletes
//! the source**.  On every long-running install we therefore end up with
//! two copies of conversations, screenshots, etc — wasted disk + a hazard
//! for multi-account machines (the second user can't see the data, but it
//! still occupies the root they "see").
//!
//! This module finishes the job by **archiving** the now-redundant root
//! copies into `<root>/.archived-legacy-<ts>/`, then GC'ing that archive
//! 30 days later.  Two-step (archive, not rm) so a wrong call is recoverable
//! by hand.
//!
//! ## Archive eligibility — scoped on purpose
//!
//! Only the items listed in [`ARCHIVE_ITEMS`] are touched.  Each one is
//! either:
//!   - in `migration_user_scope::LEGACY_ITEMS` and confirmed claimed (the
//!     user-scoped copy is source of truth), or
//!   - a known-dead historical file (legacy config split, 0-byte db stub,
//!     orphaned LLM JSON spills).
//!
//! Anything **not** in the list — including `state.json`, `data_version`,
//! `.migrated`, `device_id`, `personas/`, `playwright-profile/`, workspace
//! artefacts (`analysis/`, `charts/`, ...) and the legacy `temp/` /
//! `tmpImage/` dirs — is left alone, either because the code still reads
//! the root path, or because it falls under a separate cleanup track.
//!
//! ## Guards
//!
//! 1. `global/state.json::migrations.legacyRootClaim.claimedBy` must be set
//!    — meaning `migrate_legacy_to_user_scope_if_needed` did finish copying.
//! 2. At least 24h must have elapsed since `claimedAt`.  Window lets the
//!    user start the app at least once with the new layout so the
//!    user-scoped copy gets read end-to-end before we hide the legacy.
//! 3. The function is idempotent: a second run with
//!    `migrations.legacyRootArchived` already set is a no-op.

use std::fs;
use std::path::Path;

use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde_json::{json, Value};

use super::migration::{read_state_json, update_state_json};

/// Items eligible for archive.  Each entry is a path relative to the
/// `~/.renlijia/` root.  Order doesn't matter — directories and files are
/// handled uniformly.  See module docs for the eligibility rationale.
const ARCHIVE_ITEMS: &[&str] = &[
    // — already claimed by user-scope migration, user copy is source of truth —
    "index.json",
    "index.json.bak",
    "conversations",
    "audit",
    "api-data",
    "subagent_transcripts",
    "shared",
    "site-profiles",
    "screenshots",
    // — known-dead historical files —
    "config.json",      // split into global/config.json + users/{scope}/config.json
    "conversations.db", // 0-byte sqlite stub never populated
    // — LLM artefact spills (workspacePath defaulted to ~/.renlijia so tool
    //   writes landed here; quarantine the named ones we can identify) —
    "comp_fairness_slides.json",
    "hr_analysis_report.json",
    "report_sections.json",
    "slides.json",
];

/// How long after `legacyRootClaim.claimedAt` we wait before archiving the
/// root copy.  Gives the user at least one app launch on the new layout so
/// the user-scoped copy is read end-to-end.
const ARCHIVE_GRACE: Duration = Duration::hours(24);

/// How long the archive sits on disk before being permanently deleted.
const ARCHIVE_RETENTION: Duration = Duration::days(30);

/// Archive the legacy root copies of user data, once.
///
/// Returns `Ok(true)` if an archive was created this call, `Ok(false)` if
/// nothing was eligible (no claim yet, grace not elapsed, already archived,
/// or no surviving root files).  Errors are propagated only for unexpected
/// I/O issues — every per-item failure is logged + skipped.
pub fn cleanup_legacy_root_if_claimed(
    root: &Path,
    global_state_path: &Path,
) -> std::io::Result<bool> {
    let state = read_state_json(global_state_path)?;
    let migrations = state.get("migrations");

    if migrations
        .and_then(|m| m.get("legacyRootArchived"))
        .is_some()
    {
        // Already done.  Don't re-archive even if a stale root copy was
        // re-created since (it would land in a brand-new <ts> dir and
        // confuse the next GC pass).
        return Ok(false);
    }

    let claim = match migrations.and_then(|m| m.get("legacyRootClaim")) {
        Some(c) => c,
        None => return Ok(false),
    };
    let claimed_by = claim
        .get("claimedBy")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let claimed_at = claim
        .get("claimedAt")
        .and_then(Value::as_str)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));
    let Some(claimed_at) = claimed_at else {
        log::warn!(
            "[root-cleanup] legacyRootClaim missing/unparseable claimedAt — skip"
        );
        return Ok(false);
    };
    let now = Utc::now();
    if now - claimed_at < ARCHIVE_GRACE {
        log::info!(
            "[root-cleanup] within {}h grace of claim at {} — skip",
            ARCHIVE_GRACE.num_hours(),
            claimed_at.to_rfc3339()
        );
        return Ok(false);
    }

    // Build the archive dir name from the *claim* timestamp (not `now`) so
    // a third user logging in years later doesn't create a fresh dir for
    // the same legacy bundle.  Use a filesystem-safe form of the timestamp.
    let archive_name = format!(
        ".archived-legacy-{}",
        claimed_at.format("%Y%m%dT%H%M%SZ"),
    );
    let archive_dir = root.join(&archive_name);

    let mut moved_paths: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    for rel in ARCHIVE_ITEMS {
        let src = root.join(rel);
        if !src.exists() {
            skipped.push(format!("{rel} (absent)"));
            continue;
        }
        // Create archive dir lazily on first hit so we don't litter when
        // nothing is eligible.
        if !archive_dir.exists() {
            fs::create_dir_all(&archive_dir)?;
        }
        let dst = archive_dir.join(rel);
        if let Some(parent) = dst.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        match fs::rename(&src, &dst) {
            Ok(_) => moved_paths.push((*rel).to_string()),
            Err(e) if is_cross_device_error(&e) => {
                // Cross-device — fall back to copy + remove.
                if let Err(e2) = copy_then_remove(&src, &dst) {
                    log::warn!(
                        "[root-cleanup] EXDEV copy fallback failed for {rel}: {e2} (rename err: {e})"
                    );
                    skipped.push(format!("{rel} (copy fallback failed)"));
                } else {
                    moved_paths.push((*rel).to_string());
                }
            }
            Err(e) => {
                log::warn!("[root-cleanup] rename failed for {rel}: {e}");
                skipped.push(format!("{rel} (rename failed)"));
            }
        }
    }

    if moved_paths.is_empty() {
        log::info!(
            "[root-cleanup] claim={claimed_by}, claimedAt={}, nothing to archive (root already clean)",
            claimed_at.to_rfc3339()
        );
        // Even with nothing moved, record the marker so we don't recheck
        // every startup forever.
    } else {
        log::info!(
            "[root-cleanup] archived {} item(s) -> {}: {}",
            moved_paths.len(),
            archive_dir.display(),
            moved_paths.join(", ")
        );
        if !skipped.is_empty() {
            log::info!("[root-cleanup] skipped: {}", skipped.join(", "));
        }
    }

    update_state_json(global_state_path, |state| {
        state["migrations"]["legacyRootArchived"] = json!({
            "archivedAt": now.to_rfc3339(),
            "archiveDir": archive_name,
            "movedItems": moved_paths,
        });
    })?;

    Ok(!moved_paths.is_empty())
}

/// GC: permanently delete any `.archived-legacy-*` directories whose age
/// exceeds `ARCHIVE_RETENTION` (30 days).  Safe to call every startup —
/// missing dirs or unparseable timestamps are skipped.
pub fn cleanup_legacy_archive_if_expired(root: &Path) -> std::io::Result<usize> {
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    let now = Utc::now();
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        let Some(ts_str) = name_str.strip_prefix(".archived-legacy-") else {
            continue;
        };
        let parsed = NaiveDateTime::parse_from_str(ts_str, "%Y%m%dT%H%M%SZ")
            .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
            .ok();
        let Some(archived_at) = parsed else {
            log::warn!(
                "[root-cleanup] unparseable archive dir name {name_str}, skip"
            );
            continue;
        };
        if now - archived_at < ARCHIVE_RETENTION {
            continue;
        }
        let path = entry.path();
        log::info!(
            "[root-cleanup] deleting expired archive {} (age={}d)",
            path.display(),
            (now - archived_at).num_days(),
        );
        if let Err(e) = fs::remove_dir_all(&path) {
            log::warn!("[root-cleanup] failed to remove {}: {e}", path.display());
            continue;
        }
        removed += 1;
    }
    Ok(removed)
}

/// Returns true when an std::io::Error indicates a cross-filesystem rename
/// attempt (the canonical case is moving from `~/.renlijia` to a temp dir on
/// a different mount, which `fs::rename` refuses on every platform).
///
/// - Unix: `EXDEV` (errno 18). Pulled from `libc` to avoid hardcoding.
/// - Windows: `ERROR_NOT_SAME_DEVICE` (Win32 0x11 = decimal 17). Hardcoded
///   here to avoid pulling `windows-sys::Win32::Foundation` solely for this
///   constant.
///
/// Falls back to `ErrorKind::CrossesDevices` (stable since Rust 1.85) for
/// any platform that maps the OS error there but doesn't match a known
/// raw_os_error.
fn is_cross_device_error(e: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        if e.raw_os_error() == Some(libc::EXDEV) {
            return true;
        }
    }
    #[cfg(windows)]
    {
        // ERROR_NOT_SAME_DEVICE = 0x11 = 17
        if e.raw_os_error() == Some(17) {
            return true;
        }
    }
    matches!(e.kind(), std::io::ErrorKind::CrossesDevices)
}

fn copy_then_remove(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        copy_dir_recursive(src, dst)?;
        fs::remove_dir_all(src)?;
    } else {
        fs::copy(src, dst)?;
        fs::remove_file(src)?;
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_state_with_claim(state_path: &Path, scope: &str, claimed_at: DateTime<Utc>) {
        let v = json!({
            "migrations": {
                "legacyRootClaim": {
                    "claimedBy": scope,
                    "claimedAt": claimed_at.to_rfc3339(),
                }
            }
        });
        if let Some(p) = state_path.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(state_path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    }

    fn read_state(state_path: &Path) -> Value {
        let s = fs::read_to_string(state_path).unwrap();
        serde_json::from_str(&s).unwrap()
    }

    #[test]
    fn no_claim_means_no_archive() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("index.json"), b"x").unwrap();
        let state = root.join("global/state.json");
        fs::create_dir_all(state.parent().unwrap()).unwrap();
        fs::write(&state, "{}").unwrap();

        let moved = cleanup_legacy_root_if_claimed(root, &state).unwrap();
        assert!(!moved);
        // index.json still present.
        assert!(root.join("index.json").exists());
    }

    #[test]
    fn within_24h_grace_skipped() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("index.json"), b"x").unwrap();
        let state = root.join("global/state.json");
        write_state_with_claim(&state, "t_1__u_2", Utc::now() - Duration::hours(1));

        let moved = cleanup_legacy_root_if_claimed(root, &state).unwrap();
        assert!(!moved);
        assert!(root.join("index.json").exists());
    }

    #[test]
    fn after_grace_archives_known_items_only() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // Archive-eligible items
        fs::write(root.join("index.json"), b"i").unwrap();
        fs::create_dir_all(root.join("conversations/foo")).unwrap();
        fs::write(root.join("conversations/foo/conv.json"), b"c").unwrap();
        fs::write(root.join("conversations.db"), b"").unwrap();
        fs::write(root.join("slides.json"), b"s").unwrap();
        // NOT in the archive list — must survive.
        fs::write(root.join("data_version"), b"1").unwrap();
        fs::write(root.join("device_id"), b"dev").unwrap();
        fs::create_dir_all(root.join("playwright-profile")).unwrap();
        fs::write(root.join("playwright-profile/cookies"), b"c").unwrap();

        let claimed_at = Utc::now() - Duration::days(2);
        let state = root.join("global/state.json");
        write_state_with_claim(&state, "t_1__u_2", claimed_at);

        let moved = cleanup_legacy_root_if_claimed(root, &state).unwrap();
        assert!(moved);

        let archive_dir = root.join(format!(
            ".archived-legacy-{}",
            claimed_at.format("%Y%m%dT%H%M%SZ")
        ));
        assert!(archive_dir.exists());
        assert!(archive_dir.join("index.json").exists());
        assert!(archive_dir.join("conversations/foo/conv.json").exists());
        assert!(archive_dir.join("conversations.db").exists());
        assert!(archive_dir.join("slides.json").exists());

        // Originals are gone.
        assert!(!root.join("index.json").exists());
        assert!(!root.join("conversations").exists());
        assert!(!root.join("conversations.db").exists());
        assert!(!root.join("slides.json").exists());

        // Out-of-scope files untouched.
        assert!(root.join("data_version").exists());
        assert!(root.join("device_id").exists());
        assert!(root.join("playwright-profile/cookies").exists());

        // State.json records the archive.
        let st = read_state(&state);
        let arch = st
            .get("migrations")
            .and_then(|m| m.get("legacyRootArchived"))
            .unwrap();
        assert!(arch.get("archivedAt").is_some());
        assert!(arch.get("archiveDir").is_some());
    }

    #[test]
    fn idempotent_second_call_skipped() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("index.json"), b"x").unwrap();
        let claimed_at = Utc::now() - Duration::days(2);
        let state = root.join("global/state.json");
        write_state_with_claim(&state, "t_1__u_2", claimed_at);

        cleanup_legacy_root_if_claimed(root, &state).unwrap();
        // Resurrect a root file to be sure the second call leaves it alone.
        fs::write(root.join("index.json"), b"new-resurrected").unwrap();
        let moved = cleanup_legacy_root_if_claimed(root, &state).unwrap();
        assert!(!moved);
        assert!(root.join("index.json").exists());
        let content = fs::read(root.join("index.json")).unwrap();
        assert_eq!(content, b"new-resurrected");
    }

    #[test]
    fn nothing_to_archive_still_records_marker() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // Root is clean — only state.json exists.
        let claimed_at = Utc::now() - Duration::days(2);
        let state = root.join("global/state.json");
        write_state_with_claim(&state, "t_1__u_2", claimed_at);

        let moved = cleanup_legacy_root_if_claimed(root, &state).unwrap();
        assert!(!moved); // moved=false because nothing was eligible
        let st = read_state(&state);
        assert!(st
            .get("migrations")
            .and_then(|m| m.get("legacyRootArchived"))
            .is_some());
    }

    #[test]
    fn archive_expired_removed_only_after_30d() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Fresh archive: 5 days old — must stay.
        let fresh_name = format!(
            ".archived-legacy-{}",
            (Utc::now() - Duration::days(5)).format("%Y%m%dT%H%M%SZ")
        );
        fs::create_dir_all(root.join(&fresh_name)).unwrap();
        fs::write(root.join(&fresh_name).join("a"), b"a").unwrap();

        // Expired archive: 40 days old — must be removed.
        let expired_name = format!(
            ".archived-legacy-{}",
            (Utc::now() - Duration::days(40)).format("%Y%m%dT%H%M%SZ")
        );
        fs::create_dir_all(root.join(&expired_name)).unwrap();
        fs::write(root.join(&expired_name).join("a"), b"a").unwrap();

        // Garbage dir with same prefix but unparseable ts — must stay.
        fs::create_dir_all(root.join(".archived-legacy-not-a-timestamp")).unwrap();

        let removed = cleanup_legacy_archive_if_expired(root).unwrap();
        assert_eq!(removed, 1);
        assert!(root.join(&fresh_name).exists());
        assert!(!root.join(&expired_name).exists());
        assert!(root.join(".archived-legacy-not-a-timestamp").exists());
    }
}
