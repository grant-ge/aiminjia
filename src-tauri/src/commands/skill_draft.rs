//! Tauri commands for Skill-Smith (小程) draft management.
//!
//! Frontend uses these to:
//! - List unfinished drafts (DraftBanner in SkillsTab)
//! - Discard a draft when user gives up
//! - Inspect a single draft's metadata before resuming a conversation
//! - Import .aijia-skill packages from disk (drag-drop / file association / button)

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::storage::fs_atomic::{remove_dir_all_retry, replace_dir_atomic};
use crate::storage::skill_draft_store::{DraftMeta, SkillDraftStore};
use crate::storage::skill_package;
use crate::storage::CurrentUserStorage;

#[derive(Debug, Clone, Serialize)]
pub struct DraftMetaInfo {
    pub draft_id: String,
    pub conversation_id: Option<String>,
    pub name: String,
    pub description: String,
    pub created_at: String,
    pub last_modified_at: String,
    pub installed_to: Option<String>,
}

impl From<DraftMeta> for DraftMetaInfo {
    fn from(m: DraftMeta) -> Self {
        Self {
            draft_id: m.draft_id,
            conversation_id: m.conversation_id,
            name: m.name,
            description: m.description,
            created_at: m.created_at.to_rfc3339(),
            last_modified_at: m.last_modified_at.to_rfc3339(),
            installed_to: m.installed_to.map(|p| p.to_string_lossy().to_string()),
        }
    }
}

fn store_for(app: &AppHandle) -> Result<(Arc<SkillDraftStore>, crate::storage::UserScope), String> {
    let cus = app
        .try_state::<Arc<CurrentUserStorage>>()
        .ok_or_else(|| "CurrentUserStorage not registered".to_string())?
        .inner()
        .clone();
    let scope = cus.scope().ok_or_else(|| "no active user scope".to_string())?;
    let home = Arc::new(cus.home().clone());
    Ok((Arc::new(SkillDraftStore::new(home)), scope))
}

/// 列出当前用户的全部草稿，按 last_modified_at ��序。
#[tauri::command]
pub async fn list_skill_drafts(app: AppHandle) -> Result<Vec<DraftMetaInfo>, String> {
    let (store, scope) = store_for(&app)?;
    let metas = store.list(&scope).map_err(|e| e.to_string())?;
    Ok(metas.into_iter().map(DraftMetaInfo::from).collect())
}

/// 删除一个草稿目录（不可恢复）。
#[tauri::command]
pub async fn discard_skill_draft(app: AppHandle, draft_id: String) -> Result<(), String> {
    let (store, scope) = store_for(&app)?;
    store.discard(&scope, &draft_id).map_err(|e| e.to_string())
}

