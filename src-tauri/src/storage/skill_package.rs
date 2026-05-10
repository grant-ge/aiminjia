//! `.aijia-skill` package format — pack / unpack helpers.
//!
//! Format (zip):
//! ```text
//! my-skill-v0.1.0.aijia-skill                  (zip)
//! ├── manifest.json                             必含
//! │   {
//! │     "format_version": 1,
//! │     "id": "my-skill",
//! │     "name": "My Skill",
//! │     "version": "0.1.0",
//! │     "author": "<email or name>",
//! │     "created_at": "2026-05-09T12:00:00Z",
//! │     "exported_from": "skill-smith@v0.5.18",
//! │     "checksum_sha256": "..."   对 skill/ 目录内容打包后的哈希
//! │   }
//! └── skill/
//!     ├── SKILL.md
//!     ├── scripts/
//!     └── references/
//! ```
//!
//! 安全约束：
//! - 解压总大小 ≤ 50 MB
//! - 文件数 ≤ 256
//! - 路径强制正规化，不能逃出 skill/ 子目录（zip-slip 防御）
//! - skill/ 下仅允许 SKILL.md、scripts/* 一级、references/* 一级（与草稿规则一致）
//! - manifest 中的 id 必须与 SKILL.md frontmatter.name 一致

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const FORMAT_VERSION: u32 = 1;
pub const MAX_TOTAL_BYTES: u64 = 50 * 1024 * 1024; // 50 MB
pub const MAX_FILE_COUNT: usize = 256;
pub const MAX_PATH_LEN: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub format_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub exported_from: Option<String>,
    pub checksum_sha256: String,
}

/// 打包 `source_dir` 下（约定为 SKILL.md 所在目录）的内容，写为 zip 到 `dest_path`。
///
/// `source_dir` 必须直接包含 `SKILL.md`。可选 `scripts/`、`references/` 一级子目录。
pub fn pack_skill_dir(
    source_dir: &Path,
    dest_path: &Path,
    skill_id: &str,
    skill_name: &str,
    version: &str,
    author: Option<&str>,
    exported_from: &str,
) -> Result<PackageManifest> {
    if !source_dir.is_dir() {
        return Err(anyhow!("source dir does not exist: {:?}", source_dir));
    }
    if !source_dir.join("SKILL.md").is_file() {
        return Err(anyhow!("SKILL.md missing under {:?}", source_dir));
    }

    // 收集文件清单（filter_extra_files），按相对路径升序，保证 checksum 稳定。
    let entries = collect_entries(source_dir)?;

    // 计算 checksum：对 [(rel_path, content_bytes)] 顺序流式 hash。
    let mut hasher = Sha256::new();
    for (rel, abs) in &entries {
        hasher.update(rel.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        let bytes = fs::read(abs).with_context(|| format!("read {:?}", abs))?;
        hasher.update(&bytes);
    }
    let checksum = format!("{:x}", hasher.finalize());

    let manifest = PackageManifest {
        format_version: FORMAT_VERSION,
        id: skill_id.to_string(),
        name: skill_name.to_string(),
        version: version.to_string(),
        author: author.map(str::to_string),
        created_at: Utc::now(),
        exported_from: Some(exported_from.to_string()),
        checksum_sha256: checksum,
    };

    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let file = fs::File::create(dest_path)
        .with_context(|| format!("create {:?}", dest_path))?;
    let mut zip = ZipWriter::new(file);
    let opts: FileOptions<'_, ()> = FileOptions::default().compression_method(CompressionMethod::Deflated);

    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    zip.start_file("manifest.json", opts)?;
    zip.write_all(&manifest_bytes)?;

    for (rel, abs) in &entries {
        let zip_path = format!("skill/{}", rel.to_string_lossy().replace('\\', "/"));
        zip.start_file(&zip_path, opts)?;
        let bytes = fs::read(abs).with_context(|| format!("read {:?}", abs))?;
        zip.write_all(&bytes)?;
    }
    zip.finish()?;
    Ok(manifest)
}

#[derive(Debug)]
pub struct UnpackResult {
    pub manifest: PackageManifest,
    pub skill_dir: PathBuf,
}

/// 解包 `.aijia-skill` zip 到一个新建的临时目录，返回临时目录里的 `skill/` 子目录路径。
/// 调用方负责后续把 `skill_dir` 移动到正式安装位置 + 清理 tmp。
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
        return Err(anyhow!("too many files: {} (max {})", archive.len(), MAX_FILE_COUNT));
    }

    fs::create_dir_all(tmp_root).context("mkdir tmp_root")?;

    let mut manifest: Option<PackageManifest> = None;
    let mut total_bytes: u64 = 0;
    let mut skill_files: Vec<(PathBuf, Vec<u8>)> = vec![];

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).context("zip entry")?;
        let raw = entry.name().to_string();
        if raw.ends_with('/') {
            continue; // skip directories
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

        if raw == "manifest.json" {
            let m: PackageManifest = serde_json::from_slice(&buf)
                .context("parse manifest.json")?;
            manifest = Some(m);
            continue;
        }

        // 必须以 "skill/" 开头
        let rest = match raw.strip_prefix("skill/") {
            Some(r) => r,
            None => {
                return Err(anyhow!(
                    "unexpected entry outside skill/: {} (only manifest.json + skill/* allowed)",
                    raw
                ))
            }
        };
        let rel = sanitize_relative(rest)?;
        validate_skill_subpath(&rel)?;
        skill_files.push((rel, buf));
    }

    let manifest = manifest.ok_or_else(|| anyhow!("manifest.json missing"))?;
    if manifest.format_version != FORMAT_VERSION {
        return Err(anyhow!(
            "unsupported format_version {} (expected {})",
            manifest.format_version, FORMAT_VERSION
        ));
    }
    // skill/ 下必须有 SKILL.md
    if !skill_files.iter().any(|(p, _)| p == Path::new("SKILL.md")) {
        return Err(anyhow!("skill/SKILL.md missing in archive"));
    }

    // 把内容写到 tmp_root/skill/...
    let skill_dir = tmp_root.join("skill");
    fs::create_dir_all(&skill_dir).context("mkdir skill_dir")?;
    let mut hasher = Sha256::new();
    let mut sorted = skill_files;
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (rel, bytes) in &sorted {
        let abs = skill_dir.join(rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).context("mkdir parent")?;
        }
        fs::write(&abs, bytes).context("write file")?;
        hasher.update(rel.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        hasher.update(bytes);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != manifest.checksum_sha256 {
        return Err(anyhow!(
            "checksum mismatch: expected {}, computed {} (archive may be corrupt or tampered)",
            manifest.checksum_sha256, actual
        ));
    }

    // manifest.id 必须与 frontmatter.name 一致
    let skill_md = fs::read_to_string(skill_dir.join("SKILL.md")).context("read SKILL.md")?;
    let parsed = crate::plugin::skill::frontmatter::parse_skill_md(&skill_md)
        .context("parse SKILL.md")?;
    if parsed.frontmatter.name != manifest.id {
        return Err(anyhow!(
            "manifest.id ({}) != SKILL.md frontmatter.name ({})",
            manifest.id, parsed.frontmatter.name
        ));
    }

    Ok(UnpackResult { manifest, skill_dir })
}

