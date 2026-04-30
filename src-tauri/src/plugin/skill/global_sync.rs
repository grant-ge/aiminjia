use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::plugin::skill::frontmatter::parse_skill_md;
use crate::plugin::skill::loader::{is_valid_skill_id, load_skill_roots};
use crate::plugin::skill::registry::SkillRegistry;
use crate::runtime::dependencies::verify_sha256;

const MAX_EXTRACTED_BYTES: u64 = 50 * 1024 * 1024;
const MANIFEST_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15);
const ARTIFACT_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Deserialize)]
pub struct SkillPackageItem {
    pub id: u64,
    pub plugin_id: String,
    pub name: String,
    pub version: String,
    pub package_url: String,
    #[serde(default)]
    pub package_size: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SkillListResponse {
    pub data: Vec<SkillPackageItem>,
    #[serde(default)]
    pub total: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DownloadResponseEnvelope {
    pub data: DownloadResponseData,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DownloadResponseData {
    pub url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSkillsState {
    #[serde(default)]
    pub installed: HashMap<String, String>, // plugin_id -> version
    #[serde(default)]
    pub updated_at_unix_seconds: u64,
}

impl GlobalSkillsState {
    pub fn from_global_state_json(input: &str) -> Result<Option<Self>> {
        let value: Value = serde_json::from_str(input).context("parse global state json")?;
        match value.get("globalSkills") {
            Some(global_skills) => {
                // Best-effort parse; if old single-bundle format, return None to trigger full re-install.
                match serde_json::from_value::<GlobalSkillsState>(global_skills.clone()) {
                    Ok(state) if !state.installed.is_empty() => Ok(Some(state)),
                    _ => Ok(None),
                }
            }
            None => Ok(None),
        }
    }
}

pub fn should_persist_success_state(report: &GlobalSkillInstallReport) -> bool {
    !report.installed.is_empty() && report.skipped.is_empty()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GlobalSkillInstallReport {
    pub installed: Vec<String>,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GlobalSkillSyncConfig {
    pub manifest_url: String,
    pub state_path: PathBuf,
    pub downloads_dir: PathBuf,
    pub prepared_dir: PathBuf,
    pub global_skills_dir: PathBuf,
    pub skill_roots_for_reload: Vec<PathBuf>,
}

impl GlobalSkillSyncConfig {
    pub fn for_home(root: &Path, skill_roots_for_reload: Vec<PathBuf>) -> Self {
        let global_dir = root.join("global");
        Self {
            manifest_url: configured_global_skills_manifest_url(),
            state_path: global_dir.join("state.json"),
            downloads_dir: global_dir.join("downloads").join("skills"),
            prepared_dir: global_dir.join("prepared").join("skills"),
            global_skills_dir: root.join("skills"),
            skill_roots_for_reload,
        }
    }
}

pub fn install_prepared_global_skills(
    prepared_root: &Path,
    global_skills_dir: &Path,
) -> Result<GlobalSkillInstallReport> {
    fs::create_dir_all(global_skills_dir)
        .with_context(|| format!("create global skills dir '{}'", global_skills_dir.display()))?;

    let mut report = GlobalSkillInstallReport::default();
    if !prepared_root.is_dir() {
        return Ok(report);
    }

    for entry in fs::read_dir(prepared_root)
        .with_context(|| format!("read prepared root '{}'", prepared_root.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let skill_id = name.to_string();
        let file_type = entry.file_type()?;
        if name.starts_with('.')
            || name.starts_with('_')
            || !is_valid_skill_id(name)
            || !file_type.is_dir()
        {
            report.skipped.push(skill_id);
            continue;
        }

        if install_one_prepared_skill(&path, global_skills_dir, name).is_ok() {
            report.installed.push(skill_id);
        } else {
            report.skipped.push(skill_id);
        }
    }

    report.installed.sort();
    report.skipped.sort();
    Ok(report)
}

fn install_one_prepared_skill(
    source: &Path,
    global_skills_dir: &Path,
    skill_id: &str,
) -> Result<()> {
    reject_symlink_tree(source).with_context(|| format!("reject symlink skill '{}'", skill_id))?;
    validate_skill_md(source).with_context(|| format!("validate skill '{}'", skill_id))?;

    let unique = unique_suffix();
    let staging = global_skills_dir.join(format!(".{skill_id}.staging.{unique}"));
    let backup = global_skills_dir.join(format!(".{skill_id}.backup.{unique}"));
    let target = global_skills_dir.join(skill_id);

    if staging.exists() {
        fs::remove_dir_all(&staging)
            .with_context(|| format!("remove stale staging '{}'", staging.display()))?;
    }
    if backup.exists() {
        fs::remove_dir_all(&backup)
            .with_context(|| format!("remove stale backup '{}'", backup.display()))?;
    }

    if let Err(error) = copy_dir_without_symlinks(source, &staging) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error).context("stage skill directory");
    }
    if let Err(error) = validate_skill_md(&staging) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error).context("validate staged skill");
    }

    let mut moved_to_backup = false;
    if target.exists() {
        fs::rename(&target, &backup)
            .with_context(|| format!("backup existing skill '{}'", target.display()))?;
        moved_to_backup = true;
    }

    if let Err(error) = fs::rename(&staging, &target) {
        if moved_to_backup {
            let _ = fs::rename(&backup, &target);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(error).with_context(|| format!("install staged skill '{}'", target.display()));
    }

    if moved_to_backup {
        let _ = fs::remove_dir_all(&backup);
    }
    Ok(())
}

fn validate_skill_md(skill_dir: &Path) -> Result<()> {
    let skill_md = skill_dir.join("SKILL.md");
    let content = fs::read_to_string(&skill_md)
        .with_context(|| format!("read SKILL.md '{}'", skill_md.display()))?;
    parse_skill_md(&content).map(|_| ())
}

fn reject_symlink_tree(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("read metadata '{}'", path.display()))?;
    if metadata.file_type().is_symlink() {
        bail!("symlink is not allowed: {}", path.display());
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path).with_context(|| format!("read dir '{}'", path.display()))? {
            reject_symlink_tree(&entry?.path())?;
        }
    }
    Ok(())
}

fn copy_dir_without_symlinks(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("read metadata '{}'", source.display()))?;
    if metadata.file_type().is_symlink() {
        bail!("symlink is not allowed: {}", source.display());
    }
    if metadata.is_dir() {
        fs::create_dir_all(destination)
            .with_context(|| format!("create dir '{}'", destination.display()))?;
        for entry in
            fs::read_dir(source).with_context(|| format!("read dir '{}'", source.display()))?
        {
            let entry = entry?;
            copy_dir_without_symlinks(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else if metadata.is_file() {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create parent dir '{}'", parent.display()))?;
        }
        fs::copy(source, destination).with_context(|| {
            format!(
                "copy file '{}' to '{}'",
                source.display(),
                destination.display()
            )
        })?;
    } else {
        bail!("unsupported filesystem entry: {}", source.display());
    }
    Ok(())
}

pub fn extract_global_skills_zip(zip_path: &Path, output_dir: &Path) -> Result<()> {
    if output_dir.exists() {
        fs::remove_dir_all(output_dir)
            .with_context(|| format!("clear output dir '{}'", output_dir.display()))?;
    }
    fs::create_dir_all(output_dir)
        .with_context(|| format!("create output dir '{}'", output_dir.display()))?;

    let file =
        fs::File::open(zip_path).with_context(|| format!("open zip '{}'", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file).context("open global skills zip")?;
    let mut total_size = 0_u64;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).context("read zip entry")?;
        let Some(enclosed_name) = entry.enclosed_name() else {
            bail!("unsafe zip entry: {}", entry.name());
        };
        if is_zip_symlink(&entry) {
            bail!("unsafe zip entry symlink: {}", entry.name());
        }
        total_size = total_size
            .checked_add(entry.size())
            .ok_or_else(|| anyhow!("global skills zip size overflow"))?;
        if total_size > MAX_EXTRACTED_BYTES {
            bail!("global skills zip exceeds 50MB extraction limit");
        }

        let out_path = output_dir.join(enclosed_name);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)
                .with_context(|| format!("create zip dir '{}'", out_path.display()))?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create zip parent '{}'", parent.display()))?;
        }
        let mut output = fs::File::create(&out_path)
            .with_context(|| format!("create extracted file '{}'", out_path.display()))?;
        let copied = io::copy(&mut entry, &mut output)
            .with_context(|| format!("extract zip file '{}'", out_path.display()))?;
        if copied != entry.size() {
            bail!("zip entry size changed while extracting: {}", entry.name());
        }
    }

