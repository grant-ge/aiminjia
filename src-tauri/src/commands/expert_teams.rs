use crate::runtime::expert_team::store::{bootstrap_teams, ExpertTeamSnapshot};

#[tauri::command]
pub async fn expert_team_template_catalog() -> Result<Vec<ExpertTeamSnapshot>, String> {
    bootstrap_teams().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn expert_team_upgrade_conversation(
    _conversation_id: String,
    _target_version: String,
) -> Result<(), String> {
    Err("专家团升级将在远程快照同步完成后启用".to_string())
}