/// 读取单条 meta（前端继续草稿时用）。
#[tauri::command]
pub async fn get_skill_draft_meta(app: AppHandle, draft_id: String) -> Result<DraftMetaInfo, String> {
    let (store, scope) = store_for(&app)?;
    let meta = store.read_meta(&scope, &draft_id).map_err(|e| e.to_string())?;
    Ok(DraftMetaInfo::from(meta))
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status")]
pub enum ImportSkillOutcome {
    #[serde(rename = "installed")]
    Installed {
        id: String,
        name: String,
        version: String,
        installed_to: String,
    },
    #[serde(rename = "conflict")]
    Conflict {
        id: String,
        name: String,
        version: String,
        existing_path: String,
    },
}

/// 导入 `.aijia-skill` zip 包到当前用户的技能库。
///
/// - `force=false`：同名冲突时返回 `{status: "conflict", ...}`，由前端弹窗给用户选择。
/// - `force=true`：覆盖已有目录。
/// - 校验 manifest format_version + sha256 + frontmatter.name == manifest.id（zip-slip / 篡改防御）。
#[tauri::command]
pub async fn import_skill_package(
    app: AppHandle,
    archive_path: String,
    force: Option<bool>,
) -> Result<ImportSkillOutcome, String> {
    let force = force.unwrap_or(false);
    let archive = PathBuf::from(&archive_path);
    if !archive.is_file() {
        return Err(format!("file not found: {}", archive_path));
    }

    let cus = app
        .try_state::<Arc<CurrentUserStorage>>()
        .ok_or_else(|| "CurrentUserStorage not registered".to_string())?
        .inner()
        .clone();
    let scope = cus.scope().ok_or_else(|| "no active user scope".to_string())?;
    let home = cus.home();

    // 1) 解包到临时区
    let tmp_root = home
        .root()
        .join("tmp")
        .join(format!("skill-import-{}", uuid::Uuid::new_v4()));
    let res = skill_package::unpack_skill_archive(&archive, &tmp_root)
        .map_err(|e| format!("解包失败：{}", e))?;

    // 2) 检查目标
    let user_skills = home.user_skills_dir(&scope);
    std::fs::create_dir_all(&user_skills).map_err(|e| e.to_string())?;
    let target = user_skills.join(&res.manifest.id);

    if target.exists() && !force {
        // 清理 tmp，把冲突信息丢回去
        let _ = remove_dir_all_retry(&tmp_root);
        return Ok(ImportSkillOutcome::Conflict {
            id: res.manifest.id.clone(),
            name: res.manifest.name.clone(),
            version: res.manifest.version.clone(),
            existing_path: target.to_string_lossy().to_string(),
        });
    }

    // 3) 原子替换：先 stage 再 swap，失败时回滚
    if let Err(e) = replace_dir_atomic(&res.skill_dir, &target) {
        let _ = remove_dir_all_retry(&tmp_root);
        return Err(format!("install (commit): {}", e));
    }
    let _ = remove_dir_all_retry(&tmp_root);

    // 热加载：刷新全局 SkillRegistry，让新导入的技能立即可用，无需重启 app。
    if let Some(registry) = app
        .try_state::<Arc<std::sync::Mutex<crate::plugin::skill::registry::SkillRegistry>>>()
    {
        let roots = vec![home.user_skills_dir(&scope), home.skills_dir()];
        crate::plugin::skill::global_sync::reload_skill_registry(&roots, registry.inner());
    }

    Ok(ImportSkillOutcome::Installed {
        id: res.manifest.id,
        name: res.manifest.name,
        version: res.manifest.version,
        installed_to: target.to_string_lossy().to_string(),
    })
}

/// 把已安装技能打包成 .aijia-skill 文件。前端 SkillCard 的"导出"按钮调用此命令。
///
/// - `skill_id`：已安装技能 id（= ~/.renlijia/users/{scope}/skills/<id>/）
/// - `dest_path`：可选目标路径，默认 ~/Desktop/<id>-v<version>.aijia-skill
/// - `version` / `author`：写入 manifest，默认 "0.1.0"
///
/// 返回最终落盘的绝对路径，前端可以用 `revealItemInDir` 高亮给用户看。
#[tauri::command]
pub async fn export_installed_skill(
    app: AppHandle,
    skill_id: String,
    dest_path: Option<String>,
    version: Option<String>,
    author: Option<String>,
) -> Result<String, String> {
    let cus = app
        .try_state::<Arc<CurrentUserStorage>>()
        .ok_or_else(|| "CurrentUserStorage not registered".to_string())?
        .inner()
        .clone();
    let scope = cus.scope().ok_or_else(|| "no active user scope".to_string())?;
    let home = cus.home();

    let user_dir = home.user_skills_dir(&scope).join(&skill_id);
    let global_dir = home.skills_dir().join(&skill_id);
    let source_dir = if user_dir.is_dir() {
        user_dir
    } else if global_dir.is_dir() {
        global_dir
    } else {
        return Err(format!("已安装技能 '{}' 不存在", skill_id));
    };

    let version = version.unwrap_or_else(|| "0.1.0".to_string());
    let dest = match dest_path {
        Some(p) => PathBuf::from(p),
        None => default_export_dest(&skill_id, &version),
    };

    skill_package::pack_skill_dir(
        &source_dir,
        &dest,
        &skill_id,
        &skill_id,
        &version,
        author.as_deref(),
        concat!("skill-smith@", env!("CARGO_PKG_VERSION")),
    )
    .map_err(|e| format!("打包失败：{}", e))?;
    Ok(dest.to_string_lossy().to_string())
}

fn default_export_dest(skill_id: &str, version: &str) -> PathBuf {
    let desktop = dirs::desktop_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    desktop.join(format!("{}-v{}.aijia-skill", skill_id, version))
}
