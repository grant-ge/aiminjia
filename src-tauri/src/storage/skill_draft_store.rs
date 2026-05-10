//! Skill-Smith (小程) 草稿存储。
//!
//! 路径布局：
//! ```text
//! ~/.renlijia/users/{scope}/skill-drafts/
//! └── <draft_id>/
//!     ├── meta.json        草稿元数据
//!     ├── SKILL.md         主产物（小程随对话写入）
//!     ├── scripts/         可选 Python 脚本（M2 启用）
//!     └── references/      可选参考文档（M2 启用）
//! ```
//!
//! - `draft_id` 一般等于 conversation_id；保证一会话一草稿。
//! - 安装到正式技能库后 `meta.installed_to` 记录目标路径，但草稿不立即删除
//!   （用户可继续修改并重新安装）。
//! - 7 天未活动的草稿在启动期 GC 中清理（见 `runtime/tasks/skill_draft_gc.rs`）。

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::storage::safe_filename::ensure_safe_filename;
use crate::storage::{AiJiaHome, UserScope};

const META_FILENAME: &str = "meta.json";
const SKILL_FILENAME: &str = "SKILL.md";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftMeta {
    pub draft_id: String,
    pub conversation_id: Option<String>,
    /// kebab-case，与 frontmatter.name 一致。空串表示尚未命名。
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub created_at: DateTime<Utc>,
    pub last_modified_at: DateTime<Utc>,
    /// 安装后的目标路径（绝对路径）。None 表示尚未安装。
    #[serde(default)]
    pub installed_to: Option<PathBuf>,
}

impl DraftMeta {
    fn new(draft_id: String, conversation_id: Option<String>, name: String, description: String) -> Self {
        let now = Utc::now();
        Self {
            draft_id,
            conversation_id,
            name,
            description,
            created_at: now,
            last_modified_at: now,
            installed_to: None,
        }
    }

    pub fn touch(&mut self) {
        self.last_modified_at = Utc::now();
    }
}

#[derive(Debug, Clone)]
pub struct SkillDraftStore {
    home: Arc<AiJiaHome>,
}

impl SkillDraftStore {
    pub fn new(home: Arc<AiJiaHome>) -> Self {
        Self { home }
    }

    /// `~/.renlijia/users/{scope}/skill-drafts/`
    pub fn drafts_root(&self, scope: &UserScope) -> PathBuf {
        self.home.user_skill_drafts_dir(scope)
    }

    /// `<drafts_root>/<draft_id>/`
    pub fn draft_dir(&self, scope: &UserScope, draft_id: &str) -> Result<PathBuf> {
        ensure_safe_filename(draft_id)
            .map_err(|e| anyhow!("invalid draft_id '{}': {}", draft_id, e))?;
        Ok(self.drafts_root(scope).join(draft_id))
    }

    /// 创建新草稿。同 draft_id 已存在则返回错误（除非允许 overwrite）。
    pub fn create(
        &self,
        scope: &UserScope,
        draft_id: &str,
        conversation_id: Option<String>,
        name: &str,
        description: &str,
    ) -> Result<DraftMeta> {
        let dir = self.draft_dir(scope, draft_id)?;
        if dir.exists() {
            return Err(anyhow!("draft '{}' already exists", draft_id));
        }
        fs::create_dir_all(&dir).with_context(|| format!("create draft dir {:?}", dir))?;
        let meta = DraftMeta::new(
            draft_id.to_string(),
            conversation_id,
            name.to_string(),
            description.to_string(),
        );
        self.write_meta(scope, &meta)?;
        // 写空 SKILL.md
        fs::write(dir.join(SKILL_FILENAME), "")
            .with_context(|| format!("write empty SKILL.md to {:?}", dir))?;
        Ok(meta)
    }

    pub fn read_meta(&self, scope: &UserScope, draft_id: &str) -> Result<DraftMeta> {
        let path = self.draft_dir(scope, draft_id)?.join(META_FILENAME);
        let bytes = fs::read(&path).with_context(|| format!("read {:?}", path))?;
        let meta: DraftMeta = serde_json::from_slice(&bytes)
            .with_context(|| format!("parse meta.json {:?}", path))?;
        Ok(meta)
    }

    pub fn write_meta(&self, scope: &UserScope, meta: &DraftMeta) -> Result<()> {
        let dir = self.draft_dir(scope, &meta.draft_id)?;
        fs::create_dir_all(&dir)?;
        let path = dir.join(META_FILENAME);
        let bytes = serde_json::to_vec_pretty(meta)?;
        fs::write(&path, bytes).with_context(|| format!("write {:?}", path))?;
        Ok(())
    }

