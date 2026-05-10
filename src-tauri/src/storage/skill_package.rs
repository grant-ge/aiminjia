//! Skill package format — OPS-standard zip (no manifest, no checksum).
//!
//! 包格式（与 lotus OPS skill_packages API 对齐）：
//! ```text
//! my-skill-v0.1.0.aijia-skill            (zip)
//! └── <skill_id>/                         一级子目录（推荐）或根目录
//!     ├── SKILL.md                        必含
//!     ├── scripts/                        可选
//!     └── references/                     可选
//! ```
//!
//! 也兼容 SKILL.md 直接在 zip 根的扁平布局。
//!
//! 安全约束：
//! - 解压总大小 ≤ 50 MB（与 lotus OPS 后端一致）
//! - 文件数 ≤ 256
//! - 单路径 ≤ 200 chars
//! - 拒绝 zip-slip（`..` / 绝对路径 / Windows 卷根）
//! - 仅允许：根 / scripts/ / references/ 一级子目录

use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const MAX_TOTAL_BYTES: u64 = 50 * 1024 * 1024; // 50 MB
pub const MAX_FILE_COUNT: usize = 256;
pub const MAX_PATH_LEN: usize = 200;

/// 把 source_dir 打包成 OPS 标准 zip 到 dest_path。
/// source_dir 必须直接含 SKILL.md。
/// zip 内布局：`<skill_id>/SKILL.md` + `<skill_id>/scripts/*` + `<skill_id>/references/*`。
pub fn pack_skill_dir(source_dir: &Path, dest_path: &Path, skill_id: &str) -> Result<()> {
    if !source_dir.is_dir() {
        return Err(anyhow!("source dir does not exist: {:?}", source_dir));
    }
    if !source_dir.join("SKILL.md").is_file() {
        return Err(anyhow!("SKILL.md missing under {:?}", source_dir));
    }
    if skill_id.is_empty() {
        return Err(anyhow!("skill_id is empty"));
    }

    let entries = collect_entries(source_dir)?;

    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let file = fs::File::create(dest_path)
        .with_context(|| format!("create {:?}", dest_path))?;
    let mut zip = ZipWriter::new(file);
    let opts: FileOptions<'_, ()> =
        FileOptions::default().compression_method(CompressionMethod::Deflated);

    for (rel, abs) in &entries {
        // OPS 标准：放在 <skill_id>/ 子目录下，便于 server 端识别 plugin_id
        let zip_path = format!("{}/{}", skill_id, rel.to_string_lossy().replace('\\', "/"));
        zip.start_file(&zip_path, opts)?;
        let bytes = fs::read(abs).with_context(|| format!("read {:?}", abs))?;
        zip.write_all(&bytes)?;
    }
    zip.finish()?;
    Ok(())
}

#[derive(Debug)]
pub struct UnpackResult {
    /// 解出的 skill 内容根目录（含 SKILL.md）
    pub skill_dir: PathBuf,
    /// 从包内提取的 skill_id（来源：一级子目录名或 SKILL.md frontmatter.name）
    pub skill_id: String,
}

/// 解包 OPS 标准 zip 到一个新建的临时目录。
/// 兼容两种 zip 布局：
/// - 一级子目录：`<skill_id>/SKILL.md` （推荐）
/// - 扁平：`SKILL.md` 直接在根
pub fn unpack_skill_archive(zip_path: &Path, tmp_root: &Path) -> Result<UnpackResult> {
    let file = fs::File::open(zip_path)
        .with_context(|| format!("open {:?}", zip_path))?;
    unpack_skill_archive_from_reader(file, tmp_root)
}

