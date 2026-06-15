use std::path::{Path, PathBuf};

use log::warn;
use serde::{Deserialize, Serialize};

use super::error::StorageResult;
use super::io::{atomic_write_json, read_json_optional};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSettings {
    pub primary_model: Option<String>,
}

pub fn workspace_settings_path(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join(".aijia").join("settings.json")
}

pub fn load_workspace_settings(workspace_dir: &Path) -> WorkspaceSettings {
    let path = workspace_settings_path(workspace_dir);
    match read_json_optional::<WorkspaceSettings>(&path) {
        Ok(Some(settings)) => settings,
        Ok(None) => WorkspaceSettings::default(),
        Err(err) => {
            warn!(
                "Failed to parse workspace settings at {}: {}",
                path.display(),
                err
            );
            WorkspaceSettings::default()
        }
    }
}

pub fn save_workspace_settings(
    workspace_dir: &Path,
    settings: &WorkspaceSettings,
) -> StorageResult<()> {
    let path = workspace_settings_path(workspace_dir);
    atomic_write_json(&path, settings)?;
    Ok(())
}
