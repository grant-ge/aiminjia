use std::collections::HashSet;
use std::path::PathBuf;

/// Derive working directories from user-provided attachment paths.
///
/// Each path's parent directory (or the path itself if it's a directory) is
/// added to the result. Duplicates by canonical path are removed.
///
/// Note on filtering: this function used to skip "forbidden" directories
/// (~/.ssh, ~/.aws, /etc, etc.) but that contradicted the path-auth model.
/// User-attached paths are treated as user-authorized — if they truly want
/// to attach ~/.ssh as workspace context, that's their explicit choice.
/// Downstream path-auth (decide::is_path_allowed) still runs on every tool
/// call and will Ask before any sensitive read/write completes.
pub fn derive_working_dirs_from_attachments(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for p in paths {
        let canonical = match std::fs::canonicalize(p) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let dir = if canonical.is_dir() {
            canonical
        } else {
            match canonical.parent() {
                Some(parent) => parent.to_path_buf(),
                None => continue,
            }
        };
        seen.insert(dir);
    }
    seen.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn file_attachment_yields_parent_dir() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("data.csv");
        std::fs::write(&file, b"").unwrap();
        let result = derive_working_dirs_from_attachments(&[file]);
        let canonical_tmp = std::fs::canonicalize(tmp.path()).unwrap();
        assert!(result.contains(&canonical_tmp));
    }

    #[test]
    fn directory_attachment_yields_self() {
        let tmp = TempDir::new().unwrap();
        let canonical_tmp = std::fs::canonicalize(tmp.path()).unwrap();
        let result = derive_working_dirs_from_attachments(&[tmp.path().to_path_buf()]);
        assert!(result.contains(&canonical_tmp));
    }

    #[test]
    fn dedup_same_canonical() {
        let tmp = TempDir::new().unwrap();
        let file1 = tmp.path().join("a.txt");
        let file2 = tmp.path().join("b.txt");
        std::fs::write(&file1, b"").unwrap();
        std::fs::write(&file2, b"").unwrap();
        let result = derive_working_dirs_from_attachments(&[file1, file2]);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn skip_canonicalize_failure() {
        let nonexistent = PathBuf::from("/this/path/does/not/exist/at/all/xyz");
        let result = derive_working_dirs_from_attachments(&[nonexistent]);
        assert!(result.is_empty());
    }

    // Removed: skip_forbidden_home_self / skip_forbidden_dotssh / skip_forbidden_renlijia_root.
    // These tests asserted the old hardcoded-blacklist behavior; under the new
    // "no absolute denials" policy attachments are passed through unchanged and
    // sensitive paths are gated downstream by path-auth Ask flow.
}
