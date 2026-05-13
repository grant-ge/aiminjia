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
    let basename = source
        .file_name()
        .ok_or_else(|| InstallSkillError::Io(format!("Source '{}' has no basename", source.display())))?;
    let dest = custom_dir.join(basename);
    if dest.exists() {
        if !force {
            return Err(InstallSkillError::AlreadyExists(basename.to_string_lossy().to_string()));
        }
        std::fs::remove_dir_all(&dest)
            .map_err(|e| InstallSkillError::Io(format!("Failed to remove existing skill: {}", e)))?;
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
    let basename = source
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
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
    pub description: String,
    pub icon: Option<String>,
    pub category: Option<String>,
    /// "user"  — installed under ~/.renlijia/users/{scope}/skills/
    /// "global" — managed bundle in ~/.renlijia/skills/
    /// 用于前端区分"本地技能"分类。
    pub source: String,
}

/// Pure function for testability: list all skills in the new disk-backed registry.
pub fn list_skills_from_registry(registry: &Arc<Mutex<SkillRegistry>>) -> Vec<SkillInfo> {
    let guard = registry.lock().unwrap();
    guard
        .skill_ids()
        .into_iter()
        .filter_map(|id| {
            guard.get(&id).map(|skill| SkillInfo {
                id: skill.id.clone(),
                display_name: skill
                    .frontmatter
                    .metadata
                    .label
                    .clone()
                    .unwrap_or_else(|| skill.frontmatter.name.clone()),
                description: skill.frontmatter.description.clone(),
                icon: None,
                category: skill.frontmatter.category.clone(),
                source: match skill.source {
                    crate::plugin::skill::types::SkillSource::User => "user".to_string(),
                    crate::plugin::skill::types::SkillSource::Global => "global".to_string(),
                },
            })
        })
        .collect()
}

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

/// Global storage for the dev-mode file watcher.
/// Only one skill can be watched at a time.
static DEV_WATCHER: once_cell::sync::Lazy<std::sync::Mutex<Option<RecommendedWatcher>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(None));

#[derive(serde::Serialize)]
pub struct CustomSkillInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub path: String,
    pub enabled: bool,
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
            skills.push(CustomSkillInfo {
                id: id.clone(),
                name: id.clone(),
                description: String::new(),
                path: path.to_string_lossy().to_string(),
                enabled: true,
            });
        }
    }
    Ok(skills)
}

fn user_skills_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let cus = app.state::<Arc<crate::storage::CurrentUserStorage>>();
    Ok(cus.require_paths().map_err(|e| e.to_string())?.skills_dir())
}

/// Re-scan both [user_skills_dir, global_skills_dir] roots and replace the
/// in-memory `SkillRegistry`. Both roots are always scanned because user-root
/// skills shadow same-id global skills; a single-root scan would mis-resurrect
/// or hide skills after uninstall.
pub fn refresh_skill_registry(app: &AppHandle) -> Result<(), String> {
    use crate::plugin::skill::loader::load_skill_roots;
    use crate::storage::AiJiaHome;

    let aijia_home = app.state::<Arc<AiJiaHome>>();
    let global_root = aijia_home.skills_dir();
    let user_root = user_skills_dir(app).ok();
    let roots: Vec<PathBuf> = match user_root {
        Some(user) => vec![user, global_root],
        None => vec![global_root],
    };

    let loaded = load_skill_roots(&roots).map_err(|e| format!("load_skill_roots failed: {}", e))?;
    let registry = app.state::<Arc<Mutex<SkillRegistry>>>();
    registry
        .lock()
        .map_err(|e| format!("registry lock poisoned: {}", e))?
        .replace_all(loaded.into_values().collect());
    Ok(())
}

/// List all installed custom skills.
#[tauri::command]
pub async fn list_custom_skills(app: AppHandle) -> Result<Vec<CustomSkillInfo>, String> {
    let custom_dir = user_skills_dir(&app)?;
    list_custom_skills_in_dir(&custom_dir)
}