    Ok(())
}

fn is_zip_symlink(entry: &zip::read::ZipFile<'_>) -> bool {
    entry
        .unix_mode()
        .map(|mode| (mode & 0o170000) == 0o120000)
        .unwrap_or(false)
}

pub async fn sync_global_skills_from_manifest(
    config: GlobalSkillSyncConfig,
) -> Result<GlobalSkillInstallReport> {
    let manifest_text = download_text(&config.manifest_url).await?;
    let manifest = GlobalSkillsManifest::from_json(&manifest_text)?;
    let existing_state = read_global_skills_state(&config.state_path)?;
    if should_skip_manifest(existing_state.as_ref(), &manifest) {
        return Ok(GlobalSkillInstallReport::default());
    }

    fs::create_dir_all(&config.downloads_dir)
        .with_context(|| format!("create downloads dir '{}'", config.downloads_dir.display()))?;
    let archive_path = config.downloads_dir.join(format!(
        "global-skills-{}.zip",
        sanitize_filename(&manifest.bundle_version)
    ));
    download_file(&manifest.artifact.url, &archive_path).await?;

    let actual_size = fs::metadata(&archive_path)
        .with_context(|| format!("stat downloaded artifact '{}'", archive_path.display()))?
        .len();
    if actual_size != manifest.artifact.size_bytes {
        bail!(
            "global skills artifact size mismatch: expected {}, got {}",
            manifest.artifact.size_bytes,
            actual_size
        );
    }
    verify_sha256(&archive_path, &manifest.artifact.sha256)
        .map_err(|error| anyhow!("global skills artifact sha256 verification failed: {error}"))?;

    extract_global_skills_zip(&archive_path, &config.prepared_dir)?;
    let report = install_prepared_global_skills(&config.prepared_dir, &config.global_skills_dir)?;
    if !should_persist_success_state(&report) {
        bail!(
            "global skills artifact installed no valid skills; skipped: {:?}",
            report.skipped
        );
    }
    write_global_skills_state(
        &config.state_path,
        &GlobalSkillsState::from_manifest(&manifest),
    )?;
    Ok(report)
}

