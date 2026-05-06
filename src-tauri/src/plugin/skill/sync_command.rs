use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::auth::client::BASE_URL;
use crate::auth::AuthManager;
use crate::plugin::skill::global_sync::{
    reload_skill_registry, sync_skill_packages_from_server, GlobalSkillSyncConfig,
};
use crate::plugin::skill::registry::SkillRegistry;

#[derive(Debug, Serialize)]
pub struct SyncBuiltinSkillsResult {
    pub installed: Vec<String>,
    pub skipped: Vec<String>,
}

#[tauri::command]
pub async fn sync_builtin_skills(
    config: tauri::State<'_, GlobalSkillSyncConfig>,
    auth_manager: tauri::State<'_, Arc<AuthManager>>,
    registry: tauri::State<'_, Arc<Mutex<SkillRegistry>>>,
) -> Result<SyncBuiltinSkillsResult, String> {
    let session_key = auth_manager
        .get_session_key()
        .await
        .map_err(|e| format!("not logged in: {}", e))?;

    let cfg: GlobalSkillSyncConfig = (*config).clone();
    let registry_arc: Arc<Mutex<SkillRegistry>> = registry.inner().clone();
    let skill_roots = cfg.skill_roots_for_reload.clone();

    let report = sync_skill_packages_from_server(cfg, BASE_URL.to_string(), session_key)
        .await
        .map_err(|e| e.to_string())?;

    if !report.installed.is_empty() {
        reload_skill_registry(&skill_roots, &registry_arc);
    }

    Ok(SyncBuiltinSkillsResult {
        installed: report.installed,
        skipped: report.skipped,
    })
}
