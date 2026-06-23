use crate::plugin::skill::enablement::{SkillEnablementState, SkillEnablementStore};
use crate::plugin::skill::loader::is_valid_skill_id;
use crate::plugin::skill::registry::SkillRegistry;
use crate::storage::UserScopedPathResolver;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::AppHandle;
use tauri::Emitter;
use tauri::Manager;

/// Structured validation outcome for a skill directory. Surfaced to the
/// frontend as JSON via `InstallSkillError` so the UI can render a per-rule
/// checklist instead of parsing free-form strings.
#[derive(Debug)]
pub enum SkillValidationError {
    MissingSkillMd,
    ParseFailed(String),
    InvalidName(String),
}

/// Install-time error returned by the `install_custom_skill` command.
/// Serialized as `{ "kind": "...", "detail": "..." }` so the frontend can
/// match on `kind` directly. `AlreadyExists.detail` carries the conflicting
/// skill id; validation variants carry the underlying detail (path / parse
/// error / invalid name).
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "detail")]
pub enum InstallSkillError {
    MissingSkillMd,
    ParseFailed(String),
    InvalidName(String),
    AlreadyExists(String),
    Io(String),
}

impl InstallSkillError {
    fn from_validation(err: SkillValidationError) -> Self {
        match err {
            SkillValidationError::MissingSkillMd => Self::MissingSkillMd,
            SkillValidationError::ParseFailed(detail) => Self::ParseFailed(detail),
            SkillValidationError::InvalidName(name) => Self::InvalidName(name),
        }
    }
}

/// Pure function: copy `source` into `<custom_dir>/<basename>`. If the target
/// already exists and `force=false`, returns `AlreadyExists` without modifying
/// anything. Caller is responsible for running validation first.
pub fn install_custom_skill_to_dir_with_force(
    source: &std::path::Path,
    custom_dir: &std::path::Path,
    force: bool,
) -> Result<String, InstallSkillError> {
    let basename = source.file_name().ok_or_else(|| {
        InstallSkillError::Io(format!("Source '{}' has no basename", source.display()))
    })?;
    let dest = custom_dir.join(basename);
    if dest.exists() {
        if !force {
            return Err(InstallSkillError::AlreadyExists(
                basename.to_string_lossy().to_string(),
            ));
        }
        std::fs::remove_dir_all(&dest).map_err(|e| {
            InstallSkillError::Io(format!("Failed to remove existing skill: {}", e))
        })?;
    }
    copy_dir_recursive(source, &dest)
        .map_err(|e| InstallSkillError::Io(format!("Failed to copy skill: {}", e)))?;
    Ok(dest.to_string_lossy().to_string())
}

/// Validate that `source` is a well-formed skill directory the runtime loader
/// will actually pick up. Mirrors the rules in `loader::load_one_root` so an
/// upload that passes here is guaranteed to surface in `list_skills`.
pub fn validate_skill_directory(source: &std::path::Path) -> Result<(), SkillValidationError> {
    // Check directory basename matches is_valid_skill_id — same rule as loader.rs:52
    let basename = source.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if basename.starts_with('_') || basename.starts_with('.') || !is_valid_skill_id(basename) {
        return Err(SkillValidationError::InvalidName(basename.to_string()));
    }

    let skill_md = source.join("SKILL.md");
    if !skill_md.is_file() {
        return Err(SkillValidationError::MissingSkillMd);
    }
    let content = std::fs::read_to_string(&skill_md)
        .map_err(|e| SkillValidationError::ParseFailed(e.to_string()))?;

    crate::plugin::skill::frontmatter::parse_skill_md(&content)
        .map_err(|e| SkillValidationError::ParseFailed(e.to_string()))?;

    Ok(())
}

/// Skill info returned by `list_skills` IPC — only SKILL.md-backed skills.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    pub id: String,
    pub display_name: String,
    pub display_name_en: String,
    pub description: String,
    pub short_description_en: String,
    pub icon: Option<String>,
    pub category: Option<String>,
    /// "user"  — installed under ~/.renlijia/users/{scope}/skills/
    /// "global" — managed bundle in ~/.renlijia/skills/
    /// 用于前端区分"本地技能"分类。
    pub source: String,
    /// 技能"更新时间"。RFC 3339 UTC 字符串；读不到时为 None。
    /// 来源由 `plugin::skill::updated_at::SkillUpdatedAtResolver` 决定，
    /// 当前默认走 `DirMtimeResolver`（技能根目录 mtime）。
    pub updated_at: Option<String>,
    /// 来自 SKILL.md frontmatter 的 `version:` 字段。前端把它作为
    /// chip 显示在卡片标题旁。读不到时为 None。
    pub version: Option<String>,
    /// Whether the skill is enabled for the current logged-in user.
    /// Disabled skills remain visible in management views but are filtered from
    /// chat entrypoints and runtime catalog.
    pub enabled: bool,
}

/// Pure function for testability: list all skills in the new disk-backed registry.
pub fn list_skills_from_registry(registry: &Arc<Mutex<SkillRegistry>>) -> Vec<SkillInfo> {
    use crate::plugin::skill::updated_at::DirMtimeResolver;
    list_skills_from_registry_with_resolver(
        registry,
        &DirMtimeResolver,
        &SkillEnablementState::default(),
    )
}

pub fn list_skills_from_registry_with_enablement(
    registry: &Arc<Mutex<SkillRegistry>>,
    enablement: &SkillEnablementState,
) -> Vec<SkillInfo> {
    use crate::plugin::skill::updated_at::DirMtimeResolver;
    list_skills_from_registry_with_resolver(registry, &DirMtimeResolver, enablement)
}