pub fn unpack_skill_archive_from_reader<R: Read + Seek>(
    reader: R,
    tmp_root: &Path,
) -> Result<UnpackResult> {
    let mut archive = ZipArchive::new(reader).context("open zip")?;
    if archive.len() > MAX_FILE_COUNT {
        return Err(anyhow!(
            "too many files: {} (max {})",
            archive.len(),
            MAX_FILE_COUNT
        ));
    }

    fs::create_dir_all(tmp_root).context("mkdir tmp_root")?;

    let mut total_bytes: u64 = 0;
    let mut entries: Vec<(PathBuf, Vec<u8>)> = vec![];
    let mut detected_subdir: Option<String> = None;
    let mut has_root_skill_md = false;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).context("zip entry")?;
        let raw = entry.name().to_string();
        if raw.ends_with('/') {
            continue;
        }
        if raw.len() > MAX_PATH_LEN {
            return Err(anyhow!("path too long: {}", raw));
        }
        total_bytes += entry.size();
        if total_bytes > MAX_TOTAL_BYTES {
            return Err(anyhow!("uncompressed size exceeds 50MB"));
        }

        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).context("read entry")?;

        let rel = sanitize_relative(&raw)?;
        // 检测布局：根有 SKILL.md  vs  <subdir>/SKILL.md
        let comps: Vec<_> = rel.components().collect();
        if comps.len() == 1 && comps[0].as_os_str() == "SKILL.md" {
            has_root_skill_md = true;
        }
        if comps.len() == 2 && comps[1].as_os_str() == "SKILL.md" {
            let subdir = comps[0].as_os_str().to_string_lossy().to_string();
            detected_subdir = Some(subdir);
        }
        entries.push((rel, buf));
    }

    // 决定剥离的前缀
    let strip_prefix: Option<String> = if has_root_skill_md {
        None
    } else if let Some(s) = detected_subdir.clone() {
        Some(s)
    } else {
        return Err(anyhow!(
            "SKILL.md not found at root or one-level subdirectory"
        ));
    };

    let skill_dir = tmp_root.join("skill");
    fs::create_dir_all(&skill_dir).context("mkdir skill_dir")?;

    for (rel, bytes) in &entries {
        // 剥离一级子目录（若存在）
        let final_rel = match strip_prefix.as_ref() {
            Some(prefix) => {
                let comps: Vec<_> = rel.components().collect();
                if comps.is_empty() || comps[0].as_os_str() != prefix.as_str() {
                    // zip 内可能混有非 skill 文件（如老格式 manifest.json） — 静默跳过
                    continue;
                }
                let mut p = PathBuf::new();
                for c in comps.iter().skip(1) {
                    p.push(c.as_os_str());
                }
                p
            }
            None => rel.clone(),
        };
        if final_rel.as_os_str().is_empty() {
            continue;
        }
        validate_skill_subpath(&final_rel)?;

        let abs = skill_dir.join(&final_rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).context("mkdir parent")?;
        }
        fs::write(&abs, bytes).context("write file")?;
    }

    if !skill_dir.join("SKILL.md").is_file() {
        return Err(anyhow!("SKILL.md missing in archive"));
    }

    // skill_id：优先来自 SKILL.md frontmatter.name；fallback 到 strip_prefix
    let skill_md = fs::read_to_string(skill_dir.join("SKILL.md")).context("read SKILL.md")?;
    let skill_id = match crate::plugin::skill::frontmatter::parse_skill_md(&skill_md) {
        Ok(parsed) => parsed.frontmatter.name,
        Err(_) => strip_prefix.unwrap_or_else(|| "unknown".to_string()),
    };

    Ok(UnpackResult { skill_dir, skill_id })
}

/// 收集 source_dir 下需要打包的相对路径。
/// 仅保留 SKILL.md / scripts/* / references/* 一级文件。
fn collect_entries(source_dir: &Path) -> Result<Vec<(PathBuf, PathBuf)>> {
    let mut out = vec![];
    let skill_md = source_dir.join("SKILL.md");
    if skill_md.is_file() {
        out.push((PathBuf::from("SKILL.md"), skill_md));
    }
    for sub in ["scripts", "references"] {
        let dir = source_dir.join(sub);
        if !dir.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname == ".DS_Store" {
                continue;
            }
            crate::storage::safe_filename::ensure_safe_filename(&fname)
                .map_err(|e| anyhow!("unsafe filename in {}: {}", sub, e))?;
            out.push((PathBuf::from(sub).join(&fname), p));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn sanitize_relative(s: &str) -> Result<PathBuf> {
    let p = Path::new(s);
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::Normal(seg) => out.push(seg),
            Component::CurDir => {}
            _ => return Err(anyhow!("unsafe path component in {}", s)),
        }
    }
    if out.as_os_str().is_empty() {
        return Err(anyhow!("empty path"));
    }
    Ok(out)
}

