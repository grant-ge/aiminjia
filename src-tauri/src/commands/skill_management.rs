use tauri::AppHandle;
use tauri::Manager;
use std::path::PathBuf;

#[derive(serde::Serialize)]
pub struct CustomSkillInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub path: String,
    pub enabled: bool,
}

/// List all installed custom plugins.
#[tauri::command]
pub async fn list_custom_skills(app: AppHandle) -> Result<Vec<CustomSkillInfo>, String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let custom_dir = app_data.join("custom_plugins");

    if !custom_dir.is_dir() {
        return Ok(vec![]);
    }

    let mut skills = Vec::new();
    for entry in std::fs::read_dir(&custom_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() && !path.file_name().unwrap().to_string_lossy().starts_with('_') {
            let manifest_path = path.join("plugin.toml");
            if manifest_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                    if let Ok(manifest) = toml::from_str::<toml::Value>(&content) {
                        let plugin = manifest
                            .get("plugin")
                            .cloned()
                            .unwrap_or(toml::Value::Table(Default::default()));
                        skills.push(CustomSkillInfo {
                            id: plugin
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            name: plugin
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            description: plugin
                                .get("description")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            path: path.to_string_lossy().to_string(),
                            enabled: !path
                                .file_name()
                                .unwrap()
                                .to_string_lossy()
                                .starts_with('_'),
                        });
                    }
                }
            }
        }
    }
    Ok(skills)
}

/// Install a skill from a directory path (copy to custom_plugins/).
#[tauri::command]
pub async fn install_custom_skill(
    app: AppHandle,
    source_path: String,
) -> Result<String, String> {
    let source = PathBuf::from(&source_path);
    if !source.is_dir() {
        return Err("Source path is not a directory".to_string());
    }

    let manifest_path = source.join("plugin.toml");
    if !manifest_path.exists() {
        return Err("No plugin.toml found in source directory".to_string());
    }

    // Read plugin ID from manifest
    let content = std::fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let manifest: toml::Value = toml::from_str(&content).map_err(|e| e.to_string())?;
    let plugin_id = manifest
        .get("plugin")
        .and_then(|p| p.get("id"))
        .and_then(|v| v.as_str())
        .ok_or("plugin.id not found in manifest")?
        .to_string();

    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let custom_dir = app_data.join("custom_plugins");
    std::fs::create_dir_all(&custom_dir).map_err(|e| e.to_string())?;

    let dest = custom_dir.join(&plugin_id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
    }

    // Copy directory recursively
    copy_dir_recursive(&source, &dest).map_err(|e| e.to_string())?;

    Ok(format!(
        "Installed skill '{}' — restart app to activate",
        plugin_id
    ))
}

/// Uninstall a custom skill by ID.
#[tauri::command]
pub async fn uninstall_custom_skill(
    app: AppHandle,
    skill_id: String,
) -> Result<String, String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let skill_dir = app_data.join("custom_plugins").join(&skill_id);

    if !skill_dir.exists() {
        return Err(format!("Custom skill '{}' not found", skill_id));
    }

    std::fs::remove_dir_all(&skill_dir).map_err(|e| e.to_string())?;
    Ok(format!(
        "Uninstalled skill '{}' — restart app to take effect",
        skill_id
    ))
}

fn copy_dir_recursive(src: &PathBuf, dst: &PathBuf) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