/// 同上，但允许调用方注入自定义的 `SkillUpdatedAtResolver`，用于单测或
/// 后续切换更新时间来源。
pub fn list_skills_from_registry_with_resolver(
    registry: &Arc<Mutex<SkillRegistry>>,
    resolver: &dyn crate::plugin::skill::updated_at::SkillUpdatedAtResolver,
    enablement: &SkillEnablementState,
) -> Vec<SkillInfo> {
    let guard = registry.lock().unwrap();
    guard
        .skill_ids()
        .into_iter()
        .filter_map(|id| {
            guard.get(&id).map(|skill| {
                let english_display = skill
                    .frontmatter
                    .metadata
                    .display_i18n
                    .get("en-US")
                    .cloned()
                    .unwrap_or_default();
                SkillInfo {
                    id: skill.id.clone(),
                    display_name: skill
                        .frontmatter
                        .metadata
                        .label
                        .clone()
                        .unwrap_or_else(|| skill.frontmatter.name.clone()),
                    display_name_en: english_display.name.unwrap_or_default(),
                    description: skill.frontmatter.description.clone(),
                    short_description_en: english_display.description.unwrap_or_default(),
                    icon: None,
                    category: skill.frontmatter.category.clone(),
                    source: match skill.source {
                        crate::plugin::skill::types::SkillSource::User => "user".to_string(),
                        crate::plugin::skill::types::SkillSource::Tenant => "tenant".to_string(),
                        crate::plugin::skill::types::SkillSource::Global => "global".to_string(),
                    },
                    updated_at: resolver.resolve(skill),
                    version: skill.frontmatter.version.clone(),
                    enabled: enablement.is_enabled(&skill.id),
                }
            })
        })
        .collect()
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillEnablementChangedPayload {
    pub skill_id: String,
    pub enabled: bool,
}

pub fn set_skill_enabled_for_registry(
    registry: &Arc<Mutex<SkillRegistry>>,
    enablement_store: &SkillEnablementStore,
    skill_id: &str,
    enabled: bool,
    refresh_registry: impl FnOnce() -> Result<(), String>,
) -> Result<SkillInfo, String> {
    let exists = registry
        .lock()
        .map_err(|e| format!("registry lock poisoned: {}", e))?
        .get(skill_id)
        .is_some();

    if !exists {
        refresh_registry()?;
        let exists_after_refresh = registry
            .lock()
            .map_err(|e| format!("registry lock poisoned: {}", e))?
            .get(skill_id)
            .is_some();
        if !exists_after_refresh {
            return Err(format!("Unknown skill: {}", skill_id));
        }
    }

    let enablement = enablement_store
        .set_enabled(skill_id, enabled)
        .map_err(|e| e.to_string())?;

    list_skills_from_registry_with_enablement(registry, &enablement)
        .into_iter()
        .find(|skill| skill.id == skill_id)
        .ok_or_else(|| format!("Unknown skill: {}", skill_id))
}

#[tauri::command]
pub async fn set_skill_enabled(
    app: AppHandle,
    registry: tauri::State<'_, Arc<Mutex<SkillRegistry>>>,
    enablement_store: tauri::State<'_, Arc<SkillEnablementStore>>,
    skill_id: String,
    enabled: bool,
) -> Result<SkillInfo, String> {
    let skill = set_skill_enabled_for_registry(
        registry.inner(),
        enablement_store.inner().as_ref(),
        &skill_id,
        enabled,
        || refresh_skill_registry(&app),
    )?;

    let _ = app.emit(
        "skill:enablement-changed",
        SkillEnablementChangedPayload { skill_id, enabled },
    );
    Ok(skill)
}

#[derive(serde::Serialize)]
pub struct CustomSkillInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub path: String,
    pub enabled: bool,
    /// Optional `version:` from SKILL.md frontmatter; surfaced to UI as a badge.
    #[serde(default)]
    pub version: Option<String>,
}

fn list_custom_skills_in_dir(custom_dir: &Path) -> Result<Vec<CustomSkillInfo>, String> {
    if !custom_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut skills = Vec::new();
    for entry in std::fs::read_dir(custom_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() && path.join("SKILL.md").is_file() {
            let id = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() {
                continue;
            }
            // Best-effort parse — malformed SKILL.md should not hide the skill from the list.
            let (mut name, mut description, mut version) = (id.clone(), String::new(), None);
            if let Ok(content) = std::fs::read_to_string(path.join("SKILL.md")) {
                if let Ok(parsed) = crate::plugin::skill::frontmatter::parse_skill_md(&content) {
                    let fm = parsed.frontmatter;
                    name = fm.metadata.label.unwrap_or(fm.name);
                    description = fm.description;
                    version = fm.version;
                }
            }
            skills.push(CustomSkillInfo {
                id: id.clone(),
                name,
                description,
                path: path.to_string_lossy().to_string(),
                enabled: true,
                version,
            });
        }
    }
    Ok(skills)
}

fn user_skills_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let cus = app.state::<Arc<crate::storage::CurrentUserStorage>>();
    Ok(cus.require_paths().map_err(|e| e.to_string())?.skills_dir())
}