    /// 写入 SKILL.md（整体覆盖）。同时 touch meta。
    pub fn write_skill_md(&self, scope: &UserScope, draft_id: &str, content: &str) -> Result<()> {
        let dir = self.draft_dir(scope, draft_id)?;
        if !dir.exists() {
            return Err(anyhow!("draft '{}' does not exist", draft_id));
        }
        let path = dir.join(SKILL_FILENAME);
        fs::write(&path, content).with_context(|| format!("write {:?}", path))?;
        // bump meta
        if let Ok(mut meta) = self.read_meta(scope, draft_id) {
            meta.touch();
            self.write_meta(scope, &meta).ok();
        }
        Ok(())
    }

    pub fn read_skill_md(&self, scope: &UserScope, draft_id: &str) -> Result<String> {
        let path = self.draft_dir(scope, draft_id)?.join(SKILL_FILENAME);
        Ok(fs::read_to_string(&path).with_context(|| format!("read {:?}", path))?)
    }

    /// 写入额外文件，path 必须是 `scripts/<file>` 或 `references/<file>` 形式（仅一级子目录，无 ..）。
    pub fn write_extra_file(
        &self,
        scope: &UserScope,
        draft_id: &str,
        rel_path: &str,
        content: &str,
    ) -> Result<()> {
        let (subdir, fname) = parse_extra_path(rel_path)?;
        let dir = self.draft_dir(scope, draft_id)?.join(subdir);
        fs::create_dir_all(&dir)?;
        let path = dir.join(fname);
        fs::write(&path, content).with_context(|| format!("write {:?}", path))?;
        if let Ok(mut meta) = self.read_meta(scope, draft_id) {
            meta.touch();
            self.write_meta(scope, &meta).ok();
        }
        Ok(())
    }

    /// 列出所有草稿（按 last_modified_at 倒序）。
    pub fn list(&self, scope: &UserScope) -> Result<Vec<DraftMeta>> {
        let root = self.drafts_root(scope);
        if !root.exists() {
            return Ok(vec![]);
        }
        let mut out = vec![];
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().to_string();
            if let Ok(meta) = self.read_meta(scope, &id) {
                out.push(meta);
            }
        }
        out.sort_by(|a, b| b.last_modified_at.cmp(&a.last_modified_at));
        Ok(out)
    }

    /// 删除草稿目录（包括 meta + SKILL.md + 子目录）。
    pub fn discard(&self, scope: &UserScope, draft_id: &str) -> Result<()> {
        let dir = self.draft_dir(scope, draft_id)?;
        if dir.exists() {
            fs::remove_dir_all(&dir).with_context(|| format!("rm -rf {:?}", dir))?;
        }
        Ok(())
    }

    /// 删除超过 `max_age_days` 天未活动的草稿。返回被清理的 draft_id 列表。
    /// 已安装的草稿（meta.installed_to=Some）不会被清理。
    pub fn gc_old_drafts(&self, scope: &UserScope, max_age_days: i64) -> Result<Vec<String>> {
        let cutoff = Utc::now() - chrono::Duration::days(max_age_days);
        let mut removed = vec![];
        for meta in self.list(scope)? {
            if meta.installed_to.is_some() {
                continue;
            }
            if meta.last_modified_at < cutoff {
                if self.discard(scope, &meta.draft_id).is_ok() {
                    removed.push(meta.draft_id);
                }
            }
        }
        Ok(removed)
    }

    /// 标记已安装（不删除草稿，便于继续编辑）。
    pub fn mark_installed(&self, scope: &UserScope, draft_id: &str, target: &Path) -> Result<()> {
        let mut meta = self.read_meta(scope, draft_id)?;
        meta.installed_to = Some(target.to_path_buf());
        meta.touch();
        self.write_meta(scope, &meta)
    }

    /// 把整个 draft 目录（除 meta.json）复制到 target 目录。
    /// target 必须是不存在的或空目录；调用方负责冲突处理。
    pub fn copy_to(&self, scope: &UserScope, draft_id: &str, target: &Path) -> Result<()> {
        let src = self.draft_dir(scope, draft_id)?;
        if !src.exists() {
            return Err(anyhow!("draft '{}' does not exist", draft_id));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        if target.exists() {
            return Err(anyhow!("target '{:?}' already exists", target));
        }
        fs::create_dir_all(target)?;
        copy_recursive(&src, target, &|p| p.file_name().map(|n| n != META_FILENAME).unwrap_or(true))?;
        Ok(())
    }
}

