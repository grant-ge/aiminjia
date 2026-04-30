use serde::Serialize;
use std::sync::Arc;
use tauri::State;

use crate::runtime::agent::definition::AgentSource;
use crate::runtime::agent::registry::AgentRegistry;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub name: String,
    pub description: String,
    pub source: String,
}

#[tauri::command]
pub async fn list_agents(
    registry: State<'_, Arc<AgentRegistry>>,
) -> Result<Vec<AgentInfo>, String> {
    Ok(registry
        .list()
        .into_iter()
        .map(|def| AgentInfo {
            name: def.name.clone(),
            description: def.description.clone(),
            source: match def.source {
                AgentSource::Builtin => "builtin".to_string(),
                AgentSource::User => "user".to_string(),
            },
        })
        .collect())
}
