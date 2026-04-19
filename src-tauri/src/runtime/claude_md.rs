use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeMdFile {
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Default)]
pub struct ClaudeMdLoader {
    cache: HashMap<PathBuf, (SystemTime, String)>,
}

impl ClaudeMdLoader {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn load(&mut self, workspace_path: &Path) -> Vec<ClaudeMdFile> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();

        if let Some(home) = Self::home_dir() {
            self.try_add_file(&home.join(".claude").join("CLAUDE.md"), &mut seen, &mut result);
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
            self.try_add_file(&dir.join("CLAUDE.md"), &mut seen, &mut result);
            self.try_add_file(
                &dir.join(".claude").join("CLAUDE.md"),
                &mut seen,
                &mut result,
            );
            self.try_add_file(&dir.join("CLAUDE.local.md"), &mut seen, &mut result);
        }

        result
    }

    fn try_add_file(
        &mut self,
        path: &Path,
        seen: &mut HashSet<PathBuf>,
        result: &mut Vec<ClaudeMdFile>,
    ) {
        let dedupe_key = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !seen.insert(dedupe_key) {
            return;
        }

        if let Some(content) = self.read_with_cache(path) {
            result.push(ClaudeMdFile {
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