fn clear_enablement_override_for_skill(app: &AppHandle, skill_id: &str) -> Result<(), String> {
    if let Some(store) = app.try_state::<Arc<SkillEnablementStore>>() {
        store.clear_override(skill_id).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Re-scan both [user_skills_dir, global_skills_dir] roots and replace the
/// in-memory `SkillRegistry`. Both roots are always scanned because user-root
/// skills shadow same-id global skills; a single-root scan would mis-resurrect
/// or hide skills after uninstall.
pub fn refresh_skill_registry(app: &AppHandle) -> Result<(), String> {
    use crate::plugin::skill::loader::load_skill_roots_tagged;
    use crate::plugin::skill::types::SkillSource;
    use crate::storage::AiJiaHome;

    let aijia_home = app.state::<Arc<AiJiaHome>>();
    let global_root = aijia_home.skills_dir();
    let user_root = user_skills_dir(app).ok();
    let roots: Vec<(PathBuf, SkillSource)> = match user_root {
        Some(user) => vec![
            (user, SkillSource::User),
            (global_root, SkillSource::Global),
        ],
        None => vec![(global_root, SkillSource::Global)],
    };

    let loaded =
        load_skill_roots_tagged(&roots).map_err(|e| format!("load_skill_roots failed: {}", e))?;
    let registry = app.state::<Arc<Mutex<SkillRegistry>>>();
    registry
        .lock()
        .map_err(|e| format!("registry lock poisoned: {}", e))?
        .replace_all(loaded.into_values().collect());
    // 通知前端 registry 已刷新，让各处缓存（SkillPopover picker / 技能中心 / 派活 banner 等）
    // 调用 useSkillStore.reload() 重新拉 list_skills。失败 silent — 事件发送失败不影响 refresh
    // 这个写盘本身的成功。
    let _ = app.emit("skill:registry-refreshed", ());
    Ok(())
}

/// Tauri command wrapper for `refresh_skill_registry`. Exposed so the
/// frontend (SkillCenterPage) and runtime tools (RefreshSkills) can
/// trigger a registry refresh without restarting the app.
#[tauri::command]
pub async fn refresh_skill_registry_cmd(app: AppHandle) -> Result<(), String> {
    refresh_skill_registry(&app)
}

/// List all installed custom skills.
#[tauri::command]
pub async fn list_custom_skills(app: AppHandle) -> Result<Vec<CustomSkillInfo>, String> {
    let custom_dir = user_skills_dir(&app)?;
    list_custom_skills_in_dir(&custom_dir)
}

/// Install a skill from a local path into the current user's skills dir.
///
/// Accepts:
/// - **MD file** (single-file SKILL.md): parses YAML frontmatter `name:` →
///   writes to `{custom_dir}/{name}/SKILL.md`.
/// - **Archive** (`.zip` / `.zip`): unpacked via
///   `skill_package::unpack_skill_archive` (50MB / 256 files / zip-slip
///   guarded) into a temp dir, then moved into `{custom_dir}/{skill_id}`.
/// - **Directory** (legacy): recursively copied as-is.
///
/// `force=false`: returns `AlreadyExists` if same-id skill exists.
/// `force=true`: overwrites existing skill.
/// On success: re-scans both user + global roots and refreshes registry.
#[tauri::command]
pub async fn install_custom_skill(
    app: AppHandle,
    source_path: String,
    force: Option<bool>,
) -> Result<String, InstallSkillError> {
    let source = PathBuf::from(&source_path);
    let custom_dir = user_skills_dir(&app).map_err(InstallSkillError::Io)?;
    std::fs::create_dir_all(&custom_dir).map_err(|e| InstallSkillError::Io(e.to_string()))?;

    let dest = if source.is_file() {
        let ext = source
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if ext == "md" {
            install_skill_md_file(&source, &custom_dir, force.unwrap_or(false))?
        } else {
            let tmp = archive_tmp_root(&app);
            let result = install_skill_archive(&source, &tmp, &custom_dir, force.unwrap_or(false));
            let _ = std::fs::remove_dir_all(&tmp);
            result?
        }
    } else if source.is_dir() {
        validate_skill_directory(&source).map_err(InstallSkillError::from_validation)?;
        install_custom_skill_to_dir_with_force(&source, &custom_dir, force.unwrap_or(false))?
    } else {
        return Err(InstallSkillError::Io(format!(
            "Source path '{}' is not a file or directory",
            source_path
        )));
    };

    if let Some(skill_id) = Path::new(&dest).file_name().and_then(|name| name.to_str()) {
        clear_enablement_override_for_skill(&app, skill_id).map_err(InstallSkillError::Io)?;
    }
    refresh_skill_registry(&app).map_err(InstallSkillError::Io)?;
    Ok(dest)
}

fn archive_tmp_root(app: &AppHandle) -> PathBuf {
    let home = app.state::<Arc<crate::storage::AiJiaHome>>();
    home.root()
        .join("tmp")
        .join(format!("skill-import-{}", uuid::Uuid::new_v4()))
}

/// Install a packaged skill archive (.zip). Caller cleans up
/// the temp dir regardless of outcome.
fn install_skill_archive(
    source: &Path,
    tmp_root: &Path,
    custom_dir: &Path,
    force: bool,
) -> Result<String, InstallSkillError> {
    let unpacked = crate::storage::skill_package::unpack_skill_archive(source, tmp_root)
        .map_err(|e| InstallSkillError::ParseFailed(e.to_string()))?;
    let skill_id = unpacked.skill_id;
    if skill_id.starts_with('_') || skill_id.starts_with('.') || !is_valid_skill_id(&skill_id) {
        return Err(InstallSkillError::InvalidName(skill_id));
    }
    validate_skill_directory(&unpacked.skill_dir).map_err(InstallSkillError::from_validation)?;
    let dest = custom_dir.join(&skill_id);
    if dest.exists() {
        if !force {
            return Err(InstallSkillError::AlreadyExists(skill_id));
        }
        std::fs::remove_dir_all(&dest)
            .map_err(|e| InstallSkillError::Io(format!("remove existing: {}", e)))?;
    }
    copy_dir_recursive(&unpacked.skill_dir, &dest)
        .map_err(|e| InstallSkillError::Io(format!("copy unpacked: {}", e)))?;
    Ok(dest.to_string_lossy().to_string())
}

/// Install a single-file SKILL.md. The skill id comes from the YAML `name:`
/// field, not the source filename — frontend can pass any `.md` path.
fn install_skill_md_file(
    source: &Path,
    custom_dir: &Path,
    force: bool,
) -> Result<String, InstallSkillError> {
    let content = std::fs::read_to_string(source)
        .map_err(|e| InstallSkillError::Io(format!("read md: {}", e)))?;
    let parsed = crate::plugin::skill::frontmatter::parse_skill_md(&content)
        .map_err(|e| InstallSkillError::ParseFailed(e.to_string()))?;
    let skill_id = parsed.frontmatter.name.trim().to_string();
    if skill_id.starts_with('_') || skill_id.starts_with('.') || !is_valid_skill_id(&skill_id) {
        return Err(InstallSkillError::InvalidName(skill_id));
    }
    let dest = custom_dir.join(&skill_id);
    if dest.exists() {
        if !force {
            return Err(InstallSkillError::AlreadyExists(skill_id));
        }
        std::fs::remove_dir_all(&dest)
            .map_err(|e| InstallSkillError::Io(format!("remove existing: {}", e)))?;
    }
    std::fs::create_dir_all(&dest).map_err(|e| InstallSkillError::Io(e.to_string()))?;
    std::fs::write(dest.join("SKILL.md"), &content)
        .map_err(|e| InstallSkillError::Io(format!("write SKILL.md: {}", e)))?;
    Ok(dest.to_string_lossy().to_string())
}

/// Uninstall a custom skill by ID.
///
/// Looks in both the per-user `user_skills_dir` and the legacy global
/// `~/.renlijia/skills/` root. Runtime skill availability is user-root
/// first; the global fallback exists to clear old OPS-synced orphans left
/// behind before marketplace and tenant skills became explicit installs.
///
/// Caveat: required platform builtins can still be recreated by
/// `sync_builtin_skills`.
#[tauri::command]
pub async fn uninstall_custom_skill(app: AppHandle, skill_id: String) -> Result<String, String> {
    let user_dir = user_skills_dir(&app)?.join(&skill_id);
    let global_dir = crate::storage::AiJiaHome::from_home()
        .skills_dir()
        .join(&skill_id);

    let target = if user_dir.exists() {
        user_dir
    } else if global_dir.exists() {
        global_dir
    } else {
        return Err(format!("Custom skill '{}' not found", skill_id));
    };

    std::fs::remove_dir_all(&target).map_err(|e| e.to_string())?;
    clear_enablement_override_for_skill(&app, &skill_id)?;
    refresh_skill_registry(&app)?;
    Ok(format!("Uninstalled skill '{}'", skill_id))
}

/// Create a new skill template directory with scaffolding files.
#[tauri::command]
pub async fn init_skill_template(
    target_dir: String,
    skill_id: String,
    _skill_name: String,
) -> Result<String, String> {
    let dir = PathBuf::from(&target_dir).join(&skill_id);
    if dir.exists() {
        return Err(format!("Directory '{}' already exists", dir.display()));
    }

    // Create directory structure
    std::fs::create_dir_all(dir.join("scripts")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("references")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("assets")).map_err(|e| e.to_string())?;

    // .gitkeep files for empty directories
    std::fs::write(dir.join("scripts/.gitkeep"), "").map_err(|e| e.to_string())?;
    std::fs::write(dir.join("references/.gitkeep"), "").map_err(|e| e.to_string())?;
    std::fs::write(dir.join("assets/.gitkeep"), "").map_err(|e| e.to_string())?;

    // SKILL.md (new manifest format, Chinese template)
    let skill_md = format!(
        r#"---
name: {skill_id}
description: 描述这个技能何时应该被使用。
---

# {skill_id}

说明如何完成这个技能支持的任务。

可用资源：
- ${{AIJIA_SKILL_DIR}}/scripts/
- ${{AIJIA_SKILL_DIR}}/references/
- ${{AIJIA_SKILL_DIR}}/assets/
"#
    );
    std::fs::write(dir.join("SKILL.md"), skill_md).map_err(|e| e.to_string())?;

    Ok(dir.to_string_lossy().to_string())
}

/// Export a skill directory to a user-chosen destination.
///
/// Output format depends on `dest_path` extension:
/// - `.md`  → only SKILL.md is copied (degenerate single-file export).
/// - other (recommended `.zip` / `.zip`) → full directory packed as
///   OPS-standard zip via `skill_package::pack_skill_dir`, preserving
///   `scripts/` / `references/` / `migration-notes.md` siblings.
#[tauri::command]
pub async fn pack_skill(skill_dir: String, dest_path: String) -> Result<String, String> {
    let src_dir = PathBuf::from(&skill_dir);
    let skill_md = src_dir.join("SKILL.md");
    if !skill_md.is_file() {
        return Err(format!("SKILL.md not found in '{}'", src_dir.display()));
    }
    let dest = PathBuf::from(&dest_path);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let is_md_only = dest
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("md"))
        .unwrap_or(false);
    if is_md_only {
        std::fs::copy(&skill_md, &dest).map_err(|e| e.to_string())?;
    } else {
        let skill_id = src_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("skill")
            .to_string();
        crate::storage::skill_package::pack_skill_dir(&src_dir, &dest, &skill_id)
            .map_err(|e| e.to_string())?;
    }
    Ok(dest.to_string_lossy().to_string())
}

// ---------------------------------------------------------------------------
// Marketplace Commands
// ---------------------------------------------------------------------------

/// Marketplace skill package returned from the API.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSkillItem {
    pub id: i64,
    #[serde(alias = "plugin_id")]
    pub plugin_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub downloads: i64,
    #[serde(default)]
    pub featured: bool,
    #[serde(default, alias = "package_size")]
    pub package_size: i64,
    #[serde(default, alias = "tenant_name")]
    pub tenant_name: String,
    #[serde(default, alias = "created_at")]
    pub created_at: String,
}

