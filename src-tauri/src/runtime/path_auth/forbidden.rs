use std::path::{Path, PathBuf};

pub fn is_forbidden_dir(path: &Path) -> bool {
    let home = dirs::home_dir();

    let candidates: Vec<PathBuf> = {
        let mut v = vec![
            PathBuf::from("/"),
            PathBuf::from("/System"),
            PathBuf::from("/private"),
            PathBuf::from("/var"),
            PathBuf::from("/etc"),
            PathBuf::from("/usr"),
            PathBuf::from("/bin"),
            PathBuf::from("/sbin"),
            PathBuf::from("/Library"),
        ];
        if let Some(ref h) = home {
            v.push(h.clone());
            v.push(h.join("Library"));
            v.push(h.join(".ssh"));
            v.push(h.join(".aws"));
            v.push(h.join(".gnupg"));
            v.push(h.join(".config"));
            v.push(h.join(".kube"));
            v.push(h.join(".renlijia"));
        }
        if cfg!(windows) {
            v.push(PathBuf::from("C:\\"));
            v.push(PathBuf::from("C:\\Windows"));
            v.push(PathBuf::from("C:\\Program Files"));
            v.push(PathBuf::from("C:\\Program Files (x86)"));
        }
        v
    };

    let canonical_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    for candidate in &candidates {
        let canonical_candidate =
            std::fs::canonicalize(candidate).unwrap_or_else(|_| candidate.clone());
        if canonical_path == canonical_candidate {
            return true;
        }
    }
    false
}

pub fn is_lotus_internal(path: &Path, primary_root: Option<&Path>) -> bool {
    let renlijia_root = match dirs::home_dir() {
        Some(h) => h.join(".renlijia"),
        None => return false,
    };
    if !path.starts_with(&renlijia_root) {
        return false;
    }
    if let Some(root) = primary_root {
        // Why: §5.1 — when authorized_workspace itself lives inside ~/.renlijia/
        // (e.g. ~/.renlijia/defaultFolder/), paths under it must still be allowed.
        if path.starts_with(root) && root.starts_with(&renlijia_root) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_forbidden_dir_matches_home() {
        if let Some(home) = dirs::home_dir() {
            assert!(is_forbidden_dir(&home));
        }
    }

    #[test]
    fn is_forbidden_dir_matches_dotssh() {
        if let Some(home) = dirs::home_dir() {
            assert!(is_forbidden_dir(&home.join(".ssh")));
        }
    }

    #[test]
    fn is_forbidden_dir_matches_renlijia_root() {
        if let Some(home) = dirs::home_dir() {
            assert!(is_forbidden_dir(&home.join(".renlijia")));
        }
    }

    #[test]
    fn is_forbidden_dir_matches_root_slash() {
        #[cfg(unix)]
        assert!(is_forbidden_dir(Path::new("/")));
    }

    #[test]
    fn is_forbidden_dir_does_not_match_documents_subdir() {
        if let Some(home) = dirs::home_dir() {
            let docs = home.join("Documents");
            assert!(!is_forbidden_dir(&docs));
        }
    }

    #[test]
    fn is_lotus_internal_blocks_path_under_renlijia() {
        if let Some(home) = dirs::home_dir() {
            let path = home.join(".renlijia").join("conversations").join("abc");
            assert!(is_lotus_internal(&path, None));
        }
    }

    #[test]
    fn is_lotus_internal_allows_when_primary_inside_renlijia() {
        if let Some(home) = dirs::home_dir() {
            let renlijia = home.join(".renlijia");
            let workspace = renlijia.join("defaultFolder");
            let path = workspace.join("report.pdf");
            assert!(!is_lotus_internal(&path, Some(&workspace)));
        }
    }
}
