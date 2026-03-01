mod commands;
mod models;
mod llm;
mod search;
mod storage;
mod python;

use std::sync::Arc;
use tauri::Manager;
use commands::chat;
use commands::export;
use commands::file;
use commands::settings;
use commands::workspace;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // Debug logging
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // Initialize app data directory
            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;

            // Initialize prompt store from external .md files
            let resource_dir = app.path().resource_dir()
                .unwrap_or_else(|_| app_data_dir.clone());
            llm::prompts::init_prompts(&resource_dir, &app_data_dir);

            // Initialize file-based storage
            let db = Arc::new(
                storage::file_store::AppStorage::new(&app_data_dir)
                    .expect("Failed to initialize file storage")
            );

            // Initialize file manager
            let workspace_path = db.get_setting("workspacePath")
                .ok()
                .flatten()
                .unwrap_or_default();
            let fm_path = if workspace_path.is_empty() {
                // Default workspace: ~/.renlijia
                let default_ws = dirs::home_dir()
                    .map(|h| h.join(".renlijia"))
                    .unwrap_or_else(|| app_data_dir.clone());
                std::fs::create_dir_all(&default_ws).ok();
                default_ws
            } else {
                let p = std::path::PathBuf::from(&workspace_path);
                std::fs::create_dir_all(&p).ok();
                p
            };
            let file_mgr = Arc::new(storage::file_manager::FileManager::new(fm_path));

            // Initialize secure storage for API key encryption
            let secure_storage: Option<Arc<storage::crypto::SecureStorage>> =
                match storage::crypto::SecureStorage::new(&app_data_dir) {
                    Ok(ss) => {
                        log::info!("SecureStorage initialized (key file in app data dir)");
                        Some(Arc::new(ss))
                    }
                    Err(e) => {
                        log::warn!("SecureStorage unavailable (API keys stored as plaintext): {}", e);
                        None
                    }
                };

            // Initialize LLM gateway
            let gateway = Arc::new(llm::gateway::LlmGateway::new(db.clone()));

            // Crash recovery: clean up any tasks that were running when app crashed
            match db.cleanup_orphaned_tasks() {
                Ok(orphaned) => {
                    for conv_id in &orphaned {
                        log::warn!("Cleaning up orphaned agent task for conversation: {}", conv_id);
                        db.reset_stuck_analysis_state(conv_id).ok();
                    }
                    if !orphaned.is_empty() {
                        log::info!("Cleaned up {} orphaned agent tasks from previous crash", orphaned.len());
                    }
                }
                Err(e) => {
                    log::warn!("Failed to cleanup orphaned tasks: {}", e);
                }
            }

            // Register managed state
            app.manage(db);
            app.manage(file_mgr);
            app.manage(gateway);
            app.manage(secure_storage);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Chat commands
            chat::send_message,
            chat::stop_streaming,
            chat::get_messages,
            chat::create_conversation,
            chat::delete_conversation,
            chat::get_conversations,
            chat::is_agent_busy,
            // File commands
            file::upload_file,
            file::open_generated_file,
            file::reveal_file_in_folder,
            file::preview_file,
            file::delete_file,
            // Settings commands
            settings::get_settings,
            settings::update_settings,
            settings::validate_api_key,
            settings::get_configured_providers,
            settings::switch_provider,
            settings::get_all_provider_keys,
            settings::update_all_provider_keys,
            // Workspace commands
            workspace::select_workspace,
            workspace::get_workspace_info,
            // Export commands
            export::export_conversation,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