/// Paginated response from the marketplace API.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceResponse {
    pub items: Vec<MarketplaceSkillItem>,
    pub total: i64,
    pub page: i64,
    pub size: i64,
}

/// Full raw SKILL.md preview for an uninstalled marketplace package.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSkillPreview {
    pub raw_content: String,
}

fn parse_marketplace_response(
    body: serde_json::Value,
    requested_page: u32,
    requested_size: u32,
) -> Result<MarketplaceResponse, String> {
    let data = body
        .get("data")
        .ok_or_else(|| "No data in marketplace response".to_string())?;
    let (item_values, total, page, size) = if data.is_array() {
        let items = data.as_array().cloned().unwrap_or_default();
        let total = body
            .get("total")
            .and_then(|v| v.as_i64())
            .unwrap_or(items.len() as i64);
        (
            items,
            total,
            body.get("page")
                .and_then(|v| v.as_i64())
                .unwrap_or(requested_page as i64),
            body.get("size")
                .and_then(|v| v.as_i64())
                .unwrap_or(requested_size as i64),
        )
    } else {
        let items = data
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        (
            items,
            data.get("total").and_then(|v| v.as_i64()).unwrap_or(0),
            data.get("page")
                .and_then(|v| v.as_i64())
                .unwrap_or(requested_page as i64),
            data.get("size")
                .and_then(|v| v.as_i64())
                .unwrap_or(requested_size as i64),
        )
    };

    let mut items: Vec<MarketplaceSkillItem> =
        serde_json::from_value(serde_json::Value::Array(item_values))
            .map_err(|e| format!("Failed to parse marketplace items: {}", e))?;
    for item in &mut items {
        if item.name.trim().is_empty() {
            item.name = item.plugin_id.clone();
        }
    }

    Ok(MarketplaceResponse {
        items,
        total,
        page,
        size,
    })
}

fn marketplace_download_url(body: &serde_json::Value) -> Option<String> {
    body["package_url"]
        .as_str()
        .or_else(|| body["url"].as_str())
        .or_else(|| body["data"]["package_url"].as_str())
        .or_else(|| body["data"]["url"].as_str())
        .map(ToString::to_string)
}

