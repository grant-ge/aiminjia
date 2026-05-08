//! 项目指令文件加载器。
//!
//! 支持的文件名（仅这些）：
//! - `~/.renlijia/AGENTS.md`（用户全局）
//! - 工作目录及其父目录的 `AGENTS.md` / `.aijia/AGENTS.md` / `AGENTS.local.md`
//!
//! 不兼容：CLAUDE.md / .claude/CLAUDE.md / .claude/rules/*.md。
//! 这是 2026-05-08 的有意决定（用户决策），减少多源指令的冲突面与维护成本。
//!
//! 多文件按追加方式合并（不是覆盖），就近层级优先（工作目录覆盖用户全局）。
//! 消费侧（chat_turn_driver::build_agents_md_context_message）会把每段
//! 内容包成 `# agentsMd` user message 注入到主对话上下文。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentsMdFile {
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Default)]
pub struct AgentsMdLoader {
    cache: HashMap<PathBuf, (SystemTime, String)>,
}

impl AgentsMdLoader {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn load(&mut self, workspace_path: &Path) -> Vec<AgentsMdFile> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();

        if let Some(home) = Self::home_dir() {
            self.try_add_file(
                &home.join(".renlijia").join("AGENTS.md"),
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
            self.try_add_file(&dir.join("AGENTS.md"), &mut seen, &mut result);
            self.try_add_file(&dir.join(".aijia").join("AGENTS.md"), &mut seen, &mut result);
            self.try_add_file(&dir.join("AGENTS.local.md"), &mut seen, &mut result);
        }

        result
    }

    fn try_add_file(
        &mut self,
        path: &Path,
        seen: &mut HashSet<PathBuf>,
        result: &mut Vec<AgentsMdFile>,
    ) {
        let dedupe_key = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !seen.insert(dedupe_key) {
            return;
        }

        if let Some(content) = self.read_with_cache(path) {
            result.push(AgentsMdFile {
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
