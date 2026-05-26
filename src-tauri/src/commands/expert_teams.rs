use crate::runtime::expert_team::store::{catalog_teams, ExpertTeamSnapshot};
use crate::storage::aijia_home::AiJiaHome;

#[tauri::command]
pub async fn expert_team_template_catalog() -> Result<Vec<ExpertTeamSnapshot>, String> {
    let cache_dir = AiJiaHome::from_home().expert_team_templates_cache_dir();
    catalog_teams(&cache_dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn expert_team_upgrade_conversation(
    conversation_id: String,
    target_version: String,
) -> Result<(), String> {
    log::info!(
        "[expert-team] upgrade requested conv={} target_version={}",
        conversation_id,
        target_version
    );
    Err("当前版本仅支持检测专家团新版本，升级写入将在快照同步完成后启用".to_string())
}