fn marketplace_download_meta(body: &serde_json::Value) -> Option<serde_json::Value> {
    let category = body["category"]
        .as_str()
        .or_else(|| body["data"]["category"].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let display_i18n = body
        .get("displayI18n")
        .or_else(|| body.get("display_i18n"))
        .or_else(|| body["data"].get("displayI18n"))
        .or_else(|| body["data"].get("display_i18n"))
        .filter(|value| !value.is_null())
        .cloned();

    if category.is_none() && display_i18n.is_none() {
        return None;
    }

    let mut payload = serde_json::Map::new();
    if let Some(category) = category {
        payload.insert("category".to_string(), serde_json::Value::String(category));
    }
    if let Some(display_i18n) = display_i18n {
        payload.insert("displayI18n".to_string(), display_i18n);
    }
    Some(serde_json::Value::Object(payload))
}

fn install_marketplace_archive(
    archive_path: &Path,
    tmp_root: &Path,
    custom_dir: &Path,
    plugin_id: &str,
) -> Result<String, String> {
    if plugin_id.starts_with('_') || plugin_id.starts_with('.') || !is_valid_skill_id(plugin_id) {
        return Err(format!("Invalid plugin_id: {}", plugin_id));
    }

    std::fs::create_dir_all(custom_dir).map_err(|e| e.to_string())?;
    let prepared = tmp_root.join("prepared").join(plugin_id);
    crate::plugin::skill::global_sync::extract_global_skills_zip(archive_path, &prepared)
        .map_err(|e| e.to_string())?;

    let source = if prepared.join("SKILL.md").is_file() {
        prepared
    } else if let Some(subdir) =
        crate::plugin::skill::global_sync::find_single_level_skill_root(&prepared)
            .map_err(|e| e.to_string())?
    {
        subdir
    } else {
        return Err(format!(
            "skill package '{}' missing SKILL.md after extraction",
            plugin_id
        ));
    };

    crate::plugin::skill::global_sync::install_one_prepared_skill(&source, custom_dir, plugin_id)
        .map_err(|e| e.to_string())?;
    Ok(custom_dir.join(plugin_id).to_string_lossy().to_string())
}

fn write_marketplace_sidecar(dest: &Path, meta: Option<serde_json::Value>) {
    let Some(meta) = meta else {
        return;
    };
    if let Ok(bytes) = serde_json::to_vec(&meta) {
        if let Err(error) = std::fs::write(dest.join(".lotus-meta.json"), bytes) {
            log::warn!(
                "Marketplace: write .lotus-meta.json for '{}' failed: {}",
                dest.display(),
                error
            );
        }
    }
}

fn read_marketplace_archive_skill_md(
    archive_path: &Path,
    tmp_root: &Path,
    plugin_id: &str,
) -> Result<String, String> {
    if plugin_id.starts_with('_') || plugin_id.starts_with('.') || !is_valid_skill_id(plugin_id) {
        return Err(format!("Invalid plugin_id: {}", plugin_id));
    }

    let prepared = tmp_root.join("prepared").join(plugin_id);
    crate::plugin::skill::global_sync::extract_global_skills_zip(archive_path, &prepared)
        .map_err(|e| e.to_string())?;

    let source = if prepared.join("SKILL.md").is_file() {
        prepared
    } else if let Some(subdir) =
        crate::plugin::skill::global_sync::find_single_level_skill_root(&prepared)
            .map_err(|e| e.to_string())?
    {
        subdir
    } else {
        return Err(format!(
            "skill package '{}' missing SKILL.md after extraction",
            plugin_id
        ));
    };

    std::fs::read_to_string(source.join("SKILL.md")).map_err(|e| e.to_string())
}

/// List skill packages from the cloud marketplace.
pub async fn list_marketplace_skills_with_auth(
    auth: Arc<crate::auth::AuthManager>,
    page: u32,
    size: u32,
    category: Option<String>,
    search: Option<String>,
) -> Result<MarketplaceResponse, String> {
    let session_key = auth.get_session_key().await.map_err(|e| e.to_string())?;

    let client = reqwest::Client::new();
    // Gateway returns BOTH the caller's tenant skills AND scope=public in one
    // page; do not pass scope=public or tenant private skills are filtered out
    // server-side (see plugin/skill/global_sync.rs note).
    let mut url = format!(
        "{}/v1/skill-packages?page={}&size={}",
        crate::environment::tenant_host(),
        page,
        size
    );
    if let Some(cat) = &category {
        if !cat.is_empty() {
            url.push_str(&format!("&category={}", cat));
        }
    }
    if let Some(q) = &search {
        if !q.is_empty() {
            // Simple percent-encode for CJK search terms
            let encoded: String = q
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' {
                        c.to_string()
                    } else {
                        let mut buf = [0u8; 4];
                        c.encode_utf8(&mut buf);
                        buf[..c.len_utf8()]
                            .iter()
                            .map(|b| format!("%{:02X}", b))
                            .collect()
                    }
                })
                .collect();
            url.push_str(&format!("&search={}", encoded));
        }
    }

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", session_key))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("API error ({}): {}", status, body));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    parse_marketplace_response(body, page, size)
}

/// List skill packages from the cloud marketplace.
#[tauri::command]
pub async fn list_marketplace_skills(
    auth: tauri::State<'_, Arc<crate::auth::AuthManager>>,
    page: u32,
    size: u32,
    category: Option<String>,
    search: Option<String>,
) -> Result<MarketplaceResponse, String> {
    list_marketplace_skills_with_auth(auth.inner().clone(), page, size, category, search).await
}

