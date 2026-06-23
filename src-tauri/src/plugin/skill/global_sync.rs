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
use crate::plugin::skill::loader::is_valid_skill_id;
use crate::plugin::skill::registry::SkillRegistry;
use crate::plugin::skill::required_builtin::is_required_builtin_skill;

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
    /// `"tenant"` for caller's tenant-private skills, `"public"` for platform.
    /// Gateway always returns this field (model.SkillPackage.Scope, default
    /// `"tenant"`). Used only for telemetry today — future change can store
    /// this per-skill so loader can tag SkillSource::Tenant vs Global.
    #[serde(default)]
    pub scope: String,
    /// Category as recorded in lotus DB (e.g. "hr"/"finance"/"legal"). Used
    /// as a sidecar fallback when the package's SKILL.md frontmatter is
    /// missing `category:`. Empty string is normalized to None.
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_string")]
    pub category: Option<String>,
    #[serde(default)]
    pub display_i18n: Option<Value>,
}

fn deserialize_optional_nonempty_string<'de, D>(
    d: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let raw: Option<String> = Option::deserialize(d)?;
    Ok(raw.and_then(|s| if s.trim().is_empty() { None } else { Some(s) }))
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
    /// Category as recorded in lotus DB. Gateway omits this field for rows
    /// where category is empty (legacy). Empty string normalized to None.
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_string")]
    pub category: Option<String>,
    #[serde(default)]
    pub display_i18n: Option<Value>,
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GlobalSkillInstallReport {
    pub installed: Vec<String>,
    pub updated: Vec<String>,
    pub skipped: Vec<String>,
    pub changed: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GlobalSkillSyncConfig {
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
            report.changed.push(name.to_string());
        } else {
            report.skipped.push(skill_id);
        }
    }

    report.installed.sort();
    report.updated.sort();
    report.skipped.sort();
    report.changed.sort();
    Ok(report)
}

