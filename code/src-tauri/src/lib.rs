mod commands;
mod models;
mod llm;
mod search;
mod storage;
mod python;
mod plugin;

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
            let file_mgr = Arc::new(storage::file_manager::FileManager::new(fm_path.clone()));

            // Configure logging — write to workspace/logs/ for both debug and release
            let logs_dir = fm_path.join("logs");
            std::fs::create_dir_all(&logs_dir).ok();
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
                    .target(tauri_plugin_log::Target::new(
                        tauri_plugin_log::TargetKind::Folder {
                            path: logs_dir.clone(),
                            file_name: Some("renlijia".into()),
                        }
                    ))
                    .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
                    .max_file_size(5_000_000) // 5MB per file
                    .build(),
            )?;

            // Auto-cleanup old log files (> 7 days)
            cleanup_old_logs(&logs_dir, 7);

            // Cleanup stale temp files from previous sessions (code_*.py)
            cleanup_temp_dir(&fm_path.join("temp"));

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

            // Initialize plugin registries
            let tool_registry = Arc::new(plugin::ToolRegistry::new());
            let skill_registry = Arc::new(plugin::SkillRegistry::new("daily-assistant"));

            // Register builtin tools and skills
            tauri::async_runtime::block_on(async {
                plugin::builtin::tools::register_builtin_tools(&tool_registry).await;
                plugin::builtin::skills::register_builtin_skills(&skill_registry).await;

                // Scan bundled plugin directory for external plugins
                let plugins_dir = resource_dir.join("plugins");
                if plugins_dir.exists() {
                    scan_external_plugins(
                        &plugins_dir,
                        &tool_registry,
                        &skill_registry,
                        file_mgr.workspace_path(),
                    ).await;
                }
            });

            log::info!("Plugin system initialized");

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

            // Initialize Python session manager for persistent REPL sessions
            let session_mgr = Arc::new(
                python::session::PythonSessionManager::new(fm_path.clone(), Some(app.handle()))
            );

            // Start idle session reaper (every 5 minutes)
            {
                let session_mgr_clone = session_mgr.clone();
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
                    loop {
                        interval.tick().await;
                        session_mgr_clone.reap_idle().await;
                    }
                });
            }

            // Register managed state
            app.manage(db);
            app.manage(file_mgr);
            app.manage(gateway);
            app.manage(secure_storage);
            app.manage(tool_registry);
            app.manage(skill_registry);
            app.manage(session_mgr);

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
            file::open_file_by_name,
            file::reveal_file_by_name,
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
            workspace::open_logs_directory,
            // Export commands
            export::export_conversation,
            // Plugin commands
            commands::plugin::list_tools,
            commands::plugin::list_skills,
            commands::plugin::get_plugin_info,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                // Graceful shutdown: checkpoint all Python sessions before exit.
                // block_on is safe here — the event loop is already shutting down.
                let session_mgr = app_handle.state::<Arc<python::session::PythonSessionManager>>();
                tauri::async_runtime::block_on(session_mgr.shutdown_all());
            }
        });
}

/// Scan bundled plugin directories for external plugins (resource_dir/plugins/).
async fn scan_external_plugins(
    plugins_dir: &std::path::Path,
    tool_registry: &plugin::ToolRegistry,
    skill_registry: &plugin::SkillRegistry,
    workspace_path: &std::path::Path,
) {
    let entries = match std::fs::read_dir(plugins_dir) {
        Ok(e) => e,
        Err(e) => {
            log::warn!("Failed to read plugins directory: {}", e);
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let manifest_path = path.join("plugin.toml");
        if !manifest_path.exists() {
            continue;
        }

        let manifest_content = match std::fs::read_to_string(&manifest_path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("Failed to read {:?}: {}", manifest_path, e);
                continue;
            }
        };

        let manifest = match plugin::manifest::parse_plugin_manifest(&manifest_content) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("Invalid plugin.toml in {:?}: {}", path, e);
                continue;
            }
        };

        match manifest.plugin.plugin_type.as_str() {
            "tool" => {
                if manifest.plugin.runtime.as_deref() == Some("python") {
                    match plugin::python_bridge::PythonToolBridge::from_manifest(&manifest, path.clone()) {
                        Ok(mut bridge) => {
                            if let Err(e) = bridge.load_schema(workspace_path).await {
                                log::warn!("Failed to load schema for plugin '{}': {}", manifest.plugin.id, e);
                                continue;
                            }
                            tool_registry.register(
                                std::sync::Arc::new(bridge),
                                "plugin",
                            ).await;
                            log::info!("Loaded Python tool plugin: {}", manifest.plugin.id);
                        }
                        Err(e) => {
                            log::warn!("Failed to create Python tool bridge for '{}': {}", manifest.plugin.id, e);
                        }
                    }
                }
            }
            "skill" => {
                match plugin::declarative_skill::DeclarativeSkill::load(&manifest, &path) {
                    Ok(skill) => {
                        skill_registry.register(
                            std::sync::Arc::new(skill),
                            "plugin",
                        ).await;
                        log::info!("Loaded declarative skill plugin: {}", manifest.plugin.id);
                    }
                    Err(e) => {
                        log::warn!("Failed to load skill plugin '{}': {}", manifest.plugin.id, e);
                    }
                }
            }
            other => {
                log::warn!("Unknown plugin type '{}' in {:?}", other, manifest_path);
            }
        }
    }
}

/// Remove log files older than `retention_days` days from the logs directory.
fn cleanup_old_logs(logs_dir: &std::path::Path, retention_days: u64) {
    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(retention_days * 86400));
    let cutoff = match cutoff {
        Some(c) => c,
        None => return,
    };

    let entries = match std::fs::read_dir(logs_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Ok(meta) = path.metadata() {
            if let Ok(modified) = meta.modified() {
                if modified < cutoff {
                    if std::fs::remove_file(&path).is_ok() {
                        // Log may not be available yet during startup, use eprintln
                        eprintln!("Cleaned up old log file: {:?}", path);
                    }
                }
            }
        }
    }
}

/// Remove stale Python temp files (code_*.py) from the workspace temp directory.
///
/// These files are normally cleaned up after each execution, but if the app
/// crashes or is force-quit during Python execution, temp files may be left behind.
fn cleanup_temp_dir(temp_dir: &std::path::Path) {
    let entries = match std::fs::read_dir(temp_dir) {
        Ok(e) => e,
        Err(_) => return, // Directory doesn't exist yet — fine
    };

    let mut count = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("code_") && name.ends_with(".py") {
                    if std::fs::remove_file(&path).is_ok() {
                        count += 1;
                    }
                }
            }
        }
    }
    if count > 0 {
        eprintln!("Cleaned up {} stale temp Python files", count);
    }
}