/// Install a skill from a directory path into the current user's skills dir.
/// `force=false`: returns `AlreadyExists` if same-name skill exists.
/// `force=true`: overwrites existing skill.
/// On success: re-scans both user + global roots and refreshes in-memory registry.
#[tauri::command]
pub async fn install_custom_skill(
    app: AppHandle,
    source_path: String,
    force: Option<bool>,
) -> Result<String, InstallSkillError> {
    let source = PathBuf::from(&source_path);
    if !source.is_dir() {
        return Err(InstallSkillError::Io(format!(
            "Source path '{}' is not a directory",
            source_path
        )));
    }

    validate_skill_directory(&source).map_err(InstallSkillError::from_validation)?;

    let custom_dir = user_skills_dir(&app).map_err(InstallSkillError::Io)?;
    std::fs::create_dir_all(&custom_dir).map_err(|e| InstallSkillError::Io(e.to_string()))?;

    let dest = install_custom_skill_to_dir_with_force(&source, &custom_dir, force.unwrap_or(false))?;

    refresh_skill_registry(&app).map_err(InstallSkillError::Io)?;
    Ok(dest)
}

/// Uninstall a custom skill by ID.
#[tauri::command]
pub async fn uninstall_custom_skill(app: AppHandle, skill_id: String) -> Result<String, String> {
    let skill_dir = user_skills_dir(&app)?.join(&skill_id);

    if !skill_dir.exists() {
        return Err(format!("Custom skill '{}' not found", skill_id));
    }

    std::fs::remove_dir_all(&skill_dir).map_err(|e| e.to_string())?;
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

/// Pack a skill directory into a .aijia-skill zip file.
#[tauri::command]
pub async fn pack_skill(_skill_dir: String) -> Result<String, String> {
    unimplemented!(
        "Skill packaging will be restored in a follow-up after Phase D SkillRegistry lands."
    )
}

/// Reload a custom skill from disk (hot-reload for dev mode).
/// Re-reads the skill manifest (`SKILL.md`), unregisters the
/// old version, and registers the new one.
#[tauri::command]
pub async fn reload_skill(_app: AppHandle, _skill_path: String) -> Result<String, String> {
    unimplemented!("Skill reload will be restored after Phase D SkillRegistry lands.")
}

/// Start watching a skill directory for file changes (dev mode).
/// Emits `skill-file-changed` Tauri event when files are modified.
#[tauri::command]
pub async fn start_skill_watch(app: AppHandle, skill_path: String) -> Result<String, String> {
    let path = PathBuf::from(&skill_path);
    if !path.is_dir() {
        return Err("Not a valid directory".to_string());
    }

    let app_clone = app.clone();
    let path_str = skill_path.clone();

    let mut watcher =
        notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                // Only emit on content changes (modify/create), not on access or removal
                if event.kind.is_modify() || event.kind.is_create() {
                    let _ = app_clone.emit("skill-file-changed", &path_str);
                }
            }
        })
        .map_err(|e| e.to_string())?;

    watcher
        .watch(&path, RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;

    // Store the watcher (drops any previous watcher, stopping its watch)
    *DEV_WATCHER.lock().unwrap_or_else(|e| e.into_inner()) = Some(watcher);

    log::info!("Dev mode: watching skill directory '{}'", path.display());
    Ok(format!("Watching '{}'", path.display()))
}

/// Stop watching the skill directory (dev mode).
#[tauri::command]
pub async fn stop_skill_watch() -> Result<String, String> {
    *DEV_WATCHER.lock().unwrap_or_else(|e| e.into_inner()) = None;
    log::info!("Dev mode: stopped watching skill directory");
    Ok("Stopped watching".to_string())
}

// ---------------------------------------------------------------------------
// Marketplace Commands
// ---------------------------------------------------------------------------

/// Marketplace skill package returned from the API.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSkillItem {
    pub id: i64,
    pub plugin_id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub icon: String,
    pub version: String,
    pub scope: String,
    pub status: String,
    pub downloads: i64,
    pub featured: bool,
    pub package_size: i64,
    pub tenant_name: String,
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

