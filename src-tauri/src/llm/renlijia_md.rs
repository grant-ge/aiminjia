use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenlijiaMdFile {
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Default)]
pub struct RenlijiaMdLoader {
    cache: HashMap<PathBuf, (SystemTime, String)>,
}

impl RenlijiaMdLoader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load(&mut self, workspace_path: &Path) -> Vec<RenlijiaMdFile> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();

        if let Some(home) = Self::home_dir() {
            self.try_add_file(
                &home.join(".renlijia").join("RENLIJIA.md"),
                &mut seen,
                &mut result,
            );
        }

        if workspace_path.as_os_str().is_empty() {
            return result;
        }

        let mut dirs = Vec::new();
        let mut current = Some(workspace_path);
        while let Some(dir) = current {
            dirs.push(dir.to_path_buf());
            current = dir.parent();
        }
        dirs.reverse();

        for dir in dirs {
            self.try_add_file(&dir.join("RENLIJIA.md"), &mut seen, &mut result);
            self.try_add_file(&dir.join(".aijia").join("RENLIJIA.md"), &mut seen, &mut result);
            self.try_add_file(&dir.join("RENLIJIA.local.md"), &mut seen, &mut result);
        }

        result
    }

    fn try_add_file(
        &mut self,
        path: &Path,
        seen: &mut HashSet<PathBuf>,
        result: &mut Vec<RenlijiaMdFile>,
    ) {
        let dedupe_key = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !seen.insert(dedupe_key) {
            return;
        }
        if let Some(content) = self.read_with_cache(path) {
            result.push(RenlijiaMdFile {
                path: path.to_path_buf(),
                content,
            });
        }
    }

    fn read_with_cache(&mut self, path: &Path) -> Option<String> {
        let metadata = fs::metadata(path).ok()?;
        let mtime = metadata.modified().ok()?;
        let cache_key = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if let Some((cached_mtime, cached_content)) = self.cache.get(&cache_key) {
            if *cached_mtime == mtime {
                return Some(cached_content.clone());
            }
        }
        let content = fs::read_to_string(path).ok()?;
        self.cache.insert(cache_key, (mtime, content.clone()));
        Some(content)
    }

    fn home_dir() -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

pub fn build_renlijia_md_context_message(files: &[RenlijiaMdFile]) -> Option<String> {
    if files.is_empty() {
        return None;
    }

    let mut out = String::from("\n\n# renlijiaMd\n");
    out.push_str("以下是从 RENLIJIA.md / .aijia/RENLIJIA.md / RENLIJIA.local.md 加载的用户上下文，请遵循其中与当前任务相关的约束。\n");
    for file in files {
        let content = file.content.trim();
        if content.is_empty() {
            continue;
        }
        out.push_str("\n## ");
        out.push_str(&file.path.to_string_lossy());
        out.push_str("\n");
        out.push_str(content);
        out.push('\n');
    }

    if out.trim() == "# renlijiaMd" {
        None
    } else {
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_loads_renlijia_files_in_order() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("repo");
        let child = root.join("a/b");
        std::fs::create_dir_all(child.join(".aijia")).unwrap();
        std::fs::write(root.join("RENLIJIA.md"), "root").unwrap();
        std::fs::write(child.join(".aijia/RENLIJIA.md"), "project").unwrap();
        std::fs::write(child.join("RENLIJIA.local.md"), "local").unwrap();

        let mut loader = RenlijiaMdLoader::new();
        let files = loader.load(&child);
        let contents: Vec<_> = files.iter().map(|f| f.content.as_str()).collect();

        assert!(contents.ends_with(&["root", "project", "local"]));
    }

    #[test]
    fn test_does_not_load_legacy_claude_paths() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("repo");
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(root.join("CLAUDE.md"), "legacy root").unwrap();
        std::fs::write(root.join(".claude/CLAUDE.md"), "legacy project").unwrap();

        let mut loader = RenlijiaMdLoader::new();
        let files = loader.load(&root);

        assert!(files.iter().all(|f| !f.content.contains("legacy")));
    }

    #[test]
    fn test_build_context_uses_renlijia_tag() {
        let files = vec![RenlijiaMdFile {
            path: PathBuf::from("/tmp/RENLIJIA.md"),
            content: "hello".to_string(),
        }];

        let context = build_renlijia_md_context_message(&files).unwrap();

        assert!(context.contains("# renlijiaMd"));
        assert!(context.contains("RENLIJIA.md"));
        assert!(!context.contains("# claudeMd"));
    }
}
