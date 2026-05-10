//! Tauri commands for Skill-Smith (小程) draft management.
//!
//! Frontend uses these to:
//! - List unfinished drafts (DraftBanner in SkillsTab)
//! - Discard a draft when user gives up
//! - Inspect a single draft's metadata before resuming a conversation

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::storage::skill_draft_store::{DraftMeta, SkillDraftStore};
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
