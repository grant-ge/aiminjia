//! 技能"更新时间"读取的抽象。
//!
//! 当前实现：技能根目录的 mtime。
//! 抽象的目的是把"如何确定一个技能的更新时间"和它的"调用点"解耦——
//! 后续若要改成 SKILL.md mtime、max(目录, SKILL.md)、git 提交时间或上游 server
//! 元数据，只需要换 `SkillUpdatedAtResolver` 的实现，调用方不动。

use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::plugin::skill::types::DiskSkill;

/// 把任意"时间来源"抽象成 RFC 3339 UTC 字符串。
pub trait SkillUpdatedAtResolver {
    /// 读取该技能的"更新时间"。读不到/无效返回 None。
    fn resolve(&self, skill: &DiskSkill) -> Option<String>;
}

/// 默认实现：技能根目录 mtime。
pub struct DirMtimeResolver;

impl SkillUpdatedAtResolver for DirMtimeResolver {
    fn resolve(&self, skill: &DiskSkill) -> Option<String> {
        path_mtime_rfc3339(&skill.root)
    }
}

/// 把任意路径的 mtime 转成 RFC 3339 UTC 字符串。
/// 读不到 metadata、modified 时间无效或在 UNIX_EPOCH 之前都会返回 None。
pub fn path_mtime_rfc3339(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let duration = modified.duration_since(UNIX_EPOCH).ok()?;
    let secs = duration.as_secs() as i64;
    let nanos = duration.subsec_nanos();
    let datetime = chrono::DateTime::<chrono::Utc>::from_timestamp(secs, nanos)?;
    Some(datetime.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::skill::types::{DiskSkill, SkillFrontmatter, SkillSource};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn make_skill(root: PathBuf) -> DiskSkill {
        DiskSkill {
            id: "test".into(),
            root,
            frontmatter: SkillFrontmatter::default(),
            body: String::new(),
            localized: Default::default(),
            source: SkillSource::User,
        }
    }

    #[test]
    fn dir_mtime_resolver_returns_rfc3339_for_existing_dir() {
        let tmp = TempDir::new().unwrap();
        let skill = make_skill(tmp.path().to_path_buf());
        let stamp = DirMtimeResolver.resolve(&skill).expect("should resolve");
        // RFC 3339 长这样：2026-05-13T16:30:00.123456789+00:00 或 ...Z
        assert!(
            stamp.contains('T')
                && (stamp.ends_with('Z') || stamp.contains('+') || stamp.contains('-')),
            "expected RFC 3339, got: {stamp}",
        );
    }

    #[test]
    fn dir_mtime_resolver_returns_none_for_missing_dir() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist");
        let skill = make_skill(missing);
        assert!(DirMtimeResolver.resolve(&skill).is_none());
    }

    #[test]
    fn dir_mtime_resolver_updates_after_child_change() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("skill-root");
        fs::create_dir(&root).unwrap();
        let skill = make_skill(root.clone());
        let first = DirMtimeResolver.resolve(&skill).expect("first stamp");

        // 在目录里新建一个文件 — POSIX 语义下目录 mtime 会被刷新。
        std::thread::sleep(std::time::Duration::from_secs(1));
        fs::write(root.join("SKILL.md"), "---\nname: t\ndescription: d\n---\n").unwrap();

        let second = DirMtimeResolver.resolve(&skill).expect("second stamp");
        assert_ne!(first, second, "目录 mtime 应在子项变更后更新");
    }
}