pub(crate) fn install_one_prepared_skill(
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

/// Find a single-level subdirectory that contains SKILL.md directly.
/// Mirrors lotus tenant-portal `zipContainsSkillMd`: matches any first-level
/// folder, not just one named exactly after plugin_id. Returns `None` if no
/// such subdir exists (caller treats that as a missing-SKILL.md error).
pub(crate) fn find_single_level_skill_root(prepared: &Path) -> Result<Option<std::path::PathBuf>> {
    let entries = fs::read_dir(prepared)
        .with_context(|| format!("read prepared dir '{}'", prepared.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("iterate '{}'", prepared.display()))?;
        let path = entry.path();
        if path.is_dir() && path.join("SKILL.md").is_file() {
            return Ok(Some(path));
        }
    }
    Ok(None)
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

async fn fetch_skill_list(
    client: &reqwest::Client,
    server_base_url: &str,
    session_key: &str,
) -> Result<SkillListResponse> {
    // No scope filter: gateway returns BOTH the caller's tenant private
    // skills AND scope=public platform skills in one response (see lotus
    // api-gateway handler.SkillPackageEmployeeHandler.List). Keeping
    // scope=public would silently drop tenant-pushed skills, which was the
    // pre-2026-05-19 bug.
    let url = format!(
        "{}/v1/skill-packages?page=1&size=100",
        server_base_url.trim_end_matches('/')
    );
    let response = client
        .get(&url)
        .bearer_auth(session_key)
        .timeout(MANIFEST_DOWNLOAD_TIMEOUT)
        .send()
        .await
        .with_context(|| format!("fetch skill list '{}'", url))?
        .error_for_status()
        .with_context(|| format!("skill list status '{}'", url))?;
    let body: SkillListResponse = response
        .json()
        .await
        .with_context(|| format!("parse skill list body '{}'", url))?;
    Ok(body)
}

async fn fetch_download_url(
    client: &reqwest::Client,
    server_base_url: &str,
    session_key: &str,
    skill_id: u64,
) -> Result<DownloadResponseData> {
    let url = format!(
        "{}/v1/skill-packages/{}/download",
        server_base_url.trim_end_matches('/'),
        skill_id
    );
    let response = client
        .post(&url)
        .bearer_auth(session_key)
        .timeout(MANIFEST_DOWNLOAD_TIMEOUT)
        .send()
        .await
        .with_context(|| format!("fetch download url '{}'", url))?
        .error_for_status()
        .with_context(|| format!("download url status '{}'", url))?;
    let body: DownloadResponseEnvelope = response
        .json()
        .await
        .with_context(|| format!("parse download url body '{}'", url))?;
    if body.data.url.is_empty() {
        bail!("server returned empty download url for skill {}", skill_id);
    }
    Ok(body.data)
}

async fn install_one_skill_package(
    client: &reqwest::Client,
    server_base_url: &str,
    session_key: &str,
    item: &SkillPackageItem,
    config: &GlobalSkillSyncConfig,
) -> Result<()> {
    if !is_valid_skill_id(&item.plugin_id) {
        bail!("invalid skill id from server: {}", item.plugin_id);
    }

    // 1. Get a fresh signed download URL from server. The response also
    //    carries the DB-recorded category (when set), used below as a
    //    sidecar fallback for SKILL.md frontmatter missing `category:`.
    let download = fetch_download_url(client, server_base_url, session_key, item.id).await?;
    let download_url = &download.url;
    // Prefer download.category (fresher), fall back to item.category from
    // the list endpoint when download response omits it.
    let db_category = download.category.clone().or_else(|| item.category.clone());
    let display_i18n = download
        .display_i18n
        .clone()
        .or_else(|| item.display_i18n.clone())
        .filter(|value| !value.is_null());

    // 2. Download zip into downloads/{plugin_id}-{version}.zip
    let archive = config.downloads_dir.join(format!(
        "{}-{}.zip",
        sanitize_filename(&item.plugin_id),
        sanitize_filename(&item.version)
    ));
    download_file(download_url, &archive).await?;

    // 3. Extract into prepared/{plugin_id}/  (clears any prior content)
    let prepared = config.prepared_dir.join(&item.plugin_id);
    extract_global_skills_zip(&archive, &prepared)?;

    // 4. Locate SKILL.md (compatible with both layouts: flat or one-level subdir).
    //    Server-side validation (lotus tenant-portal zipContainsSkillMd) accepts
    //    SKILL.md at archive root OR under ANY single subdir; mirror that here.
    //    The previous logic only matched `{plugin_id}/SKILL.md` exactly, which
    //    failed when the zip's inner folder name didn't match plugin_id (e.g.
    //    user zipped as "skill/SKILL.md" but registered plugin_id="rehcm").
    let source = if prepared.join("SKILL.md").exists() {
        prepared.clone()
    } else if let Some(sub) = find_single_level_skill_root(&prepared)? {
        sub
    } else {
        bail!(
            "skill package '{}' v{} missing SKILL.md after extraction",
            item.plugin_id,
            item.version
        );
    };

    // 5. Atomically install into global_skills_dir/{plugin_id}/
    install_one_prepared_skill(&source, &config.global_skills_dir, &item.plugin_id)?;

    // 6. Write scope marker so the loader can tag the skill as Tenant vs Global.
    //    Gateway emits `"tenant"` for tenant-private skills and `"public"` for
    //    platform/OPS skills (default `"tenant"` for legacy rows without scope).
    let installed = config.global_skills_dir.join(&item.plugin_id);
    let scope_marker = if item.scope == "public" {
        "public"
    } else {
        "tenant"
    };
    let scope_path = installed.join(".scope");
    if let Err(error) = fs::write(&scope_path, scope_marker) {
        log::warn!(
            "[skill-sync] write .scope marker for '{}' failed: {}",
            item.plugin_id,
            error
        );
    }

    // 7. Write the lotus-side metadata sidecar (.lotus-meta.json). The loader
    //    overlays this onto the SKILL.md frontmatter when frontmatter fields
    //    are missing — primarily to give legacy packages (uploaded before the
    //    strict-frontmatter contract) a correct category instead of falling
    //    back to "general". Untouched SKILL.md keeps sha256 integrity intact.
    if db_category.is_some() || display_i18n.is_some() {
        let meta_path = installed.join(".lotus-meta.json");
        let mut payload = serde_json::Map::new();
        if let Some(cat) = db_category.as_deref() {
            payload.insert("category".to_string(), Value::String(cat.to_string()));
        }
        if let Some(display) = display_i18n {
            payload.insert("displayI18n".to_string(), display);
        }
        match serde_json::to_vec(&payload) {
            Ok(bytes) => {
                if let Err(error) = fs::write(&meta_path, bytes) {
                    log::warn!(
                        "[skill-sync] write .lotus-meta.json for '{}' failed: {}",
                        item.plugin_id,
                        error
                    );
                }
            }
            Err(error) => {
                log::warn!(
                    "[skill-sync] encode .lotus-meta.json for '{}' failed: {}",
                    item.plugin_id,
                    error
                );
            }
        }
    }
    Ok(())
}

fn is_global_skill_installed(global_skills_dir: &Path, skill_id: &str) -> bool {
    global_skills_dir.join(skill_id).join("SKILL.md").is_file()
}

fn prune_non_required_global_skills(
    state: &mut GlobalSkillsState,
    global_skills_dir: &Path,
) -> Result<Vec<String>> {
    let mut changed: HashSet<String> = HashSet::new();
    let tracked_ids: Vec<String> = state.installed.keys().cloned().collect();
    for skill_id in tracked_ids {
        if is_required_builtin_skill(&skill_id) {
            continue;
        }
        state.installed.remove(&skill_id);
        let target = global_skills_dir.join(&skill_id);
        if target.exists() {
            fs::remove_dir_all(&target).with_context(|| {
                format!("remove non-required global skill '{}'", target.display())
            })?;
        }
        changed.insert(skill_id);
    }

    if global_skills_dir.is_dir() {
        for entry in fs::read_dir(global_skills_dir)
            .with_context(|| format!("read global skills dir '{}'", global_skills_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let Some(skill_id) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !is_valid_skill_id(skill_id) || is_required_builtin_skill(skill_id) {
                continue;
            }
            // Only remove managed remote leftovers. Locally-created folders
            // without the sync sidecar are ignored here, and the loader still
            // keeps them out of the runtime catalog.
            if path.join(".scope").is_file() {
                fs::remove_dir_all(&path).with_context(|| {
                    format!(
                        "remove legacy marketplace global skill '{}'",
                        path.display()
                    )
                })?;
                changed.insert(skill_id.to_string());
            }
        }
    }

    let mut changed: Vec<String> = changed.into_iter().collect();
    changed.sort();
    Ok(changed)
}

pub fn prune_non_required_global_skill_installs(
    state_path: &Path,
    global_skills_dir: &Path,
) -> Result<Vec<String>> {
    let mut state = read_global_skills_state(state_path)?.unwrap_or_default();
    let changed = prune_non_required_global_skills(&mut state, global_skills_dir)?;
    if !changed.is_empty() {
        state.updated_at_unix_seconds = now_unix_seconds();
        write_global_skills_state(state_path, &state)?;
    }
    Ok(changed)
}

fn should_sync_remote_skill(item: &SkillPackageItem, local_state: &GlobalSkillsState) -> bool {
    is_required_builtin_skill(&item.plugin_id)
        || local_state.installed.contains_key(&item.plugin_id)
}

fn skill_package_scope_rank(item: &SkillPackageItem) -> u8 {
    if item.scope == "tenant" {
        2
    } else {
        1
    }
}

fn compare_version_text(a: &str, b: &str) -> std::cmp::Ordering {
    let left = a.trim();
    let right = b.trim();
    if left == right {
        return std::cmp::Ordering::Equal;
    }
    if left.is_empty() {
        return std::cmp::Ordering::Less;
    }
    if right.is_empty() {
        return std::cmp::Ordering::Greater;
    }

    let left_parts: Vec<&str> = left.split(['.', '_', '-']).collect();
    let right_parts: Vec<&str> = right.split(['.', '_', '-']).collect();
    let len = left_parts.len().max(right_parts.len());
    for idx in 0..len {
        let lp = left_parts.get(idx).copied().unwrap_or("0");
        let rp = right_parts.get(idx).copied().unwrap_or("0");
        match (lp.parse::<i64>(), rp.parse::<i64>()) {
            (Ok(ln), Ok(rn)) if ln != rn => return ln.cmp(&rn),
            _ if lp != rp => return lp.cmp(rp),
            _ => {}
        }
    }
    std::cmp::Ordering::Equal
}

fn should_replace_skill_package(current: &SkillPackageItem, candidate: &SkillPackageItem) -> bool {
    let scope_delta = skill_package_scope_rank(candidate).cmp(&skill_package_scope_rank(current));
    if scope_delta != std::cmp::Ordering::Equal {
        return scope_delta == std::cmp::Ordering::Greater;
    }

    let version_delta = compare_version_text(&candidate.version, &current.version);
    if version_delta != std::cmp::Ordering::Equal {
        return version_delta == std::cmp::Ordering::Greater;
    }

    candidate.id > current.id
}

fn dedupe_skill_packages(items: &[SkillPackageItem]) -> Vec<&SkillPackageItem> {
    let mut by_plugin_id: HashMap<&str, &SkillPackageItem> = HashMap::new();
    for item in items {
        match by_plugin_id.get(item.plugin_id.as_str()) {
            Some(current) if !should_replace_skill_package(current, item) => {}
            _ => {
                by_plugin_id.insert(item.plugin_id.as_str(), item);
            }
        }
    }
    let mut deduped: Vec<&SkillPackageItem> = by_plugin_id.into_values().collect();
    deduped.sort_by(|a, b| a.id.cmp(&b.id));
    deduped
}

pub async fn sync_skill_packages_from_server(
    config: GlobalSkillSyncConfig,
    server_base_url: String,
    session_key: String,
) -> Result<GlobalSkillInstallReport> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .context("build skill sync http client")?;

    log::info!("[skill-sync] start sync from {}", server_base_url);

    // 1. Fetch the published skill list from lotus-server
    let list = fetch_skill_list(&client, &server_base_url, &session_key).await?;
    let (n_tenant, n_public) = list.data.iter().fold((0usize, 0usize), |(t, p), it| {
        if it.scope == "tenant" {
            (t + 1, p)
        } else {
            (t, p + 1)
        }
    });
    log::info!(
        "[skill-sync] fetched {} skills from server ({} tenant + {} public): {:?}",
        list.data.len(),
        n_tenant,
        n_public,
        list.data
            .iter()
            .map(|i| format!("{}@{}({})", i.plugin_id, i.version, i.scope))
            .collect::<Vec<_>>()
    );

    // 2. Read local installed-version state (None if first run or schema mismatch)
    let mut local_state = read_global_skills_state(&config.state_path)?.unwrap_or_default();
    log::info!(
        "[skill-sync] local state has {} installed skills: {:?}",
        local_state.installed.len(),
        local_state.installed
    );

    let mut report = GlobalSkillInstallReport::default();
    report.changed.extend(prune_non_required_global_skills(
        &mut local_state,
        &config.global_skills_dir,
    )?);
    let mut new_installed: HashMap<String, String> = local_state.installed.clone();
    let remote_ids: HashSet<String> = list
        .data
        .iter()
        .map(|item| item.plugin_id.clone())
        .collect();
    let packages_to_sync = dedupe_skill_packages(&list.data);
    if packages_to_sync.len() != list.data.len() {
        log::info!(
            "[skill-sync] deduped server skill list from {} to {} plugin ids",
            list.data.len(),
            packages_to_sync.len()
        );
    }

    fs::create_dir_all(&config.downloads_dir)
        .with_context(|| format!("create downloads dir '{}'", config.downloads_dir.display()))?;
    fs::create_dir_all(&config.prepared_dir)
        .with_context(|| format!("create prepared dir '{}'", config.prepared_dir.display()))?;

    // 3. Install or update remote skills whose version changed (or are missing locally)
    for item in packages_to_sync {
        if !should_sync_remote_skill(item, &local_state) {
            report.skipped.push(item.plugin_id.clone());
            log::info!(
                "[skill-sync] skip '{}' v{} (marketplace package not installed locally)",
                item.plugin_id,
                item.version
            );
            continue;
        }

        let installed_on_disk =
            is_global_skill_installed(&config.global_skills_dir, &item.plugin_id);
        let was_installed =
            installed_on_disk || local_state.installed.contains_key(&item.plugin_id);
        let version_changed = local_state
            .installed
            .get(&item.plugin_id)
            .map_or(true, |v| v != &item.version);
        let need_install = !installed_on_disk || version_changed;
        if !need_install {
            report.skipped.push(item.plugin_id.clone());
            log::info!(
                "[skill-sync] skip '{}' v{} (already installed)",
                item.plugin_id,
                item.version
            );
            continue;
        }
        log::info!(
            "[skill-sync] installing '{}' v{} ...",
            item.plugin_id,
            item.version
        );
        match install_one_skill_package(&client, &server_base_url, &session_key, item, &config)
            .await
        {
            Ok(()) => {
                if was_installed {
                    report.updated.push(item.plugin_id.clone());
                } else {
                    report.installed.push(item.plugin_id.clone());
                }
                report.changed.push(item.plugin_id.clone());
                new_installed.insert(item.plugin_id.clone(), item.version.clone());
                log::info!(
                    "[skill-sync] installed/updated '{}' v{}",
                    item.plugin_id,
                    item.version
                );
            }
            Err(error) => {
                log::warn!(
                    "[skill-sync] install '{}' v{} failed: {}",
                    item.plugin_id,
                    item.version,
                    error
                );
                report.skipped.push(item.plugin_id.clone());
            }
        }
    }

    // 4. Uninstall any locally-tracked skill no longer present remotely
    let to_remove: Vec<String> = local_state
        .installed
        .keys()
        .filter(|name| !remote_ids.contains(*name))
        .cloned()
        .collect();
    for name in to_remove {
        let target = config.global_skills_dir.join(&name);
        if target.exists() {
            match fs::remove_dir_all(&target) {
                Ok(()) => {
                    new_installed.remove(&name);
                    report.changed.push(name.clone());
                    log::info!("[skill-sync] uninstalled '{}'", name);
                }
                Err(error) => {
                    log::warn!("[skill-sync] uninstall '{}' failed: {}", name, error);
                }
            }
        } else {
            new_installed.remove(&name);
            report.changed.push(name);
        }
    }

    // 5. Persist updated state
    let new_state = GlobalSkillsState {
        installed: new_installed,
        updated_at_unix_seconds: now_unix_seconds(),
    };
    write_global_skills_state(&config.state_path, &new_state)?;

    report.installed.sort();
    report.updated.sort();
    report.skipped.sort();
    report.changed.sort();
    report.changed.dedup();
    log::info!(
        "[skill-sync] done: installed={:?}, updated={:?}, skipped={:?}, changed={:?}",
        report.installed,
        report.updated,
        report.skipped,
        report.changed
    );
    Ok(report)
}

pub fn reload_skill_registry(skill_roots: &[PathBuf], registry: &Arc<Mutex<SkillRegistry>>) {
    let tagged: Vec<(PathBuf, crate::plugin::skill::types::SkillSource)> = match skill_roots {
        [] => Vec::new(),
        [global] => vec![(
            global.clone(),
            crate::plugin::skill::types::SkillSource::Global,
        )],
        roots => roots
            .iter()
            .enumerate()
            .map(|(idx, root)| {
                let source = if idx == 0 {
                    crate::plugin::skill::types::SkillSource::User
                } else {
                    crate::plugin::skill::types::SkillSource::Global
                };
                (root.clone(), source)
            })
            .collect(),
    };
    match crate::plugin::skill::loader::load_skill_roots_tagged(&tagged) {
        Ok(skills) => match registry.lock() {
            Ok(mut guard) => {
                *guard = SkillRegistry::from_skills(skills.into_values().collect());
            }
            Err(error) => {
                log::warn!("[skill-sync] registry lock poisoned: {}", error);
            }
        },
        Err(error) => {
            log::warn!("[skill-sync] reload skill roots failed: {}", error);
        }
    }
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
    if let Some(parent) = state_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create global skills state dir '{}'", parent.display()))?;
    }
    crate::storage::migration::update_state_json(state_path, |value| {
        value["globalSkills"] =
            serde_json::to_value(state).expect("serialize globalSkills state should not fail");
    })
    .map_err(|error| anyhow!("write global skills state: {error}"))
}

async fn download_file(url: &str, path: &Path) -> Result<()> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .context("build skill artifact http client")?;
    let response = client
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{Cursor, Write};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config(root: &Path) -> GlobalSkillSyncConfig {
        GlobalSkillSyncConfig {
            state_path: root.join("global").join("state.json"),
            downloads_dir: root.join("global").join("downloads").join("skills"),
            prepared_dir: root.join("global").join("prepared").join("skills"),
            global_skills_dir: root.join("skills"),
            skill_roots_for_reload: Vec::new(),
        }
    }

    fn skill_zip_bytes(skill_id: &str, version: &str) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(cursor);
        let options: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("SKILL.md", options).unwrap();
        write!(
            zip,
            "---\nname: {skill_id}\ndescription: {skill_id}\nversion: \"{version}\"\n---\nbody\n"
        )
        .unwrap();
        zip.finish().unwrap().into_inner()
    }

    fn write_managed_global_skill(config: &GlobalSkillSyncConfig, skill_id: &str, scope: &str) {
        let dir = config.global_skills_dir.join(skill_id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!(
                "---\nname: {skill_id}\ndescription: {skill_id}\nversion: \"1.0.0\"\n---\nbody\n"
            ),
        )
        .unwrap();
        fs::write(dir.join(".scope"), scope).unwrap();
    }

    async fn mock_skill_package(
        server: &MockServer,
        package_id: u64,
        skill_id: &str,
        version: &str,
    ) -> serde_json::Value {
        let artifact_path = format!("/artifacts/{skill_id}-{version}.zip");
        let artifact_url = format!("{}{}", server.uri(), artifact_path);
        Mock::given(method("POST"))
            .and(path(format!("/v1/skill-packages/{package_id}/download")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "url": artifact_url
                }
            })))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(artifact_path))
            .respond_with(
                ResponseTemplate::new(200).set_body_bytes(skill_zip_bytes(skill_id, version)),
            )
            .mount(server)
            .await;
        json!({
            "id": package_id,
            "plugin_id": skill_id,
            "name": skill_id,
            "version": version,
            "package_url": artifact_url,
            "package_size": 128,
            "scope": "public"
        })
    }

    async fn mock_skill_package_with_scope(
        server: &MockServer,
        package_id: u64,
        skill_id: &str,
        version: &str,
        scope: &str,
    ) -> serde_json::Value {
        let mut item = mock_skill_package(server, package_id, skill_id, version).await;
        item["scope"] = json!(scope);
        item
    }

    #[tokio::test]
    async fn first_login_installs_required_builtin_packages_only() {
        let tmp = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        let required = mock_skill_package(&server, 1, "dingtalk-workspace", "1.0.0").await;
        let market_only = mock_skill_package(&server, 2, "market-only", "1.0.0").await;

        Mock::given(method("GET"))
            .and(path("/v1/skill-packages"))
            .and(query_param("page", "1"))
            .and(query_param("size", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [required, market_only],
                "total": 2
            })))
            .mount(&server)
            .await;

        let config = test_config(tmp.path());
        let report = sync_skill_packages_from_server(
            config.clone(),
            server.uri(),
            "session-key".to_string(),
        )
        .await
        .unwrap();

        assert_eq!(report.installed, vec!["dingtalk-workspace".to_string()]);
        assert_eq!(report.skipped, vec!["market-only".to_string()]);
        assert!(config
            .global_skills_dir
            .join("dingtalk-workspace")
            .join("SKILL.md")
            .is_file());
        assert!(!config.global_skills_dir.join("market-only").exists());

        let state = read_global_skills_state(&config.state_path)
            .unwrap()
            .expect("state written");
        assert_eq!(
            state.installed.keys().cloned().collect::<Vec<_>>(),
            vec!["dingtalk-workspace".to_string()]
        );
    }

    #[tokio::test]
    async fn sync_dedupes_duplicate_packages_before_updating_required_builtins() {
        let tmp = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        let tenant_newer =
            mock_skill_package_with_scope(&server, 11, "dingtalk-workspace", "1.3.0", "tenant")
                .await;
        let public_older =
            mock_skill_package_with_scope(&server, 12, "dingtalk-workspace", "1.2.0", "public")
                .await;

        Mock::given(method("GET"))
            .and(path("/v1/skill-packages"))
            .and(query_param("page", "1"))
            .and(query_param("size", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [tenant_newer, public_older],
                "total": 2
            })))
            .mount(&server)
            .await;

        let config = test_config(tmp.path());
        write_managed_global_skill(&config, "dingtalk-workspace", "tenant");
        write_global_skills_state(
            &config.state_path,
            &GlobalSkillsState {
                installed: HashMap::from([("dingtalk-workspace".to_string(), "1.2.0".to_string())]),
                updated_at_unix_seconds: 1,
            },
        )
        .unwrap();

        let report = sync_skill_packages_from_server(
            config.clone(),
            server.uri(),
            "session-key".to_string(),
        )
        .await
        .unwrap();

        assert_eq!(report.updated, vec!["dingtalk-workspace".to_string()]);
        let state = read_global_skills_state(&config.state_path)
            .unwrap()
            .expect("state written");
        assert_eq!(
            state.installed.get("dingtalk-workspace"),
            Some(&"1.3.0".to_string())
        );
        let skill_md = fs::read_to_string(
            config
                .global_skills_dir
                .join("dingtalk-workspace")
                .join("SKILL.md"),
        )
        .unwrap();
        assert!(skill_md.contains("version: \"1.3.0\""));
    }

    #[tokio::test]
    async fn sync_prunes_legacy_non_required_global_skills() {
        let tmp = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        let required = mock_skill_package(&server, 1, "dingtalk-workspace", "1.0.0").await;
        let market_only = mock_skill_package(&server, 2, "market-only", "1.0.0").await;

        Mock::given(method("GET"))
            .and(path("/v1/skill-packages"))
            .and(query_param("page", "1"))
            .and(query_param("size", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [required, market_only],
                "total": 2
            })))
            .mount(&server)
            .await;

        let config = test_config(tmp.path());
        write_managed_global_skill(&config, "dingtalk-workspace", "tenant");
        write_managed_global_skill(&config, "market-only", "public");
        write_global_skills_state(
            &config.state_path,
            &GlobalSkillsState {
                installed: HashMap::from([
                    ("dingtalk-workspace".to_string(), "1.0.0".to_string()),
                    ("market-only".to_string(), "1.0.0".to_string()),
                ]),
                updated_at_unix_seconds: 1,
            },
        )
        .unwrap();

        let report = sync_skill_packages_from_server(
            config.clone(),
            server.uri(),
            "session-key".to_string(),
        )
        .await
        .unwrap();

        assert!(report.changed.contains(&"market-only".to_string()));
        assert!(!config.global_skills_dir.join("market-only").exists());
        assert!(config
            .global_skills_dir
            .join("dingtalk-workspace")
            .join("SKILL.md")
            .is_file());

        let state = read_global_skills_state(&config.state_path)
            .unwrap()
            .expect("state written");
        assert_eq!(
            state.installed.keys().cloned().collect::<Vec<_>>(),
            vec!["dingtalk-workspace".to_string()]
        );
    }
}
