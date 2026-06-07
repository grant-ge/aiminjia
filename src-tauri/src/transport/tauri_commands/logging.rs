use std::sync::Arc;

use tauri::State;

use crate::log_level;
use crate::storage::GlobalConfigStore;

#[tauri::command]
pub fn get_log_level(global_store: State<'_, Arc<GlobalConfigStore>>) -> String {
    global_store
        .get_setting(log_level::CONFIG_KEY)
        .ok()
        .flatten()
        .unwrap_or_else(|| "info".to_string())
}

#[tauri::command]
pub fn set_log_level(
    level: String,
    global_store: State<'_, Arc<GlobalConfigStore>>,
) -> Result<(), String> {
    let valid = log_level::validate(&level)
        .ok_or_else(|| format!("invalid log level: {level}"))?;

    // Gate log::* macro calls (tracing subscriber passes all levels through).
    log::set_max_level(log_level::to_log_filter(&valid));

    global_store
        .set_setting(log_level::CONFIG_KEY, &valid)
        .map_err(|e| e.to_string())
}