/// 收集 source_dir 下所有需要打包的相对路径。仅保留 SKILL.md / scripts/* / references/*
/// 一级文件，按相对路径升序返回（保证 checksum 与平台无关）。
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
            crate::storage::safe_filename::ensure_safe_filename(&fname)
                .map_err(|e| anyhow!("unsafe filename in {}: {}", sub, e))?;
            out.push((PathBuf::from(sub).join(&fname), p));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// 把 zip 内的相对路径正规化为绝对安全的相对 PathBuf：
/// - 拒绝绝对路径
/// - 拒绝 `..`
/// - 拒绝 Windows 卷根盘符 (`C:`)
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

/// 校验 skill/ 下的子路径合法：
/// - 一级仅允许 SKILL.md / scripts/<file> / references/<file>
/// - 仅一级深度
fn validate_skill_subpath(rel: &Path) -> Result<()> {
    let comps: Vec<_> = rel.components().collect();
    if comps.is_empty() {
        return Err(anyhow!("empty path"));
    }
    let allowed_subdirs: HashSet<&str> = ["scripts", "references"].into_iter().collect();
    match comps.len() {
        1 => {
            let name = comps[0].as_os_str().to_string_lossy();
            if name != "SKILL.md" {
                return Err(anyhow!(
                    "top-level file '{}' not allowed (only SKILL.md)",
                    name
                ));
            }
        }
        2 => {
            let dir = comps[0].as_os_str().to_string_lossy();
            if !allowed_subdirs.contains(dir.as_ref()) {
                return Err(anyhow!(
                    "subdirectory '{}' not allowed (only scripts/ + references/)",
                    dir
                ));
            }
            let fname = comps[1].as_os_str().to_string_lossy();
            crate::storage::safe_filename::ensure_safe_filename(&fname)
                .map_err(|e| anyhow!("unsafe filename '{}': {}", fname, e))?;
        }
        _ => return Err(anyhow!("path too deep: {:?}", rel)),
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
        let manifest = pack_skill_dir(
            &src,
            &dest,
            "my-skill",
            "My Skill",
            "0.1.0",
            Some("alice@example.com"),
            "skill-smith@test",
        )
        .unwrap();
        assert!(dest.exists());
        assert_eq!(manifest.id, "my-skill");
        assert_eq!(manifest.format_version, FORMAT_VERSION);
        assert!(!manifest.checksum_sha256.is_empty());

        // unpack
        let unpack_root = tmp.path().join("unpack");
        let res = unpack_skill_archive(&dest, &unpack_root).unwrap();
        assert_eq!(res.manifest.id, "my-skill");
        assert_eq!(res.manifest.checksum_sha256, manifest.checksum_sha256);
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
        let err = pack_skill_dir(&src, &dest, "x", "X", "0.1.0", None, "test").unwrap_err();
        assert!(err.to_string().contains("SKILL.md missing"));
    }

    #[test]
    fn unpack_rejects_zip_slip() {
        // 手工造一个 zip，含路径 ../../etc/passwd
        let tmp = TempDir::new().unwrap();
        let zip_path = tmp.path().join("evil.aijia-skill");
        {
            let f = fs::File::create(&zip_path).unwrap();
            let mut z = ZipWriter::new(f);
            let opts: FileOptions<'_, ()> = FileOptions::default();
            z.start_file("manifest.json", opts).unwrap();
            z.write_all(
                br#"{"format_version":1,"id":"x","name":"X","version":"0.1.0","created_at":"2026-01-01T00:00:00Z","checksum_sha256":""}"#,
            )
            .unwrap();
            z.start_file("../../etc/passwd", opts).unwrap();
            z.write_all(b"evil").unwrap();
            z.finish().unwrap();
        }
        let err = unpack_skill_archive(&zip_path, &tmp.path().join("out")).unwrap_err();
        assert!(err.to_string().contains("outside skill/") || err.to_string().contains("unsafe"));
    }

    #[test]
    fn unpack_rejects_disallowed_subdir() {
        let tmp = TempDir::new().unwrap();
        let zip_path = tmp.path().join("evil.aijia-skill");
        {
            let f = fs::File::create(&zip_path).unwrap();
            let mut z = ZipWriter::new(f);
            let opts: FileOptions<'_, ()> = FileOptions::default();
            z.start_file("manifest.json", opts).unwrap();
            z.write_all(
                br#"{"format_version":1,"id":"x","name":"X","version":"0.1.0","created_at":"2026-01-01T00:00:00Z","checksum_sha256":""}"#,
            )
            .unwrap();
            z.start_file("skill/SKILL.md", opts).unwrap();
            z.write_all(b"---\nname: x\ndescription: x\n---\n").unwrap();
            z.start_file("skill/secret/password.txt", opts).unwrap();
            z.write_all(b"hi").unwrap();
            z.finish().unwrap();
        }
        let err = unpack_skill_archive(&zip_path, &tmp.path().join("out")).unwrap_err();
        assert!(err.to_string().contains("not allowed"));
    }

    #[test]
    fn unpack_rejects_checksum_mismatch() {
        let tmp = TempDir::new().unwrap();
        let zip_path = tmp.path().join("bad.aijia-skill");
        {
            let f = fs::File::create(&zip_path).unwrap();
            let mut z = ZipWriter::new(f);
            let opts: FileOptions<'_, ()> = FileOptions::default();
            z.start_file("manifest.json", opts).unwrap();
            z.write_all(
                br#"{"format_version":1,"id":"x","name":"X","version":"0.1.0","created_at":"2026-01-01T00:00:00Z","checksum_sha256":"deadbeef"}"#,
            )
            .unwrap();
            z.start_file("skill/SKILL.md", opts).unwrap();
            z.write_all(b"---\nname: x\ndescription: x\n---\n").unwrap();
            z.finish().unwrap();
        }
        let err = unpack_skill_archive(&zip_path, &tmp.path().join("out")).unwrap_err();
        assert!(err.to_string().contains("checksum"));
    }

    #[test]
    fn unpack_rejects_id_name_mismatch() {
        let tmp = TempDir::new().unwrap();
        let src = make_skill_dir(tmp.path());
        let dest = tmp.path().join("good.aijia-skill");
        // pack first, then surgically rewrite manifest.id
        pack_skill_dir(&src, &dest, "my-skill", "X", "0.1.0", None, "t").unwrap();
        // surgery: re-pack with mismatched id
        let bad_dest = tmp.path().join("bad.aijia-skill");
        let mut original = ZipArchive::new(fs::File::open(&dest).unwrap()).unwrap();
        let f = fs::File::create(&bad_dest).unwrap();
        let mut z = ZipWriter::new(f);
        let opts: FileOptions<'_, ()> = FileOptions::default();
        for i in 0..original.len() {
            let mut e = original.by_index(i).unwrap();
            let name = e.name().to_string();
            let mut buf = vec![];
            e.read_to_end(&mut buf).unwrap();
            if name == "manifest.json" {
                let mut m: PackageManifest = serde_json::from_slice(&buf).unwrap();
                m.id = "different-id".to_string(); // mismatch
                buf = serde_json::to_vec_pretty(&m).unwrap();
            }
            z.start_file(name, opts).unwrap();
            z.write_all(&buf).unwrap();
        }
        z.finish().unwrap();
        let err = unpack_skill_archive(&bad_dest, &tmp.path().join("out")).unwrap_err();
        assert!(err.to_string().contains("manifest.id") || err.to_string().contains("frontmatter"));
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