/// List skill packages from the cloud marketplace.
#[tauri::command]
pub async fn list_marketplace_skills(
    auth: tauri::State<'_, Arc<crate::auth::AuthManager>>,
    page: u32,
    size: u32,
    category: Option<String>,
    search: Option<String>,
) -> Result<MarketplaceResponse, String> {
    let session_key = auth.get_session_key().await.map_err(|e| e.to_string())?;

    let client = reqwest::Client::new();
    let mut url = format!(
        "https://ai-tenant.renlijia.com/v1/skill-packages?page={}&size={}&scope=public",
        page, size
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

    // Parse { code: 0, data: { items, total, page, size } }
    let data = &body["data"];
    let items: Vec<MarketplaceSkillItem> =
        serde_json::from_value(data["items"].clone()).unwrap_or_default();

    Ok(MarketplaceResponse {
        items,
        total: data["total"].as_i64().unwrap_or(0),
        page: data["page"].as_i64().unwrap_or(1),
        size: data["size"].as_i64().unwrap_or(20),
    })
}

/// Download and install a skill package from the marketplace.
/// Downloads the zip from `package_url` and extracts to `~/.renlijia/skills/{plugin_id}/`.
#[tauri::command]
pub async fn install_marketplace_skill(
    app: AppHandle,
    auth: tauri::State<'_, Arc<crate::auth::AuthManager>>,
    package_id: i64,
    plugin_id: String,
) -> Result<String, String> {
    let session_key = auth.get_session_key().await.map_err(|e| e.to_string())?;

    let client = reqwest::Client::new();

    // Step 1: Get the download URL
    let download_url = format!(
        "https://ai-tenant.renlijia.com/v1/skill-packages/{}/download",
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
    let package_url = body["package_url"]
        .as_str()
        .or_else(|| body["data"]["package_url"].as_str())
        .ok_or("No package_url in response")?
        .to_string();

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

    // Step 3: Extract to ~/.renlijia/skills/{plugin_id}/
    let custom_dir = user_skills_dir(&app)?;
    std::fs::create_dir_all(&custom_dir).map_err(|e| e.to_string())?;

    let dest = custom_dir.join(&plugin_id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;

    let cursor = std::io::Cursor::new(zip_bytes.as_ref());
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("Invalid zip: {}", e))?;

    const MAX_EXTRACT_SIZE: u64 = 50 * 1024 * 1024; // 50 MB limit
    let mut total_extracted: u64 = 0;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;

        total_extracted += file.size();
        if total_extracted > MAX_EXTRACT_SIZE {
            let _ = std::fs::remove_dir_all(&dest);
            return Err("Package too large (exceeds 50MB extraction limit)".to_string());
        }

        // Use enclosed_name() to prevent path traversal attacks — it returns
        // None for entries whose resolved path would escape the destination.
        let relative = file
            .enclosed_name()
            .ok_or_else(|| format!("Unsafe zip entry path: {:?}", file.name()))?
            .to_path_buf();
        let out_path = dest.join(&relative);

        // Belt-and-suspenders: verify the resolved path is still under dest
        if !out_path.starts_with(&dest) {
            return Err(format!("Path traversal detected: {:?}", relative));
        }

        if file.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut outfile = std::fs::File::create(&out_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut file, &mut outfile).map_err(|e| e.to_string())?;
        }
    }

    log::info!("Marketplace: installed skill '{}' to {:?}", plugin_id, dest);
    Ok(format!(
        "Installed '{}' — restart app to activate",
        plugin_id
    ))
}

#[allow(dead_code)] // Placeholder until Phase D SkillRegistry lands; covered by `pack_skill_to_dir_unimplemented_until_phase_d` test.
pub(crate) fn pack_skill_to_dir(_skill_dir: &Path, _output_dir: &Path) -> Result<PathBuf, String> {
    unimplemented!(
        "Skill packaging will be restored in a follow-up after Phase D SkillRegistry lands."
    )
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
    #[should_panic(expected = "Skill packaging will be restored")]
    fn pack_skill_to_dir_unimplemented_until_phase_d() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("skill-md-only");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let output_dir = tmp.path().join("out");
        // Still unimplemented — Phase D SkillRegistry
        let _ = pack_skill_to_dir(&skill_dir, &output_dir);
    }
}
