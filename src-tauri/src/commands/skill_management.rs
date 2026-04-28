use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::AppHandle;
use tauri::Emitter;
use tauri::Manager;
use crate::storage::UserScopedPathResolver;
use crate::plugin::skill::registry::SkillRegistry;

/// Skill info returned by `list_skills` IPC — only SKILL.md-backed skills.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub icon: Option<String>,
    pub category: Option<String>,
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
                category: None,
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
    Ok(cus
        .require_paths()
        .map_err(|e| e.to_string())?
        .skills_dir())
}

fn install_custom_skill_to_dir(source: &Path, custom_dir: &Path) -> Result<String, String> {
    let basename = source
        .file_name()
        .ok_or_else(|| format!("Source path '{}' has no basename", source.display()))?;
    let dest = custom_dir.join(basename);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| {
            format!("Failed to remove existing skill at '{}': {}", dest.display(), e)
        })?;
    }
    copy_dir_recursive(source, &dest)
        .map_err(|e| format!("Failed to copy skill: {}", e))?;
    Ok(dest.to_string_lossy().to_string())
}

fn load_skill_for_reload(
    _path: &Path,
) -> Result<(String, Box<dyn crate::plugin::skill_trait::Skill>), String> {
    unimplemented!("Skill reload will be restored after Phase D SkillRegistry lands.")
}

/// List all installed custom skills.
#[tauri::command]
pub async fn list_custom_skills(app: AppHandle) -> Result<Vec<CustomSkillInfo>, String> {
    let custom_dir = user_skills_dir(&app)?;
    list_custom_skills_in_dir(&custom_dir)
}

/// Install a skill from a directory path (copy to ~/.renlijia/skills/).
#[tauri::command]
pub async fn install_custom_skill(app: AppHandle, source_path: String) -> Result<String, String> {
    let source = PathBuf::from(&source_path);
    if !source.is_dir() {
        return Err(format!("Source path '{}' is not a directory", source_path));
    }
    if !source.join("SKILL.md").is_file() {
        return Err(format!(
            "Source path '{}' does not contain SKILL.md",
            source_path
        ));
    }
    let custom_dir = user_skills_dir(&app)?;
    std::fs::create_dir_all(&custom_dir).map_err(|e| e.to_string())?;
    install_custom_skill_to_dir(&source, &custom_dir)
}

/// Uninstall a custom skill by ID.
#[tauri::command]
pub async fn uninstall_custom_skill(app: AppHandle, skill_id: String) -> Result<String, String> {
    let skill_dir = user_skills_dir(&app)?.join(&skill_id);

    if !skill_dir.exists() {
        return Err(format!("Custom skill '{}' not found", skill_id));
    }

    std::fs::remove_dir_all(&skill_dir).map_err(|e| e.to_string())?;
    Ok(format!(
        "Uninstalled skill '{}' — restart app to take effect",
        skill_id
    ))
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
    unimplemented!("Skill packaging will be restored in a follow-up after Phase D SkillRegistry lands.")
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

        let out_path = dest.join(file.mangled_name());

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

pub(crate) fn pack_skill_to_dir(_skill_dir: &Path, _output_dir: &Path) -> Result<PathBuf, String> {
    unimplemented!("Skill packaging will be restored in a follow-up after Phase D SkillRegistry lands.")
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
