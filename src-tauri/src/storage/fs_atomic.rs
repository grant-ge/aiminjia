//! Cross-platform filesystem atomicity helpers.
//!
//! Centralizes the two write/delete patterns that Windows tooling (杀软扫描 /
//! Explorer 缩略图 / Indexer) breaks if you use the naive APIs:
//!
//! 1. **Atomic file write** — write to `<path>.tmp` then `rename`. A crash
//!    between the two leaves the original file untouched (or `.tmp` orphan
//!    that the next write overwrites). Without this, `fs::write` can leave
//!    a half-written file that breaks `serde_json::from_slice` on next
//!    startup.
//! 2. **Retry-loop directory removal** — `fs::remove_dir_all` with 3 ×
//!    150–300 ms backoff. Antivirus or Explorer may briefly hold handles
//!    to files we want to delete; a single retry after a short pause clears
//!    almost all those cases.
//!
//! These were originally private helpers in `runtime::employee::store`. Hoisted
//! here so skill_smith / skill_package / draft commands can reuse — see CLAUDE.md
//! decision 41 (\"任何写到磁盘的状态文件优先 tmp + rename 原子写；目录删除走
//! `remove_dir_all_retry` 3×150–300ms backoff\").

use std::fs;
use std::io;
use std::path::Path;

use anyhow::{Context, Result};

/// Write `bytes` to `path` atomically by going through a sibling `.tmp` file.
///
/// The `.tmp` file is created next to `path` (same parent, same volume) so the
/// final `rename` is a single inode flip, not a cross-device copy.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create parent: {}", parent.display()))?;
        }
    }
    let tmp = tmp_sibling(path);
    fs::write(&tmp, bytes).with_context(|| format!("write tmp: {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("rename tmp → {}", path.display()))?;
    Ok(())
}

/// `fs::remove_dir_all` with Windows-friendly retry. 3 attempts × 150–300 ms
/// backoff. Idempotent: NotFound is treated as success.
pub fn remove_dir_all_retry(dir: &Path) -> Result<()> {
    let mut last_err: Option<io::Error> = None;
    for attempt in 0..3 {
        match fs::remove_dir_all(dir) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                if attempt < 2 {
                    std::thread::sleep(std::time::Duration::from_millis(150 * (attempt + 1)));
                }
            }
        }
    }
    Err(last_err.unwrap().into())
}

/// Move a directory atomically: prefer `rename`, fall back to copy + remove
/// when source/dest are on different volumes (common on Windows when tmp
/// is on `C:` and target is on `D:`). After a successful copy the source
/// is removed via `remove_dir_all_retry`.
pub fn move_dir_atomic(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create parent: {}", parent.display()))?;
    }
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Cross-device or locked — copy then remove.
            copy_dir_recursive(src, dst)
                .with_context(|| format!("copy {} → {}", src.display(), dst.display()))?;
            remove_dir_all_retry(src)?;
            Ok(())
        }
    }
}

/// Replace `target` directory with `staged` directory **safely**:
///   1. If `target` exists, rename it to `target.bak.<rand>`
///   2. Move `staged` → `target`
///   3. Remove `target.bak.<rand>` (best effort)
///
/// If step 2 fails after step 1, we restore the backup. This avoids the
/// classic "remove target → copy new → CRASH → user loses everything" hole.
pub fn replace_dir_atomic(staged: &Path, target: &Path) -> Result<()> {
    let backup = if target.exists() {
        let bak = backup_sibling(target);
        fs::rename(target, &bak)
            .with_context(|| format!("backup {} → {}", target.display(), bak.display()))?;
        Some(bak)
    } else {
        None
    };

    if let Err(e) = move_dir_atomic(staged, target) {
        // restore backup
        if let Some(bak) = backup.as_ref() {
            let _ = fs::rename(bak, target);
        }
        return Err(e);
    }

    if let Some(bak) = backup {
        // best effort — orphan backup is harmless and gets GC'd by next run
        let _ = remove_dir_all_retry(&bak);
    }
    Ok(())
}

fn tmp_sibling(path: &Path) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".tmp");
    s.into()
}

fn backup_sibling(path: &Path) -> std::path::PathBuf {
    let suffix = format!(".bak.{}", uuid::Uuid::new_v4().simple());
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    s.into()
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
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

    #[test]
    fn write_atomic_creates_file_and_no_tmp_left() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a").join("foo.json");
        write_atomic(&path, b"{}").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{}");
        // .tmp sibling should be gone
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn write_atomic_overwrites_atomically() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("foo.json");
        write_atomic(&path, b"v1").unwrap();
        write_atomic(&path, b"v2").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"v2");
    }

    #[test]
    fn remove_dir_all_retry_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("gone");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("x"), b"hi").unwrap();
        remove_dir_all_retry(&dir).unwrap();
        // Second call on missing dir → still Ok
        remove_dir_all_retry(&dir).unwrap();
    }

    #[test]
    fn move_dir_atomic_works_same_volume() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(src.join("nested")).unwrap();
        fs::write(src.join("nested/a.txt"), b"x").unwrap();
        let dst = tmp.path().join("dst");
        move_dir_atomic(&src, &dst).unwrap();
        assert!(!src.exists());
        assert_eq!(fs::read(dst.join("nested/a.txt")).unwrap(), b"x");
    }

    #[test]
    fn replace_dir_atomic_restores_target_when_staged_missing() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("target");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("keep"), b"original").unwrap();

        // staged 不存在 → 应该报错，target 被还原
        let staged = tmp.path().join("never-existed");
        let res = replace_dir_atomic(&staged, &target);
        assert!(res.is_err());
        // target 仍是原内容
        assert!(target.is_dir());
        assert_eq!(fs::read(target.join("keep")).unwrap(), b"original");
    }

    #[test]
    fn replace_dir_atomic_swaps_when_staged_exists() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("target");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("old"), b"old").unwrap();
        let staged = tmp.path().join("staged");
        fs::create_dir_all(&staged).unwrap();
        fs::write(staged.join("new"), b"new").unwrap();

        replace_dir_atomic(&staged, &target).unwrap();
        assert!(!target.join("old").exists());
        assert_eq!(fs::read(target.join("new")).unwrap(), b"new");
        assert!(!staged.exists());
    }

    #[test]
    fn replace_dir_atomic_works_when_target_missing() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("fresh");
        let staged = tmp.path().join("staged");
        fs::create_dir_all(&staged).unwrap();
        fs::write(staged.join("x"), b"new").unwrap();

        replace_dir_atomic(&staged, &target).unwrap();
        assert_eq!(fs::read(target.join("x")).unwrap(), b"new");
    }
}