/// 校验额外文件相对路径，只允许 `scripts/<safe_filename>` 或 `references/<safe_filename>`。
fn parse_extra_path(rel_path: &str) -> Result<(&'static str, String)> {
    let normalized = rel_path.trim_start_matches('.').trim_start_matches('/');
    let (subdir_raw, rest) = normalized
        .split_once('/')
        .ok_or_else(|| anyhow!("path must be 'scripts/<file>' or 'references/<file>'"))?;
    let subdir: &'static str = match subdir_raw {
        "scripts" => "scripts",
        "references" => "references",
        other => return Err(anyhow!("subdir must be 'scripts' or 'references', got '{}'", other)),
    };
    if rest.contains('/') || rest.contains('\\') {
        return Err(anyhow!("only one-level deep is allowed"));
    }
    if rest.is_empty() {
        return Err(anyhow!("filename is empty"));
    }
    ensure_safe_filename(rest).map_err(|e| anyhow!("unsafe filename '{}': {}", rest, e))?;
    Ok((subdir, rest.to_string()))
}

fn copy_recursive<F: Fn(&Path) -> bool>(src: &Path, dst: &Path, filter: &F) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let p = entry.path();
        if !filter(&p) {
            continue;
        }
        let dst_p = dst.join(entry.file_name());
        if p.is_dir() {
            fs::create_dir_all(&dst_p)?;
            copy_recursive(&p, &dst_p, filter)?;
        } else {
            fs::copy(&p, &dst_p)?;
        }
    }
    Ok(())
}

