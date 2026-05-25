//! 项目指令文件加载器。
//!
//! 只读取 **一个** 文件：`{authorized_workspace}/AGENTS.md`。
//!
//! ## 完全废弃的旧路径（不再加载，不能恢复）
//! - `~/.renlijia/AGENTS.md`（用户全局）
//! - `.aijia/AGENTS.md`（子目录变体）
//! - `AGENTS.local.md`（local 变体）
//! - 父目录链遍历（workspace → / 逐级向上）
//!
//! ## 设计决策（2026-05-09）
//! 旧设计的 workspace_path 实际收到的是产物根目录（`~/.renlijia`），
//! 导致用户在 `~/Documents/项目/AGENTS.md` 放的指令文件从未被加载。
//! 新设计直接使用 `authorized_workspace.root_path`，精准对应用户授权目录。
//!
//! 不兼容：CLAUDE.md / .claude/CLAUDE.md / .claude/rules/*.md。
//! 消费侧（chat_turn_driver::build_agents_md_context_message）会把内容
//! 包成 `# agentsMd` user message 注入到主对话上下文。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

/// 已加载的 AGENTS.md 文件（path + 内容）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentsMdFile {
    pub path: PathBuf,
    pub content: String,
}

/// 带 mtime 缓存的加载器。
///
/// 每个 `AgentsMdLoader` 实例维护一个 `(canonical_path → (mtime, content))` 缓存；
/// 只要文件的 mtime 未变，就不重新读磁盘。
#[derive(Debug, Default)]
pub struct AgentsMdLoader {
    cache: HashMap<PathBuf, (SystemTime, String)>,
}

/// 文件大小上限：64 KiB。超出部分截断并打 WARN 日志。
const MAX_BYTES: usize = 65536;

impl AgentsMdLoader {
    pub fn new() -> Self {
        Self::default()
    }

    /// 加载 `{authorized_workspace}/AGENTS.md`。
    ///
    /// - `None` → 立即返回空 Vec，打 INFO 日志。
    /// - `Some(ws)` → 检查 `{ws.root_path}/AGENTS.md`；不存在则返回空 Vec。
    /// - 文件大小 > 64 KiB → 截断到 64 KiB 并打 WARN 日志。
    /// - 返回长度 ∈ {0, 1}。
    pub async fn load(
        &mut self,
        authorized_workspace: Option<&crate::runtime::store::AuthorizedWorkspaceRef>,
    ) -> Vec<AgentsMdFile> {
        let ws = match authorized_workspace {
            None => {
                log::info!("agents_md skipped reason=no-authorized-workspace");
                return Vec::new();
            }
            Some(w) => w,
        };

        let agents_md_path = ws.root_path.join("AGENTS.md");
        let content = match self.read_with_cache(&agents_md_path) {
            None => {
                log::info!("agents_md absent path={}", agents_md_path.display());
                return Vec::new();
            }
            Some(c) => c,
        };

        log::info!(
            "agents_md loaded path={} bytes={}",
            agents_md_path.display(),
            content.len()
        );

        vec![AgentsMdFile {
            path: agents_md_path,
            content,
        }]
    }

    /// 按 mtime 缓存读取文件内容；超过 64 KiB 则截断。
    fn read_with_cache(&mut self, path: &std::path::Path) -> Option<String> {
        let metadata = fs::metadata(path).ok()?;
        let mtime = metadata.modified().ok()?;
        let cache_key = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

        if let Some((cached_mtime, cached_content)) = self.cache.get(&cache_key) {
            if *cached_mtime == mtime {
                return Some(cached_content.clone());
            }
        }

        let raw = fs::read(path).ok()?;
        let content = if raw.len() > MAX_BYTES {
            log::warn!(
                "agents_md_truncated path={} original_bytes={} truncated_to={}",
                path.display(),
                raw.len(),
                MAX_BYTES,
            );
            // 截断到 ≤ MAX_BYTES 的最大字符边界，避免多字节字符跨界产生 U+FFFD
            let full = String::from_utf8_lossy(&raw);
            let mut end = MAX_BYTES;
            while end > 0 && !full.is_char_boundary(end) {
                end -= 1;
            }
            full[..end].to_string()
        } else {
            String::from_utf8_lossy(&raw).into_owned()
        };

        self.cache.insert(cache_key, (mtime, content.clone()));
        Some(content)
    }
}