/// Download and install a skill package from the marketplace.
/// Downloads the zip from `package_url` and installs it under the current
/// user's `~/.renlijia/users/{scope}/skills/{plugin_id}/`.
pub async fn install_marketplace_skill_with_auth(
    app: AppHandle,
    auth: Arc<crate::auth::AuthManager>,
    package_id: i64,
    plugin_id: String,
) -> Result<String, String> {
    let session_key = auth.get_session_key().await.map_err(|e| e.to_string())?;

    let client = reqwest::Client::new();

    // Step 1: Get the download URL
    let download_url = format!(
        "{}/v1/skill-packages/{}/download",
        crate::environment::tenant_host(),
        package_id
    );
    let resp = client
        .post(&download_url)
        .header("Authorization", format!("Bearer {}", session_key))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Download API error: {}", body));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let package_url = marketplace_download_url(&body).ok_or("No package_url in response")?;
    let sidecar_meta = marketplace_download_meta(&body);

    // Step 2: Download the zip file
    let zip_resp = client
        .get(&package_url)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| format!("Download error: {}", e))?;

    if !zip_resp.status().is_success() {
        return Err(format!(
            "Failed to download package: HTTP {}",
            zip_resp.status()
        ));
    }

    let zip_bytes = zip_resp
        .bytes()
        .await
        .map_err(|e| format!("Download error: {}", e))?;

    // Step 3: Extract to the current user's skills dir.
    let custom_dir = user_skills_dir(&app)?;
    let tmp = archive_tmp_root(&app);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let archive_path = tmp.join("marketplace-skill.zip");
    std::fs::write(&archive_path, zip_bytes.as_ref()).map_err(|e| e.to_string())?;
    let dest = install_marketplace_archive(
        &archive_path,
        &tmp.join("unpacked"),
        &custom_dir,
        &plugin_id,
    );
    let _ = std::fs::remove_dir_all(&tmp);
    let dest = dest?;
    write_marketplace_sidecar(Path::new(&dest), sidecar_meta);

    log::info!("Marketplace: installed skill '{}' to {:?}", plugin_id, dest);
    clear_enablement_override_for_skill(&app, &plugin_id)?;
    refresh_skill_registry(&app)?;
    Ok(format!("Installed '{}'", plugin_id))
}

/// Download and install a skill package from the marketplace.
/// Downloads the zip from `package_url` and installs it under the current
/// user's `~/.renlijia/users/{scope}/skills/{plugin_id}/`.
#[tauri::command]
pub async fn install_marketplace_skill(
    app: AppHandle,
    auth: tauri::State<'_, Arc<crate::auth::AuthManager>>,
    package_id: i64,
    plugin_id: String,
) -> Result<String, String> {
    install_marketplace_skill_with_auth(app, auth.inner().clone(), package_id, plugin_id).await
}

/// Download a marketplace skill package and return its SKILL.md without
/// installing it to the user's skill directory.
#[tauri::command]
pub async fn preview_marketplace_skill(
    app: AppHandle,
    auth: tauri::State<'_, Arc<crate::auth::AuthManager>>,
    package_id: i64,
    plugin_id: String,
) -> Result<MarketplaceSkillPreview, String> {
    let session_key = auth.get_session_key().await.map_err(|e| e.to_string())?;
    let client = reqwest::Client::new();
    let download_url = format!(
        "{}/v1/skill-packages/{}/download",
        crate::environment::tenant_host(),
        package_id
    );
    let resp = client
        .post(&download_url)
        .header("Authorization", format!("Bearer {}", session_key))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Download API error: {}", body));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let package_url = marketplace_download_url(&body).ok_or("No package_url in response")?;
    let zip_resp = client
        .get(&package_url)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| format!("Download error: {}", e))?;

    if !zip_resp.status().is_success() {
        return Err(format!(
            "Failed to download package: HTTP {}",
            zip_resp.status()
        ));
    }

    let zip_bytes = zip_resp
        .bytes()
        .await
        .map_err(|e| format!("Download error: {}", e))?;

    let tmp = archive_tmp_root(&app).join("preview");
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let archive_path = tmp.join("marketplace-skill-preview.zip");
    std::fs::write(&archive_path, zip_bytes.as_ref()).map_err(|e| e.to_string())?;
    let raw_content =
        read_marketplace_archive_skill_md(&archive_path, &tmp.join("unpacked"), &plugin_id);
    let _ = std::fs::remove_dir_all(&tmp);

    Ok(MarketplaceSkillPreview {
        raw_content: raw_content?,
    })
}