/// 启动期对所有用户作用域跑一次 GC。
/// 直接扫 `~/.renlijia/users/*/skill-drafts/<draft_id>/meta.json`，不需要登录态。
pub fn gc_all_users(home: &AiJiaHome, max_age_days: i64) -> Result<usize> {
    let users_root = home.root().join("users");
    if !users_root.exists() {
        return Ok(0);
    }
    let cutoff = Utc::now() - chrono::Duration::days(max_age_days);
    let mut total = 0usize;
    for user_entry in fs::read_dir(&users_root)? {
        let user_entry = user_entry?;
        if !user_entry.file_type()?.is_dir() {
            continue;
        }
        let drafts_dir = user_entry.path().join("skill-drafts");
        if !drafts_dir.exists() {
            continue;
        }
        for draft_entry in fs::read_dir(&drafts_dir)? {
            let draft_entry = draft_entry?;
            if !draft_entry.file_type()?.is_dir() {
                continue;
            }
            let dir = draft_entry.path();
            let meta_path = dir.join(META_FILENAME);
            let bytes = match fs::read(&meta_path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let meta: DraftMeta = match serde_json::from_slice(&bytes) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.installed_to.is_some() {
                continue;
            }
            if meta.last_modified_at < cutoff {
                if fs::remove_dir_all(&dir).is_ok() {
                    total += 1;
                }
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fixture() -> (TempDir, SkillDraftStore, UserScope) {
        let tmp = TempDir::new().unwrap();
        let home = AiJiaHome::from_path(tmp.path().to_path_buf());
        let store = SkillDraftStore::new(Arc::new(home));
        let scope = UserScope::new(0, 0);
        (tmp, store, scope)
    }

    #[test]
    fn create_and_read_meta() {
        let (_tmp, store, scope) = fixture();
        let meta = store
            .create(&scope, "draft-1", Some("conv-1".into()), "my-skill", "desc")
            .unwrap();
        assert_eq!(meta.draft_id, "draft-1");
        assert_eq!(meta.name, "my-skill");
        assert!(meta.installed_to.is_none());

        let read = store.read_meta(&scope, "draft-1").unwrap();
        assert_eq!(read.draft_id, "draft-1");
    }

    #[test]
    fn create_rejects_duplicate() {
        let (_tmp, store, scope) = fixture();
        store.create(&scope, "draft-1", None, "x", "y").unwrap();
        let err = store.create(&scope, "draft-1", None, "x", "y").unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn write_and_read_skill_md_bumps_meta() {
        let (_tmp, store, scope) = fixture();
        let meta_a = store
            .create(&scope, "draft-1", None, "x", "y")
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        store
            .write_skill_md(&scope, "draft-1", "---\nname: x\n---\nbody")
            .unwrap();
        let meta_b = store.read_meta(&scope, "draft-1").unwrap();
        assert!(meta_b.last_modified_at > meta_a.last_modified_at);
        let body = store.read_skill_md(&scope, "draft-1").unwrap();
        assert!(body.contains("name: x"));
    }

    #[test]
    fn write_extra_file_only_in_whitelist_subdirs() {
        let (_tmp, store, scope) = fixture();
        store.create(&scope, "draft-1", None, "x", "y").unwrap();
        store
            .write_extra_file(&scope, "draft-1", "scripts/foo.py", "print('hi')")
            .unwrap();
        store
            .write_extra_file(&scope, "draft-1", "references/spec.md", "# spec")
            .unwrap();
        // forbidden
        assert!(store
            .write_extra_file(&scope, "draft-1", "evil/foo.py", "x")
            .is_err());
        assert!(store
            .write_extra_file(&scope, "draft-1", "scripts/sub/foo.py", "x")
            .is_err());
        assert!(store
            .write_extra_file(&scope, "draft-1", "../escape.py", "x")
            .is_err());
    }

    #[test]
    fn parse_extra_path_rejects_traversal_and_bad_subdir() {
        assert!(parse_extra_path("scripts/foo.py").is_ok());
        assert!(parse_extra_path("references/foo.md").is_ok());
        assert!(parse_extra_path("/scripts/foo.py").is_ok()); // leading slash trimmed
        assert!(parse_extra_path("foo.py").is_err());
        assert!(parse_extra_path("evil/foo.py").is_err());
        assert!(parse_extra_path("scripts/").is_err());
        assert!(parse_extra_path("scripts/sub/foo.py").is_err());
    }

    #[test]
    fn list_sorted_by_last_modified_desc() {
        let (_tmp, store, scope) = fixture();
        store.create(&scope, "old", None, "x", "y").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        store.create(&scope, "new", None, "x", "y").unwrap();
        let list = store.list(&scope).unwrap();
        assert_eq!(list[0].draft_id, "new");
        assert_eq!(list[1].draft_id, "old");
    }

    #[test]
    fn discard_removes_dir() {
        let (_tmp, store, scope) = fixture();
        store.create(&scope, "draft-1", None, "x", "y").unwrap();
        store.discard(&scope, "draft-1").unwrap();
        assert!(store.read_meta(&scope, "draft-1").is_err());
    }

    #[test]
    fn copy_to_skips_meta_json() {
        let (tmp, store, scope) = fixture();
        store.create(&scope, "draft-1", None, "my-skill", "y").unwrap();
        store
            .write_skill_md(&scope, "draft-1", "body")
            .unwrap();
        store
            .write_extra_file(&scope, "draft-1", "scripts/foo.py", "print('hi')")
            .unwrap();
        let target = tmp.path().join("installed").join("my-skill");
        store.copy_to(&scope, "draft-1", &target).unwrap();
        assert!(target.join("SKILL.md").exists());
        assert!(target.join("scripts/foo.py").exists());
        assert!(!target.join("meta.json").exists()); // meta is draft-only
    }

    #[test]
    fn copy_to_refuses_existing_target() {
        let (tmp, store, scope) = fixture();
        store.create(&scope, "draft-1", None, "x", "y").unwrap();
        let target = tmp.path().join("dest");
        fs::create_dir_all(&target).unwrap();
        let err = store.copy_to(&scope, "draft-1", &target).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn mark_installed_records_target() {
        let (tmp, store, scope) = fixture();
        store.create(&scope, "draft-1", None, "x", "y").unwrap();
        let target = tmp.path().join("installed").join("x");
        store.mark_installed(&scope, "draft-1", &target).unwrap();
        let meta = store.read_meta(&scope, "draft-1").unwrap();
        assert_eq!(meta.installed_to.as_deref(), Some(target.as_path()));
    }

    #[test]
    fn rejects_unsafe_draft_id() {
        let (_tmp, store, scope) = fixture();
        assert!(store.draft_dir(&scope, "../escape").is_err());
        assert!(store.draft_dir(&scope, "a/b").is_err());
    }

    #[test]
    fn gc_removes_old_unsaved_drafts() {
        let (_tmp, store, scope) = fixture();
        // 1) old, not installed → should be removed
        store.create(&scope, "old-untouched", None, "x", "y").unwrap();
        let mut m = store.read_meta(&scope, "old-untouched").unwrap();
        m.last_modified_at = Utc::now() - chrono::Duration::days(10);
        store.write_meta(&scope, &m).unwrap();
        // 2) old, but installed → keep
        store.create(&scope, "old-installed", None, "x", "y").unwrap();
        let mut m = store.read_meta(&scope, "old-installed").unwrap();
        m.last_modified_at = Utc::now() - chrono::Duration::days(10);
        m.installed_to = Some(PathBuf::from("/somewhere"));
        store.write_meta(&scope, &m).unwrap();
        // 3) recent, not installed → keep
        store.create(&scope, "recent", None, "x", "y").unwrap();

        let removed = store.gc_old_drafts(&scope, 7).unwrap();
        assert_eq!(removed, vec!["old-untouched".to_string()]);
        assert!(store.read_meta(&scope, "old-installed").is_ok());
        assert!(store.read_meta(&scope, "recent").is_ok());
    }
}