pub fn spawn_global_skill_sync(
    config: GlobalSkillSyncConfig,
    registry: Arc<Mutex<SkillRegistry>>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let skill_roots_for_reload = config.skill_roots_for_reload.clone();
        match sync_global_skills_from_manifest(config).await {
            Ok(report) => {
                if report.installed.is_empty() {
                    return;
                }
                match load_skill_roots(&skill_roots_for_reload) {
                    Ok(skills) => match registry.lock() {
                        Ok(mut guard) => {
                            *guard = SkillRegistry::from_skills(skills.into_values().collect());
                        }
                        Err(error) => {
                            log::warn!("[global-skill-sync] registry lock poisoned: {}", error);
                        }
                    },
                    Err(error) => {
                        log::warn!("[global-skill-sync] reload skill roots failed: {}", error);
                    }
                }
            }
            Err(error) => {
                log::warn!("[global-skill-sync] sync failed: {}", error);
            }
        }
    })
}

pub fn read_global_skills_state(state_path: &Path) -> Result<Option<GlobalSkillsState>> {
    if !state_path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(state_path)
        .with_context(|| format!("read state json '{}'", state_path.display()))?;
    GlobalSkillsState::from_global_state_json(&text)
}

pub fn write_global_skills_state(state_path: &Path, state: &GlobalSkillsState) -> Result<()> {
    crate::storage::migration::update_state_json(state_path, |value| {
        value["globalSkills"] =
            serde_json::to_value(state).expect("serialize globalSkills state should not fail");
    })
    .map_err(|error| anyhow!("write global skills state: {error}"))
}

async fn download_text(url: &str) -> Result<String> {
    let response = reqwest::Client::new()
        .get(url)
        .timeout(MANIFEST_DOWNLOAD_TIMEOUT)
        .send()
        .await
        .with_context(|| format!("download manifest '{}'", url))?
        .error_for_status()
        .with_context(|| format!("download manifest status '{}'", url))?;
    response
        .text()
        .await
        .with_context(|| format!("read manifest body '{}'", url))
}

async fn download_file(url: &str, path: &Path) -> Result<()> {
    let response = reqwest::Client::new()
        .get(url)
        .timeout(ARTIFACT_DOWNLOAD_TIMEOUT)
        .send()
        .await
        .with_context(|| format!("download artifact '{}'", url))?
        .error_for_status()
        .with_context(|| format!("download artifact status '{}'", url))?;
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("read artifact body '{}'", url))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create artifact parent '{}'", parent.display()))?;
    }
    let tmp = path.with_extension("zip.tmp");
    fs::write(&tmp, &bytes).with_context(|| format!("write artifact temp '{}'", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("replace artifact '{}'", path.display()))?;
    Ok(())
}

fn sanitize_filename(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn unique_suffix() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!("{}.{}", std::process::id(), millis)
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}
