use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::runtime::path_auth::forbidden::is_forbidden_dir;

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
        if is_forbidden_dir(&dir) {
            continue;
        }
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

    #[test]
    fn skip_forbidden_home_self() {
        if let Some(home) = dirs::home_dir() {
            let result = derive_working_dirs_from_attachments(&[home]);
            assert!(result.is_empty());
        }
    }

    #[test]
    fn skip_forbidden_dotssh() {
        let Some(home) = dirs::home_dir() else { return; };
        let dotssh = home.join(".ssh");
        if !dotssh.exists() {
            // Cannot exercise the integration path on this machine because
            // canonicalize would fail and the path would be skipped before
            // is_forbidden_dir is even called. The unit-level guarantee that
            // is_forbidden_dir(~/.ssh) returns true is covered in forbidden.rs tests.
            return;
        }
        let result = derive_working_dirs_from_attachments(&[dotssh.clone()]);
        assert!(
            !result.iter().any(|p| p == &dotssh || p.ends_with(".ssh")),
            "~/.ssh should be filtered out, got: {:?}",
            result
        );
    }

    #[test]
    fn skip_forbidden_renlijia_root() {
        let Some(home) = dirs::home_dir() else { return; };
        let renlijia = home.join(".renlijia");
        if !renlijia.exists() {
            // Cannot exercise the integration path on this machine because
            // canonicalize would fail and the path would be skipped before
            // is_forbidden_dir is even called. The unit-level guarantee that
            // is_forbidden_dir(~/.renlijia) returns true is covered in forbidden.rs tests.
            return;
        }
        let result = derive_working_dirs_from_attachments(&[renlijia.clone()]);
        assert!(
            !result.iter().any(|p| p == &renlijia || p.ends_with(".renlijia")),
            "~/.renlijia should be filtered out, got: {:?}",
            result
        );
    }
}