pub(crate) fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        // Skip symlinks to prevent path traversal attacks
        if src_path.is_symlink() {
            log::warn!("Skipping symlink during copy: {}", src_path.display());
            continue;
        }
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::skill::enablement::SkillEnablementState;
    use crate::plugin::skill::types::{DiskSkill, SkillFrontmatter, SkillMetadata, SkillSource};

    fn test_disk_skill(id: &str, source: SkillSource) -> DiskSkill {
        DiskSkill {
            id: id.to_string(),
            root: PathBuf::from("/tmp").join(id),
            frontmatter: SkillFrontmatter {
                name: id.to_string(),
                description: "desc".to_string(),
                when_to_use: None,
                allowed_tools: vec![],
                argument_hint: None,
                arguments: vec![],
                model: None,
                effort: None,
                context: None,
                agent: None,
                user_invocable: true,
                disable_model_invocation: false,
                version: None,
                paths: vec![],
                hooks: Default::default(),
                shell: None,
                category: None,
                metadata: SkillMetadata::default(),
            },
            body: String::new(),
            source,
        }
    }

    #[test]
    fn parses_marketplace_array_response_from_gateway() {
        let body = serde_json::json!({
            "code": 0,
            "data": [
                {
                    "id": 42,
                    "plugin_id": "bid-writing",
                    "name": "标书撰写",
                    "description": "解析招标文件",
                    "category": "general",
                    "icon": "file",
                    "version": "0.4",
                    "scope": "tenant",
                    "status": "published",
                    "package_size": 1234,
                    "created_at": "2026-06-15T00:00:00Z"
                }
            ],
            "total": 1,
            "page": 1,
            "size": 100
        });

        let parsed = parse_marketplace_response(body, 1, 100).unwrap();

        assert_eq!(parsed.total, 1);
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].plugin_id, "bid-writing");
        assert_eq!(parsed.items[0].package_size, 1234);
    }

    #[test]
    fn parses_marketplace_paged_items_response() {
        let body = serde_json::json!({
            "code": 0,
            "data": {
                "items": [
                    {
                        "id": 7,
                        "pluginId": "deep-research",
                        "description": "研究报告",
                        "scope": "public"
                    }
                ],
                "total": 1,
                "page": 2,
                "size": 10
            }
        });

        let parsed = parse_marketplace_response(body, 2, 10).unwrap();

        assert_eq!(parsed.page, 2);
        assert_eq!(parsed.size, 10);
        assert_eq!(parsed.items[0].plugin_id, "deep-research");
        assert_eq!(parsed.items[0].name, "deep-research");
    }

    #[test]
    fn list_skills_merges_enabled_state_without_filtering_disabled() {
        let registry = Arc::new(Mutex::new(SkillRegistry::from_skills(vec![
            test_disk_skill("enabled-skill", SkillSource::User),
            test_disk_skill("disabled-skill", SkillSource::User),
        ])));
        let mut enablement = SkillEnablementState::default();
        enablement
            .disabled_skill_ids
            .insert("disabled-skill".to_string());

        let infos = list_skills_from_registry_with_enablement(&registry, &enablement);

        assert_eq!(infos.len(), 2);
        assert_eq!(
            infos
                .iter()
                .map(|info| (info.id.as_str(), info.enabled))
                .collect::<Vec<_>>(),
            vec![("disabled-skill", false), ("enabled-skill", true)]
        );
    }

    #[test]
    fn set_skill_enabled_unknown_skill_does_not_write_config() {
        let tmp = tempfile::tempdir().unwrap();
        let home = Arc::new(crate::storage::AiJiaHome::from_path(
            tmp.path().to_path_buf(),
        ));
        let current_user = Arc::new(crate::storage::CurrentUserStorage::new(home));
        current_user
            .activate_scope(crate::storage::UserScope::new(1, 2))
            .unwrap();
        let config_path = current_user.resolve_paths().unwrap().skills_config_path();
        let store = crate::plugin::skill::enablement::SkillEnablementStore::new(current_user);
        let registry = Arc::new(Mutex::new(SkillRegistry::from_skills(Vec::new())));

        let err =
            set_skill_enabled_for_registry(&registry, &store, "missing-skill", false, || Ok(()))
                .unwrap_err();

        assert!(err.contains("Unknown skill"));
        assert!(!config_path.exists());
    }

    #[test]
    fn init_skill_template_writes_skill_md_and_subdirs() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().to_string_lossy().to_string();

        let output_dir = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(init_skill_template(
                target,
                "demo-skill".to_string(),
                "演示技能".to_string(),
            ))
            .unwrap();

        let skill_dir = PathBuf::from(output_dir);
        assert!(skill_dir.join("SKILL.md").exists());
        assert!(skill_dir.join("scripts").is_dir());
        assert!(skill_dir.join("references").is_dir());
        assert!(skill_dir.join("assets").is_dir());
        // SKILL.md contains name: and description: lines
        let content = std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
        assert!(content.contains("name:"));
        assert!(content.contains("description:"));
    }

    #[test]
    fn pack_skill_md_only_writes_skill_md() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("demo");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let body = "---\nname: demo\ndescription: 测试\n---\n# body\n";
        std::fs::write(skill_dir.join("SKILL.md"), body).unwrap();

        let dest = tmp.path().join("out").join("demo.md");
        let written = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(pack_skill(
                skill_dir.to_string_lossy().to_string(),
                dest.to_string_lossy().to_string(),
            ))
            .unwrap();
        let content = std::fs::read_to_string(&written).unwrap();
        assert_eq!(content, body);
    }

    #[test]
    fn pack_skill_zip_packs_full_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("demo");
        std::fs::create_dir_all(skill_dir.join("references")).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: 测试\n---\nbody",
        )
        .unwrap();
        std::fs::write(skill_dir.join("references").join("a.md"), "ref").unwrap();

        let dest = tmp.path().join("out").join("demo.zip");
        let written = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(pack_skill(
                skill_dir.to_string_lossy().to_string(),
                dest.to_string_lossy().to_string(),
            ))
            .unwrap();
        let unpack_root = tmp.path().join("unpack");
        let res = crate::storage::skill_package::unpack_skill_archive(
            std::path::Path::new(&written),
            &unpack_root,
        )
        .unwrap();
        assert!(res.skill_dir.join("SKILL.md").is_file());
        assert!(res.skill_dir.join("references").join("a.md").is_file());
        assert_eq!(res.skill_id, "demo");
    }

    // ---- install_skill_md_file -------------------------------------------

    fn write_md(path: &Path, body: &str) {
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn install_md_writes_skill_md_under_id_from_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let custom_dir = tmp.path().join("user-skills");
        let src = tmp.path().join("anything.md");
        // skill_id comes from `name:`, not the filename.
        write_md(&src, "---\nname: my-skill\ndescription: 测试\n---\nbody");
        let dest = install_skill_md_file(&src, &custom_dir, false).unwrap();
        let dest_path = PathBuf::from(&dest);
        assert!(dest_path.join("SKILL.md").is_file());
        assert_eq!(dest_path.file_name().unwrap().to_string_lossy(), "my-skill");
    }

    #[test]
    fn install_md_tolerates_crlf_and_bom_windows_editors() {
        // Windows Notepad: UTF-8 BOM + CRLF line endings.
        let tmp = tempfile::tempdir().unwrap();
        let custom_dir = tmp.path().join("user-skills");
        let src = tmp.path().join("skill.md");
        let body = "\u{feff}---\r\nname: win-skill\r\ndescription: 测试\r\n---\r\nbody\r\n";
        write_md(&src, body);
        let dest = install_skill_md_file(&src, &custom_dir, false).unwrap();
        assert!(PathBuf::from(&dest).join("SKILL.md").is_file());
    }

    #[test]
    fn install_md_rejects_invalid_skill_id() {
        let tmp = tempfile::tempdir().unwrap();
        let custom_dir = tmp.path().join("user-skills");
        let src = tmp.path().join("bad.md");
        // Uppercase + space — not is_valid_skill_id.
        write_md(&src, "---\nname: Bad Name\ndescription: 测试\n---\nbody");
        let err = install_skill_md_file(&src, &custom_dir, false).unwrap_err();
        assert!(matches!(err, InstallSkillError::InvalidName(_)));
    }

    #[test]
    fn install_md_missing_frontmatter_returns_parse_failed() {
        let tmp = tempfile::tempdir().unwrap();
        let custom_dir = tmp.path().join("user-skills");
        let src = tmp.path().join("nofm.md");
        write_md(&src, "no frontmatter here\n");
        let err = install_skill_md_file(&src, &custom_dir, false).unwrap_err();
        assert!(matches!(err, InstallSkillError::ParseFailed(_)));
    }

    #[test]
    fn install_md_already_exists_unless_force() {
        let tmp = tempfile::tempdir().unwrap();
        let custom_dir = tmp.path().join("user-skills");
        let src = tmp.path().join("a.md");
        write_md(&src, "---\nname: dup-skill\ndescription: 测试\n---\nv1");
        install_skill_md_file(&src, &custom_dir, false).unwrap();

        write_md(&src, "---\nname: dup-skill\ndescription: 测试\n---\nv2");
        let err = install_skill_md_file(&src, &custom_dir, false).unwrap_err();
        match err {
            InstallSkillError::AlreadyExists(id) => assert_eq!(id, "dup-skill"),
            other => panic!("expected AlreadyExists, got {:?}", other),
        }

        let dest = install_skill_md_file(&src, &custom_dir, true).unwrap();
        let written = std::fs::read_to_string(PathBuf::from(dest).join("SKILL.md")).unwrap();
        assert!(written.contains("v2"));
    }

    // ---- install_skill_archive -------------------------------------------

    fn make_skill_dir(root: &Path, id: &str) -> PathBuf {
        let d = root.join(id);
        std::fs::create_dir_all(d.join("references")).unwrap();
        std::fs::write(
            d.join("SKILL.md"),
            format!("---\nname: {id}\ndescription: 测试\n---\nbody"),
        )
        .unwrap();
        std::fs::write(d.join("references").join("a.md"), "ref").unwrap();
        d
    }

    #[test]
    fn install_archive_happy_path() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("src");
        let skill = make_skill_dir(&staging, "demo");
        let archive = tmp.path().join("demo.zip");
        crate::storage::skill_package::pack_skill_dir(&skill, &archive, "demo").unwrap();

        let custom_dir = tmp.path().join("user-skills");
        let unpack = tmp.path().join("unpack-tmp");
        let dest = install_skill_archive(&archive, &unpack, &custom_dir, false).unwrap();
        let dest_path = PathBuf::from(&dest);
        assert!(dest_path.join("SKILL.md").is_file());
        assert!(dest_path.join("references").join("a.md").is_file());
        assert_eq!(dest_path.file_name().unwrap().to_string_lossy(), "demo");
    }

    #[test]
    fn install_archive_already_exists_unless_force() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("src");
        let skill = make_skill_dir(&staging, "dup");
        let archive = tmp.path().join("dup.zip");
        crate::storage::skill_package::pack_skill_dir(&skill, &archive, "dup").unwrap();
        let custom_dir = tmp.path().join("user-skills");

        install_skill_archive(&archive, &tmp.path().join("u1"), &custom_dir, false).unwrap();
        let err = install_skill_archive(&archive, &tmp.path().join("u2"), &custom_dir, false)
            .unwrap_err();
        assert!(matches!(err, InstallSkillError::AlreadyExists(id) if id == "dup"));

        install_skill_archive(&archive, &tmp.path().join("u3"), &custom_dir, true).unwrap();
    }

    #[test]
    fn install_archive_rejects_corrupt_zip() {
        // Zip with no SKILL.md anywhere — unpack_skill_archive must reject.
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("bad.zip");
        let file = std::fs::File::create(&archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("only/readme.txt", opts).unwrap();
        use std::io::Write;
        zip.write_all(b"not a skill").unwrap();
        zip.finish().unwrap();

        let custom_dir = tmp.path().join("user-skills");
        let err =
            install_skill_archive(&archive, &tmp.path().join("u"), &custom_dir, false).unwrap_err();
        assert!(
            matches!(err, InstallSkillError::ParseFailed(_)),
            "got {:?}",
            err
        );
    }

    fn write_marketplace_zip(path: &Path, entries: &[(&str, &str)]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        use std::io::Write;
        for (name, content) in entries {
            zip.start_file(name, opts).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn install_marketplace_archive_accepts_server_sidecars() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("server.zip");
        write_marketplace_zip(
            &archive,
            &[
                (
                    "SKILL.md",
                    "---\nname: xiaojia-doctor\ndescription: doctor\n---\nbody",
                ),
                (".lotus-meta.json", r#"{"category":"runtime"}"#),
                (".scope", "tenant"),
                ("scripts/doctor.ps1", "Write-Output ok"),
                ("references/runtime-doctor.md", "# ref"),
            ],
        );

        let custom_dir = tmp.path().join("user-skills");
        let dest = install_marketplace_archive(
            &archive,
            &tmp.path().join("unpack"),
            &custom_dir,
            "xiaojia-doctor",
        )
        .unwrap();
        let dest = PathBuf::from(dest);

        assert_eq!(
            dest.file_name().unwrap().to_string_lossy(),
            "xiaojia-doctor"
        );
        assert!(dest.join("SKILL.md").is_file());
        assert!(dest.join(".lotus-meta.json").is_file());
        assert!(dest.join(".scope").is_file());
        assert!(dest.join("scripts").join("doctor.ps1").is_file());
        assert!(dest.join("references").join("runtime-doctor.md").is_file());
    }

    #[test]
    fn install_marketplace_archive_accepts_inner_skill_layout_and_i18n() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("server-inner.zip");
        write_marketplace_zip(
            &archive,
            &[
                (
                    "bid-writing/SKILL.md",
                    "---\nname: bid-writing\ndescription: bid\n---\nbody",
                ),
                (
                    "bid-writing/i18n/en-US/SKILL.md",
                    "---\nname: bid-writing\ndescription: bid\n---\nbody",
                ),
                ("bid-writing/references/outline_check.py", "print('ok')"),
            ],
        );

        let custom_dir = tmp.path().join("user-skills");
        let dest = install_marketplace_archive(
            &archive,
            &tmp.path().join("unpack"),
            &custom_dir,
            "bid-writing",
        )
        .unwrap();
        let dest = PathBuf::from(dest);

        assert_eq!(dest.file_name().unwrap().to_string_lossy(), "bid-writing");
        assert!(dest.join("SKILL.md").is_file());
        assert!(dest.join("i18n").join("en-US").join("SKILL.md").is_file());
        assert!(dest.join("references").join("outline_check.py").is_file());
    }
}