fn validate_skill_subpath(rel: &Path) -> Result<()> {
    let comps: Vec<_> = rel.components().collect();
    if comps.is_empty() {
        return Err(anyhow!("empty path"));
    }
    // 一级子目录白名单：scripts / references 是用户技能常用；assets / templates / docs
    // 是模板类技能（如 html-ppt）常用。允许后两个递归任意深度（至 MAX_PATH_LEN 限制）。
    let one_level_only: std::collections::HashSet<&str> =
        ["scripts", "references"].into_iter().collect();
    let nested_allowed: std::collections::HashSet<&str> =
        ["assets", "templates", "docs"].into_iter().collect();
    match comps.len() {
        1 => {
            let name = comps[0].as_os_str().to_string_lossy();
            if name != "SKILL.md" && name != "LICENSE" && name != "README.md" {
                return Err(anyhow!("top-level file '{}' not allowed", name));
            }
        }
        2 => {
            let dir = comps[0].as_os_str().to_string_lossy();
            if !one_level_only.contains(dir.as_ref())
                && !nested_allowed.contains(dir.as_ref())
            {
                return Err(anyhow!(
                    "subdirectory '{}' not allowed (allowed: scripts/ references/ assets/ templates/ docs/)",
                    dir
                ));
            }
            let fname = comps[1].as_os_str().to_string_lossy();
            crate::storage::safe_filename::ensure_safe_filename(&fname)
                .map_err(|e| anyhow!("unsafe filename '{}': {}", fname, e))?;
        }
        _ => {
            // 三级及以上：仅 assets/ templates/ docs/ 允许（模板类技能）。
            let top = comps[0].as_os_str().to_string_lossy();
            if !nested_allowed.contains(top.as_ref()) {
                return Err(anyhow!("path too deep under '{}': {:?}", top, rel));
            }
            // 校验最后一段是合法文件名，中间段不强校验（容许跨平台子目录）。
            let last = comps[comps.len() - 1].as_os_str().to_string_lossy();
            crate::storage::safe_filename::ensure_safe_filename(&last)
                .map_err(|e| anyhow!("unsafe filename '{}': {}", last, e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_skill_dir(tmp: &Path) -> PathBuf {
        let dir = tmp.join("src");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            "---\nname: my-skill\ndescription: ok\n---\nbody",
        )
        .unwrap();
        fs::create_dir_all(dir.join("scripts")).unwrap();
        fs::write(dir.join("scripts/foo.py"), "print(1)").unwrap();
        fs::create_dir_all(dir.join("references")).unwrap();
        fs::write(dir.join("references/note.md"), "# note").unwrap();
        dir
    }

    #[test]
    fn pack_then_unpack_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let src = make_skill_dir(tmp.path());
        let dest = tmp.path().join("out").join("my-skill-v0.1.0.aijia-skill");
        pack_skill_dir(&src, &dest, "my-skill").unwrap();
        assert!(dest.exists());

        let unpack_root = tmp.path().join("unpack");
        let res = unpack_skill_archive(&dest, &unpack_root).unwrap();
        assert_eq!(res.skill_id, "my-skill");
        assert!(res.skill_dir.join("SKILL.md").is_file());
        assert!(res.skill_dir.join("scripts/foo.py").is_file());
        assert!(res.skill_dir.join("references/note.md").is_file());
    }

    #[test]
    fn pack_rejects_when_skill_md_missing() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("empty");
        fs::create_dir_all(&src).unwrap();
        let dest = tmp.path().join("x.aijia-skill");
        let err = pack_skill_dir(&src, &dest, "x").unwrap_err();
        assert!(err.to_string().contains("SKILL.md missing"));
    }

    #[test]
    fn unpack_rejects_zip_slip() {
        let tmp = TempDir::new().unwrap();
        let zip_path = tmp.path().join("evil.aijia-skill");
        {
            let f = fs::File::create(&zip_path).unwrap();
            let mut z = ZipWriter::new(f);
            let opts: FileOptions<'_, ()> = FileOptions::default();
            z.start_file("../../etc/passwd", opts).unwrap();
            z.write_all(b"evil").unwrap();
            z.finish().unwrap();
        }
        let err = unpack_skill_archive(&zip_path, &tmp.path().join("out")).unwrap_err();
        assert!(err.to_string().contains("unsafe") || err.to_string().contains("not found"));
    }

    #[test]
    fn unpack_rejects_disallowed_subdir() {
        let tmp = TempDir::new().unwrap();
        let zip_path = tmp.path().join("evil.aijia-skill");
        {
            let f = fs::File::create(&zip_path).unwrap();
            let mut z = ZipWriter::new(f);
            let opts: FileOptions<'_, ()> = FileOptions::default();
            z.start_file("my-skill/SKILL.md", opts).unwrap();
            z.write_all(b"---\nname: x\ndescription: x\n---\n").unwrap();
            z.start_file("my-skill/secret/password.txt", opts).unwrap();
            z.write_all(b"hi").unwrap();
            z.finish().unwrap();
        }
        let err = unpack_skill_archive(&zip_path, &tmp.path().join("out")).unwrap_err();
        assert!(err.to_string().contains("not allowed"));
    }

    #[test]
    fn unpack_accepts_root_layout() {
        // 兼容："SKILL.md 直接在 zip 根" 的扁平布局
        let tmp = TempDir::new().unwrap();
        let zip_path = tmp.path().join("flat.aijia-skill");
        {
            let f = fs::File::create(&zip_path).unwrap();
            let mut z = ZipWriter::new(f);
            let opts: FileOptions<'_, ()> = FileOptions::default();
            z.start_file("SKILL.md", opts).unwrap();
            z.write_all(b"---\nname: flat-skill\ndescription: x\n---\nbody")
                .unwrap();
            z.start_file("scripts/util.py", opts).unwrap();
            z.write_all(b"print(1)").unwrap();
            z.finish().unwrap();
        }
        let res = unpack_skill_archive(&zip_path, &tmp.path().join("out")).unwrap();
        assert_eq!(res.skill_id, "flat-skill");
        assert!(res.skill_dir.join("SKILL.md").is_file());
        assert!(res.skill_dir.join("scripts/util.py").is_file());
    }

    #[test]
    fn unpack_skips_old_manifest_json_for_compat() {
        // 老 .aijia-skill 包带 manifest.json — 解包时应静默跳过，仍然能成功
        let tmp = TempDir::new().unwrap();
        let zip_path = tmp.path().join("old.aijia-skill");
        {
            let f = fs::File::create(&zip_path).unwrap();
            let mut z = ZipWriter::new(f);
            let opts: FileOptions<'_, ()> = FileOptions::default();
            // 老格式：manifest.json 在根 + skill/ 子目录含 SKILL.md
            // 新解包逻辑：检测到 skill/SKILL.md 走子目录路径，manifest.json 因不在 skill/ 下被跳过
            z.start_file("manifest.json", opts).unwrap();
            z.write_all(b"{}").unwrap();
            z.start_file("skill/SKILL.md", opts).unwrap();
            z.write_all(b"---\nname: legacy\ndescription: ok\n---\nbody")
                .unwrap();
            z.finish().unwrap();
        }
        let res = unpack_skill_archive(&zip_path, &tmp.path().join("out")).unwrap();
        assert_eq!(res.skill_id, "legacy");
    }

    #[test]
    fn sanitize_relative_rejects_traversal() {
        assert!(sanitize_relative("../escape").is_err());
        assert!(sanitize_relative("/abs/path").is_err());
        assert!(sanitize_relative("foo/../bar").is_err());
        assert!(sanitize_relative("foo").is_ok());
        assert!(sanitize_relative("foo/bar").is_ok());
    }
}
